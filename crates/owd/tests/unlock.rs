//! Daemon-side vault unlock over the real socket: passphrase mode, fake values only.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use ow_core::{FetchCtx, Paths, Provider, ProviderError, Sensitivity, TopicSpec};
use ow_vault::{Profile, Vault};
use owd::{Server, VaultPolicy};
use serde_json::{Map, Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const PASS: &str = "FAKE-passphrase-correct-horse";
const WRONG: &str = "FAKE-passphrase-wrong-horse!";
const TOKEN: &str = "FAKE-TOKEN-VALUE-77c1e";
const GRANTS: &str = "[[grant]]\nwidget=\"w1\"\ntopics=[\"fake.secret\",\"fake.plain\",\"fake.undeclared\"]\nfields=[\"len\",\"n\"]\n";

/// A secret-backed provider: one topic that uses its secret and one that asks for a secret it
/// did not declare. Secrets are declared per provider, so a topic that needs none lives in
/// `PlainProvider`.
struct SecretProvider;

struct PlainProvider;

static PLAIN: [TopicSpec; 1] = [TopicSpec::new(
    "fake.plain",
    Sensitivity::Low,
    Duration::from_millis(50),
    &["n"],
)];

impl Provider for PlainProvider {
    fn id(&self) -> &'static str {
        "plain"
    }
    fn topics(&self) -> &'static [TopicSpec] {
        &PLAIN
    }
    fn fetch(
        &self,
        _t: &str,
        _p: &Map<String, Value>,
        _c: &FetchCtx<'_>,
    ) -> Result<Map<String, Value>, ProviderError> {
        Ok(json!({"n": 1}).as_object().cloned().unwrap())
    }
}

static TOPICS: [TopicSpec; 2] = [
    TopicSpec::new(
        "fake.secret",
        Sensitivity::High,
        Duration::from_millis(50),
        &["len"],
    ),
    TopicSpec::new(
        "fake.undeclared",
        Sensitivity::High,
        Duration::from_millis(50),
        &["len"],
    ),
];

impl Provider for SecretProvider {
    fn id(&self) -> &'static str {
        "fake"
    }
    fn topics(&self) -> &'static [TopicSpec] {
        &TOPICS
    }
    fn secrets(&self) -> &'static [&'static str] {
        &["fake-token"]
    }
    fn fetch(
        &self,
        topic: &str,
        _p: &Map<String, Value>,
        ctx: &FetchCtx<'_>,
    ) -> Result<Map<String, Value>, ProviderError> {
        let out = |v: Value| Ok(v.as_object().cloned().unwrap());
        match topic {
            // Proves resolution without emitting the value: only its length leaves.
            "fake.secret" => out(json!({"len": ctx.secret("fake-token")?.expose().len()})),
            "fake.undeclared" => out(json!({"len": ctx.secret("other")?.expose().len()})),
            _ => Err(ProviderError::UnknownTopic),
        }
    }
}

struct Clock(Arc<Mutex<Instant>>);

impl Clock {
    fn advance(&self, secs: u64) {
        *self.0.lock().unwrap() += Duration::from_secs(secs);
    }
}

struct H {
    paths: Paths,
    stop: Arc<AtomicBool>,
    th: Option<JoinHandle<()>>,
    clock: Clock,
    _dir: tempfile::TempDir,
}

fn start(idle: Duration) -> H {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path());
    std::fs::create_dir_all(&paths.config).unwrap();
    std::fs::write(paths.grants(), GRANTS).unwrap();
    std::fs::set_permissions(paths.grants(), std::fs::Permissions::from_mode(0o600)).unwrap();
    ow_core::private_dir(&paths.data).unwrap();
    let mut v = Vault::create(&paths.vault(), PASS.as_bytes(), Profile::Standard).unwrap();
    v.set("fake-token", TOKEN).unwrap();
    v.set("other", "FAKE-other-value").unwrap();
    drop(v);
    let now = Arc::new(Mutex::new(Instant::now()));
    let n2 = Arc::clone(&now);
    let policy = VaultPolicy {
        idle,
        clock: Arc::new(move || *n2.lock().unwrap()),
    };
    let server = Server::bind_with(
        &paths,
        vec![Arc::new(SecretProvider), Arc::new(PlainProvider)],
        policy,
    )
    .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (s2, r2) = (Arc::clone(&stop), Arc::new(AtomicBool::new(false)));
    let th = std::thread::spawn(move || server.run(&s2, &r2));
    H {
        paths,
        stop,
        th: Some(th),
        clock: Clock(now),
        _dir: dir,
    }
}

impl H {
    fn ask(&self, line: &str) -> String {
        let mut s = UnixStream::connect(self.paths.socket()).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        s.write_all(line.as_bytes()).unwrap();
        let mut out = String::new();
        BufReader::new(s).read_line(&mut out).unwrap();
        out
    }

    fn unlock(&self, pass: &str) -> Value {
        let line = ow_proto::unlock_request(1, pass.as_bytes()).unwrap();
        serde_json::from_str(&self.ask(&line)).unwrap()
    }

