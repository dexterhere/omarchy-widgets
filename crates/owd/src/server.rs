use crate::conn::{self, Shared};
use crate::hub::{self, Hub};
use crate::unlock::{VaultPolicy, VaultState};
use ow_core::{AuditLog, Grants, Paths, Provider, RateLimiter};
use rustix::fs::FileType;
use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{self, ErrorKind};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

const MAX_CONNECTIONS: usize = 32;
/// Scheduler resolution.
const POLL_TICK: Duration = Duration::from_millis(50);
/// Per (widget, topic): burst of 10, 2 requests/second sustained.
const RATE_BURST: u32 = 10;
const RATE_PER_SEC: f64 = 2.0;

/// Frees a connection slot on drop, including when a handler thread panics.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub struct Server {
    // Held for the daemon's lifetime: serialises concurrent starts (flock).
    _lock: File,
    listener: UnixListener,
    path: PathBuf,
    active: Arc<AtomicUsize>,
    shared: Arc<Shared>,
    next_conn: AtomicU64,
}

impl Server {
    /// Create the socket directory (0700), refuse unsafe ones, replace a stale socket,
    /// refuse to start if another daemon is alive, bind, and chmod the socket 0600.
    pub fn bind(paths: &Paths, providers: Vec<Arc<dyn Provider>>) -> io::Result<Server> {
        Self::bind_with(paths, providers, VaultPolicy::default())
    }

    /// `bind` with an explicit vault policy (idle timeout and clock), for tests.
    pub fn bind_with(
        paths: &Paths,
        providers: Vec<Arc<dyn Provider>>,
        vault_policy: VaultPolicy,
    ) -> io::Result<Server> {
        let mut b = DirBuilder::new();
        b.recursive(true).mode(0o700);
        b.create(&paths.runtime)?;
        ow_core::check_private_dir(&paths.runtime).map_err(|e| {
            io::Error::other(format!(
                "runtime directory {}: {e}",
                paths.runtime.display()
            ))
        })?;
        // Fail closed: a bad grants file stops startup rather than allowing everything.
        let grants =
            Grants::load(&paths.grants()).map_err(|e| io::Error::other(format!("grants: {e}")))?;
        let mut sb = DirBuilder::new();
        sb.recursive(true).mode(0o700);
        sb.create(&paths.state)?;
        ow_core::check_private_dir(&paths.state).map_err(|e| {
            io::Error::other(format!("state directory {}: {e}", paths.state.display()))
        })?;
        let audit = AuditLog::open(&paths.audit())?;
        let vault = Arc::new(VaultState::new(paths.vault(), vault_policy));
        let shared = Arc::new(Shared {
            grants: RwLock::new(grants),
            grants_path: paths.grants(),
            audit: Mutex::new(audit),
            limiter: Mutex::new(RateLimiter::new(RATE_BURST, RATE_PER_SEC)),
            hub: Hub::new(providers, Arc::clone(&vault)),
            vault,
        });
        // Exclusive lock first, so two concurrent starts cannot both pass the stale check.
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(paths.runtime.join("owd.lock"))?;
        rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive)
            .map_err(|_| io::Error::new(ErrorKind::AddrInUse, "owd is already running"))?;
        let path = paths.socket();
        match std::fs::symlink_metadata(&path) {
            Ok(md) => {
                if FileType::from_raw_mode(md.mode()) != FileType::Socket {
                    return Err(io::Error::other("socket path exists and is not a socket"));
                }
                if UnixStream::connect(&path).is_ok() {
                    return Err(io::Error::new(
                        ErrorKind::AddrInUse,
                        "owd is already running",
                    ));
                }
                std::fs::remove_file(&path)?;
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        // The 0700 directory protects the window between bind and chmod.
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        Ok(Server {
            _lock: lock,
            listener,
            path,
            active: Arc::new(AtomicUsize::new(0)),
            shared,
            next_conn: AtomicU64::new(1),
        })
    }

    pub fn socket_path(&self) -> &std::path::Path {
        &self.path
    }

    /// Accept loop until `stop` is set; `reload` (SIGHUP) re-reads the grants file.
    /// Thread per connection, capped.
    // ponytail: polling accept + threads; move to tokio when providers need async.
    pub fn run(&self, stop: &AtomicBool, reload: &AtomicBool) {
        std::thread::scope(|scope| {
            // Scheduler: refresh due topics and push to subscribers.
            scope.spawn(|| {
                while !stop.load(Ordering::Relaxed) {
                    hub::poll_once(&self.shared);
                    std::thread::sleep(POLL_TICK);
                }
            });
            self.accept_loop(stop, reload);
        });
    }

    fn accept_loop(&self, stop: &AtomicBool, reload: &AtomicBool) {
        while !stop.load(Ordering::Relaxed) {
            if reload.swap(false, Ordering::Relaxed) {
                match self.shared.reload_grants() {
                    Ok(()) => eprintln!("owd: grants reloaded"),
                    Err(e) => eprintln!("owd: grants reload failed ({e}); all access denied"),
                }
            }
            match self.listener.accept() {
                Ok((stream, _)) => self.spawn(stream),
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    }

    fn spawn(&self, stream: UnixStream) {
        if self.active.fetch_add(1, Ordering::AcqRel) >= MAX_CONNECTIONS {
            self.active.fetch_sub(1, Ordering::AcqRel);
            return; // drop: over the connection cap
        }
        let slot = Slot(Arc::clone(&self.active));
        let shared = Arc::clone(&self.shared);
        let conn_id = self.next_conn.fetch_add(1, Ordering::Relaxed);
        // Builder::spawn returns Err instead of panicking when the OS refuses a thread;
        // the closure (and its Slot) is dropped, which frees the slot.
        let _ = std::thread::Builder::new().spawn(move || {
            let _slot = slot;
            let _ = stream.set_nonblocking(false);
            conn::serve(stream, &shared, conn_id);
        });
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
