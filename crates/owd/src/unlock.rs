//! The daemon's unlocked-vault state: passphrase mode only (docs/DECISIONS.md AUTH-UNLOCK).
//!
//! The only copy of the key material is the `ow_vault::Vault` held in `Inner`; its keys and
//! records are zeroizing buffers, and dropping it (lock, idle expiry, shutdown) wipes them.
//! Nothing in this module logs, formats or returns a passphrase, key or value; errors are
//! categories. Time comes from an injectable clock so the policy can be tested without sleeping.
use crate::conn::lock;
use ow_core::{ProviderError, SecretHandle, SecretSource};
use ow_vault::Vault;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

/// Locks itself when no secret has been resolved for this long. A visible widget whose
/// provider polls keeps the vault open, by design: idle means "nothing is using it".
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// Consecutive wrong passphrases tolerated before a lockout starts.
pub const FREE_FAILURES: u32 = 5;
/// First lockout; each further consecutive failure doubles it.
pub const LOCKOUT_BASE: Duration = Duration::from_secs(30);
/// Longest lockout.
pub const LOCKOUT_MAX: Duration = Duration::from_secs(15 * 60);

pub type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

/// Tunables, with the clock injectable for tests.
#[derive(Clone)]
pub struct VaultPolicy {
    pub idle: Duration,
    pub clock: Clock,
}