    fn status(&self) -> Value {
        serde_json::from_str(&self.ask("{\"v\":0,\"id\":2,\"op\":\"vault.status\"}\n")).unwrap()
    }

    fn snap(&self, topic: &str) -> Value {
        serde_json::from_str(&self.ask(&format!(
            "{{\"v\":0,\"id\":3,\"op\":\"snapshot\",\"widget\":\"w1\",\"topic\":\"{topic}\"}}\n"
        )))
        .unwrap()
    }

    fn audit(&self) -> String {
        std::fs::read_to_string(self.paths.audit()).unwrap_or_default()
    }

    fn wait_locked(&self) {
        for _ in 0..100 {
            if self.status()["state"] == "locked" {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("vault did not lock");
    }
}

impl Drop for H {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.th.take() {
            let _ = t.join();
        }
    }
}

fn code(v: &Value) -> &str {
    v["error"]["code"].as_str().unwrap_or("")
}

#[test]
fn starts_locked_and_status_shows_no_values() {
    let h = start(Duration::from_secs(900));
    let s = h.status();
    assert_eq!(s["ok"], true);
    assert_eq!(s["state"], "locked");
    assert!(s.get("names").is_none() && s.get("count").is_none());
    h.unlock(PASS);
    let s = h.status();
    assert_eq!(s["state"], "unlocked");
    assert_eq!(s["count"], 2);
    assert_eq!(s["names"], json!(["fake-token", "other"]));
    assert_eq!(s["idleSecsLeft"], 900);
    let raw = s.to_string();
    assert!(!raw.contains(TOKEN) && !raw.contains(PASS));
}

#[test]
fn secret_topic_is_locked_until_unlock_and_plain_topic_is_not() {
    let h = start(Duration::from_secs(900));
    let r = h.snap("fake.secret");
    assert_eq!(code(&r), "locked", "{r}");
    assert_eq!(r["error"]["topic"], "fake.secret");
    // Subscribing is refused the same way.
    let sub = h.ask(
        "{\"v\":0,\"id\":4,\"op\":\"subscribe\",\"widget\":\"w1\",\"topic\":\"fake.secret\"}\n",
    );
    assert!(sub.contains("\"locked\""), "{sub}");
    // A topic with no secret is unaffected by the lock.
    assert_eq!(h.snap("fake.plain")["data"]["n"], 1);

    assert_eq!(h.unlock(PASS)["state"], "unlocked");
    let r = h.snap("fake.secret");
    assert_eq!(r["data"]["len"], TOKEN.len(), "{r}");
    assert!(!r.to_string().contains(TOKEN));
}

#[test]
fn a_provider_cannot_resolve_a_secret_it_did_not_declare() {
    let h = start(Duration::from_secs(900));
    h.unlock(PASS);
    // "other" is in the vault, but fake.undeclared's provider declared only fake-token.
    assert_eq!(code(&h.snap("fake.undeclared")), "provider_error");
}

#[test]
fn wrong_passphrase_stays_locked_with_a_generic_error() {
    let h = start(Duration::from_secs(900));
    let raw = h.ask(&ow_proto::unlock_request(9, WRONG.as_bytes()).unwrap());
    assert_eq!(raw, "{\"v\":0,\"id\":9,\"error\":{\"code\":\"denied\"}}\n");
    assert_eq!(h.status()["state"], "locked");
    assert_eq!(code(&h.snap("fake.secret")), "locked");
}

#[test]
fn five_failures_start_a_lockout_that_blocks_even_the_right_passphrase() {
    let h = start(Duration::from_secs(900));
    for _ in 0..owd::FREE_FAILURES {
        assert_eq!(code(&h.unlock(WRONG)), "denied");
    }
    assert_eq!(code(&h.unlock(PASS)), "rate_limited");
    let s = h.status();
    assert_eq!(s["state"], "locked");
    assert_eq!(s["lockoutSecsLeft"], owd::LOCKOUT_BASE.as_secs());
    // Time passes (injected clock): the lockout ends and the right passphrase works.
    h.clock.advance(owd::LOCKOUT_BASE.as_secs());
    assert_eq!(h.unlock(PASS)["state"], "unlocked");
    assert!(h.status().get("lockoutSecsLeft").is_none());
}

#[test]
fn lock_drops_access_and_purges_the_cache() {
    let h = start(Duration::from_secs(900));
    h.unlock(PASS);
    // Warm the cache and a subscription.
    assert_eq!(h.snap("fake.secret")["data"]["len"], TOKEN.len());
    let r = h.ask("{\"v\":0,\"id\":5,\"op\":\"vault.lock\"}\n");
    assert!(r.contains("\"state\":\"locked\""), "{r}");
    assert_eq!(h.status()["state"], "locked");
    // No cached copy is served: the answer is locked, not the last value.
    assert_eq!(code(&h.snap("fake.secret")), "locked");
    // Unlocking again works and the data comes back fresh.
    h.unlock(PASS);
    assert_eq!(h.snap("fake.secret")["data"]["len"], TOKEN.len());
}

#[test]
fn subscribers_stop_receiving_after_lock() {
    let h = start(Duration::from_secs(900));
    h.unlock(PASS);
    let mut s = UnixStream::connect(h.paths.socket()).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(
        b"{\"v\":0,\"id\":1,\"op\":\"subscribe\",\"widget\":\"w1\",\"topic\":\"fake.secret\"}\n",
    )
    .unwrap();
    let mut r = BufReader::new(s);
    let mut l = String::new();
    r.read_line(&mut l).unwrap(); // reply
    l.clear();
    r.read_line(&mut l).unwrap(); // first push
    assert!(l.contains("\"len\""), "{l}");
    h.ask("{\"v\":0,\"id\":5,\"op\":\"vault.lock\"}\n");
    // Drain anything already in flight, then nothing more may arrive.
    r.get_ref()
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    while r.read_line(&mut l).is_ok_and(|n| n > 0) {
        l.clear();
    }
    r.get_ref()
        .set_read_timeout(Some(Duration::from_millis(600)))
        .unwrap();
    l.clear();
    assert!(r.read_line(&mut l).is_err(), "push after lock: {l}");
}

#[test]
fn idle_timeout_locks_the_vault_and_audits_it() {
    let h = start(Duration::from_secs(60));
    h.unlock(PASS);
    h.clock.advance(59);
    assert_eq!(h.status()["state"], "unlocked");
    h.clock.advance(1);
    h.wait_locked();
    assert_eq!(code(&h.snap("fake.secret")), "locked");
    // The scheduler reported it once.
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(h.audit().matches("\"op\":\"autolock\"").count(), 1);
}

#[test]
fn audit_records_events_without_secret_material() {
    let h = start(Duration::from_secs(900));
    h.unlock(WRONG);
    h.unlock(PASS);
    h.snap("fake.secret");
    h.ask("{\"v\":0,\"id\":5,\"op\":\"vault.lock\"}\n");
    let a = h.audit();
    for op in ["unlock", "lock"] {
        assert!(a.contains(&format!("\"op\":\"{op}\"")), "{a}");
    }
    assert!(
        a.contains("\"op\":\"unlock\",\"decision\":\"deny\"")
            && a.contains("\"op\":\"unlock\",\"decision\":\"allow\""),
        "{a}"
    );
    for needle in [PASS, WRONG, TOKEN, "FAKE-other-value"] {
        assert!(!a.contains(needle), "audit leaked {needle}");
    }
    let hexed: String = PASS.bytes().map(|b| format!("{b:02x}")).collect();
    assert!(!a.contains(&hexed));
}

#[test]
fn widget_shaped_admin_requests_are_refused_and_audited_without_payload() {
    let h = start(Duration::from_secs(900));
    let hexed: String = PASS.bytes().map(|b| format!("{b:02x}")).collect();
    // Admin op that claims a widget identity.
    let r = h.ask(&format!(
        "{{\"v\":0,\"id\":6,\"op\":\"vault.unlock\",\"widget\":\"w1\",\"passphrase\":\"{hexed}\"}}\n"
    ));
    assert_eq!(r, "{\"v\":0,\"id\":6,\"error\":{\"code\":\"denied\"}}\n");
    assert_eq!(h.status()["state"], "locked", "must not have unlocked");
    // Data ops that name an admin op or carry a passphrase field.
    let r = h.ask(&format!(
        "{{\"v\":0,\"id\":7,\"op\":\"snapshot\",\"widget\":\"w1\",\"topic\":\"vault.unlock\",\"passphrase\":\"{PASS}\"}}\n"
    ));
    assert_eq!(code(&serde_json::from_str(&r).unwrap()), "denied");
    let a = h.audit();
    assert!(
        a.contains("\"op\":\"vault.unlock\",\"decision\":\"deny\""),
        "{a}"
    );
    assert!(!a.contains(PASS) && !a.contains(&hexed), "{a}");
    assert!(!r.contains(PASS));
}

#[test]
fn malformed_unlock_frames_never_echo_the_passphrase() {
    let h = start(Duration::from_secs(900));
    let hexed: String = PASS.bytes().map(|b| format!("{b:02x}")).collect();
    for line in [
        format!("{{\"v\":0,\"id\":8,\"op\":\"vault.unlock\",\"passphrase\":\"{hexed}zz\"}}\n"),
        format!("{{\"v\":0,\"id\":8,\"op\":\"vault.unlock\",\"pass\":\"{PASS}\"}}\n"),
        format!("{{\"v\":1,\"id\":8,\"op\":\"vault.unlock\",\"passphrase\":\"{hexed}\"}}\n"),
        format!("{{\"v\":0,\"id\":8,\"op\":\"vault.unlock\",\"passphrase\":\"{PASS}\\u0041\"}}\n"),
    ] {
        let r = h.ask(&line);
        assert!(r.contains("\"error\""), "{r}");
        assert!(!r.contains(PASS) && !r.contains(&hexed), "{r}");
    }
    assert_eq!(h.status()["state"], "locked");
    let a = h.audit();
    assert!(!a.contains(PASS) && !a.contains(&hexed));
}