impl Default for VaultPolicy {
    fn default() -> Self {
        VaultPolicy {
            idle: IDLE_TIMEOUT,
            clock: Arc::new(Instant::now),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UnlockError {
    /// Wrong passphrase, missing, unreadable or corrupt vault: deliberately one answer.
    /// `locked_out` is true when this failure started (or extended) a lockout.
    Denied { locked_out: bool },
    /// Too many failures: no attempt is made until the lockout ends.
    LockedOut,
    /// Another attempt is running; attempts are serialised so guesses cannot be parallel.
    Busy,
}

pub(crate) struct Status {
    pub unlocked: bool,
    pub names: Vec<String>,
    pub idle_left: Option<Duration>,
    pub lockout_left: Option<Duration>,
}

struct Inner {
    vault: Option<Vault>,
    last_use: Instant,
    fails: u32,
    lockout_until: Option<Instant>,
    /// Set when the idle timer locked the vault, so the next scheduler tick purges and audits.
    purge_due: bool,
}

pub(crate) struct VaultState {
    path: PathBuf,
    policy: VaultPolicy,
    attempt: Mutex<()>,
    /// Bumped whenever the key changes (unlock, lock, idle lock), before caches are purged.
    epoch: AtomicU64,
    inner: Mutex<Inner>,
}

fn lockout_for(fails: u32) -> Option<Duration> {
    let over = fails.checked_sub(FREE_FAILURES)?;
    Some((LOCKOUT_BASE * 2u32.saturating_pow(over.min(16))).min(LOCKOUT_MAX))
}

impl VaultState {
    pub(crate) fn new(path: PathBuf, policy: VaultPolicy) -> Self {
        let now = (policy.clock)();
        VaultState {
            path,
            policy,
            attempt: Mutex::new(()),
            epoch: AtomicU64::new(0),
            inner: Mutex::new(Inner {
                vault: None,
                last_use: now,
                fails: 0,
                lockout_until: None,
                purge_due: false,
            }),
        }
    }

    fn now(&self) -> Instant {
        (self.policy.clock)()
    }

    /// Lock if idle for too long. Caller holds `inner`.
    fn expire(&self, g: &mut Inner, now: Instant) {
        if g.vault.is_some() && now.saturating_duration_since(g.last_use) >= self.policy.idle {
            self.epoch.fetch_add(1, Ordering::SeqCst);
            g.vault = None; // drop wipes keys and records
            g.purge_due = true;
        }
    }

    /// Try to unlock with `pass`. The Argon2 derivation runs without holding the state mutex.
    pub(crate) fn unlock(&self, pass: &[u8]) -> Result<(), UnlockError> {
        let _attempt = match self.attempt.try_lock() {
            Ok(g) => g,
            Err(TryLockError::Poisoned(p)) => p.into_inner(),
            Err(TryLockError::WouldBlock) => return Err(UnlockError::Busy),
        };
        let now = self.now();
        if lock(&self.inner).lockout_until.is_some_and(|t| now < t) {
            return Err(UnlockError::LockedOut);
        }
        let opened = Vault::open(&self.path, pass);
        let now = self.now();
        let mut g = lock(&self.inner);
        match opened {
            Ok(v) => {
                self.epoch.fetch_add(1, Ordering::SeqCst);
                g.vault = Some(v);
                g.last_use = now;
                g.fails = 0;
                g.lockout_until = None;
                g.purge_due = false;
                Ok(())
            }
            Err(_) => {
                g.fails = g.fails.saturating_add(1);
                let wait = lockout_for(g.fails);
                g.lockout_until = wait.map(|w| now + w);
                Err(UnlockError::Denied {
                    locked_out: wait.is_some(),
                })
            }
        }
    }

    /// Drop the key. True if it was unlocked.
    pub(crate) fn lock(&self) -> bool {
        let mut g = lock(&self.inner);
        self.epoch.fetch_add(1, Ordering::SeqCst);
        g.vault.take().is_some()
    }

    /// Key epoch: changes on every unlock, lock and idle lock.
    pub(crate) fn generation(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }

    pub(crate) fn is_unlocked(&self) -> bool {
        let mut g = lock(&self.inner);
        let now = self.now();
        self.expire(&mut g, now);
        g.vault.is_some()
    }

    /// Run the idle check and report whether the idle timer locked the vault since the last
    /// call (so the caller can purge caches and audit once).
    pub(crate) fn tick(&self) -> bool {
        let mut g = lock(&self.inner);
        let now = self.now();
        self.expire(&mut g, now);
        std::mem::take(&mut g.purge_due)
    }

    /// Does not count as use: asking for status never keeps the vault open.
    pub(crate) fn status(&self) -> Status {
        let mut g = lock(&self.inner);
        let now = self.now();
        self.expire(&mut g, now);
        Status {
            unlocked: g.vault.is_some(),
            names: g
                .vault
                .as_ref()
                .map(|v| v.names().map(str::to_owned).collect())
                .unwrap_or_default(),
            idle_left: g.vault.as_ref().map(|_| {
                self.policy
                    .idle
                    .saturating_sub(now.saturating_duration_since(g.last_use))
            }),
            lockout_left: g
                .lockout_until
                .map(|t| t.saturating_duration_since(now))
                .filter(|d| !d.is_zero()),
        }
    }
}

impl SecretSource for VaultState {
    fn secret(&self, name: &str) -> Result<SecretHandle, ProviderError> {
        let mut g = lock(&self.inner);
        let now = self.now();
        self.expire(&mut g, now);
        let Some(v) = g.vault.as_ref() else {
            return Err(ProviderError::Locked);
        };
        let handle = v.get(name).map(SecretHandle::new);
        if handle.is_some() {
            g.last_use = now;
        }
        handle.ok_or(ProviderError::NoSecret)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use ow_vault::Profile;

    const PW: &[u8] = b"FAKE correct horse battery";

    struct Fixture {
        _d: tempfile::TempDir,
        now: Arc<Mutex<Instant>>,
        st: VaultState,
    }

    impl Fixture {
        fn new() -> Fixture {
            let d = tempfile::tempdir().unwrap();
            let path = d.path().join("v.owv");
            let mut v = Vault::create(&path, PW, Profile::Standard).unwrap();
            v.set("tok", "FAKE-secret-value").unwrap();
            drop(v);
            let now = Arc::new(Mutex::new(Instant::now()));
            let n2 = Arc::clone(&now);
            let st = VaultState::new(
                path,
                VaultPolicy {
                    idle: Duration::from_secs(60),
                    clock: Arc::new(move || *n2.lock().unwrap()),
                },
            );
            Fixture { _d: d, now, st }
        }

        fn advance(&self, secs: u64) {
            *self.now.lock().unwrap() += Duration::from_secs(secs);
        }
    }

    #[test]
    fn starts_locked_and_resolves_nothing() {
        let f = Fixture::new();
        assert!(!f.st.is_unlocked());
        assert_eq!(f.st.secret("tok").err(), Some(ProviderError::Locked));
    }

    #[test]
    fn unlock_then_resolve_then_lock_wipes_access() {
        let f = Fixture::new();
        f.st.unlock(PW).unwrap();
        assert_eq!(f.st.secret("tok").unwrap().expose(), "FAKE-secret-value");
        assert_eq!(f.st.secret("nope").err(), Some(ProviderError::NoSecret));
        let s = f.st.status();
        assert!(s.unlocked && s.names == ["tok"]);
        assert!(f.st.lock());
        assert!(!f.st.lock());
        assert_eq!(f.st.secret("tok").err(), Some(ProviderError::Locked));
        assert!(f.st.status().names.is_empty());
    }

    #[test]
    fn wrong_passphrase_stays_locked_with_one_generic_answer() {
        let f = Fixture::new();
        assert_eq!(
            f.st.unlock(b"FAKE wrong passphrase"),
            Err(UnlockError::Denied { locked_out: false })
        );
        assert!(!f.st.is_unlocked());
        // A missing vault gives the same answer as a wrong passphrase.
        let missing = VaultState::new(f._d.path().join("none.owv"), VaultPolicy::default());
        assert_eq!(
            missing.unlock(PW),
            Err(UnlockError::Denied { locked_out: false })
        );
    }

    #[test]
    fn lockout_after_five_failures_doubles_and_caps() {
        let f = Fixture::new();
        for i in 1..=FREE_FAILURES {
            let r = f.st.unlock(b"FAKE wrong passphrase");
            assert_eq!(
                r,
                Err(UnlockError::Denied {
                    locked_out: i == FREE_FAILURES
                })
            );
        }
        // Locked out: even the right passphrase is not tried.
        assert_eq!(f.st.unlock(PW), Err(UnlockError::LockedOut));
        assert_eq!(f.st.status().lockout_left, Some(LOCKOUT_BASE));
        f.advance(29);
        assert_eq!(f.st.unlock(PW), Err(UnlockError::LockedOut));
        f.advance(1);
        // Next failure doubles the wait.
        assert!(matches!(
            f.st.unlock(b"FAKE wrong passphrase"),
            Err(UnlockError::Denied { locked_out: true })
        ));
        assert_eq!(f.st.status().lockout_left, Some(LOCKOUT_BASE * 2));
        // Far past the cap the wait stays at the maximum.
        for _ in 0..20 {
            f.advance(LOCKOUT_MAX.as_secs());
            let _ = f.st.unlock(b"FAKE wrong passphrase");
        }
        assert_eq!(f.st.status().lockout_left, Some(LOCKOUT_MAX));
        // After the lockout the right passphrase works and resets the counter.
        f.advance(LOCKOUT_MAX.as_secs());
        f.st.unlock(PW).unwrap();
        assert_eq!(f.st.status().lockout_left, None);
        f.st.lock();
        assert_eq!(
            f.st.unlock(b"FAKE wrong passphrase"),
            Err(UnlockError::Denied { locked_out: false })
        );
    }

    #[test]
    fn idle_timeout_locks_and_use_extends_it() {
        let f = Fixture::new();
        f.st.unlock(PW).unwrap();
        f.advance(59);
        assert_eq!(f.st.status().idle_left, Some(Duration::from_secs(1)));
        // Status is not use: it did not extend the timer. A secret read is use.
        f.st.secret("tok").unwrap();
        f.advance(59);
        assert!(f.st.is_unlocked());
        f.advance(1);
        assert!(!f.st.is_unlocked());
        assert_eq!(f.st.secret("tok").err(), Some(ProviderError::Locked));
        // The scheduler is told exactly once.
        assert!(f.st.tick());
        assert!(!f.st.tick());
    }

    #[test]
    fn tick_alone_notices_expiry() {
        let f = Fixture::new();
        f.st.unlock(PW).unwrap();
        assert!(!f.st.tick());
        f.advance(60);
        assert!(f.st.tick());
        assert!(!f.st.status().unlocked);
    }

    #[test]
    fn concurrent_attempts_are_refused_not_run_in_parallel() {
        let f = Fixture::new();
        let _held = f.st.attempt.lock().unwrap();
        assert_eq!(f.st.unlock(PW), Err(UnlockError::Busy));
    }
}
