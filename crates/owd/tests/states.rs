//! State categories end to end (docs/AUTH_PLAN.md 5.2 item 9, DAEMON_SPEC section 5 "States"):
//! a scripted provider fails in each way a network provider can, and the real server loop must
//! report exactly the documented category (an enum plus at most one number), keep serving the
//! last good value as stale, and never forward text.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use ow_core::{FetchCtx, Notice, Paths, Provider, ProviderError, Sensitivity, TopicSpec};
use ow_proto::{State, StateInfo};
use ow_vault::{Profile, Vault};
use owd::Server;
use serde_json::{Map, Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PASS: &str = "FAKE-passphrase-correct-horse";
const GRANTS: &str = "[[grant]]\nwidget=\"w1\"\ntopics=[\"fake.state\",\"fake.needs\",\"github.prs\",\"fake.hold\"]\nfields=[\"n\"]\n";

type Step = (Result<u64, ProviderError>, Option<Notice>);

/// Plays back a script: each fetch pops one step; an empty script repeats the last answer.
struct Scripted {
    steps: Mutex<VecDeque<Step>>,
    last: Mutex<Step>,
    fetches: AtomicUsize,
}

static TOPICS: [TopicSpec; 1] = [TopicSpec::new(
    "fake.state",
    Sensitivity::High,
    Duration::from_millis(50),
    &["n"],
)];

impl Scripted {
    fn new(steps: Vec<Step>) -> Arc<Self> {
        Arc::new(Scripted {
            steps: Mutex::new(steps.into()),
            last: Mutex::new((Err(ProviderError::Unavailable), None)),
            fetches: 0.into(),
        })
    }

    fn push(&self, step: Step) {
        self.steps.lock().unwrap().push_back(step);
    }
}

impl Provider for Scripted {
    fn id(&self) -> &'static str {
        "scripted"
    }
    fn topics(&self) -> &'static [TopicSpec] {
        &TOPICS
    }
    fn fetch(
        &self,
        _t: &str,
        _p: &Map<String, Value>,
        ctx: &FetchCtx<'_>,
    ) -> Result<Map<String, Value>, ProviderError> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        let step = {
            let mut q = self.steps.lock().unwrap();
            let mut last = self.last.lock().unwrap();
            if let Some(s) = q.pop_front() {
                *last = s;
            }
            *last
        };
        if let Some(n) = step.1 {
            ctx.notify(n);
        }
        step.0.map(|n| {
            json!({"n": n, "secret_extra": "FAKE-undeclared"})
                .as_object()
                .cloned()
                .unwrap()
        })
    }
}

/// Needs a vault secret that the vault does not hold.
struct Needs;

static NEEDS: [TopicSpec; 1] = [TopicSpec::new(
    "fake.needs",
    Sensitivity::High,
    Duration::from_millis(50),
    &["n"],
)];

impl Provider for Needs {
    fn id(&self) -> &'static str {
        "needs"
    }
    fn topics(&self) -> &'static [TopicSpec] {
        &NEEDS
    }
    fn secrets(&self) -> &'static [&'static str] {
        &["github-missing"]
    }
    fn fetch(
        &self,
        _t: &str,
        _p: &Map<String, Value>,
        ctx: &FetchCtx<'_>,
    ) -> Result<Map<String, Value>, ProviderError> {
        let n = ctx.with_secret("github-missing", |s| s.len())?;
        Ok(json!({"n": n}).as_object().cloned().unwrap())
    }
}

struct H {
    paths: Paths,
    stop: Arc<AtomicBool>,
    th: Option<std::thread::JoinHandle<()>>,
    _dir: tempfile::TempDir,
}

fn start(providers: Vec<Arc<dyn Provider>>) -> H {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path());
    std::fs::create_dir_all(&paths.config).unwrap();
    std::fs::write(paths.grants(), GRANTS).unwrap();
    std::fs::set_permissions(paths.grants(), std::fs::Permissions::from_mode(0o600)).unwrap();
    ow_core::private_dir(&paths.data).unwrap();
    let mut v = Vault::create(&paths.vault(), PASS.as_bytes(), Profile::Standard).unwrap();
    v.set("other", "FAKE-other-value").unwrap();
    drop(v);
    let server = Server::bind(&paths, providers).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (s2, r2) = (Arc::clone(&stop), Arc::new(AtomicBool::new(false)));
    let th = std::thread::spawn(move || server.run(&s2, &r2));
    H {
        paths,
        stop,
        th: Some(th),
        _dir: dir,
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

impl H {
    fn ask_as(&self, widget: &str, topic: &str) -> Value {
        let mut s = UnixStream::connect(self.paths.socket()).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let line = format!(
            "{{\"v\":0,\"id\":3,\"op\":\"snapshot\",\"widget\":\"{widget}\",\"topic\":\"{topic}\"}}\n"
        );
        s.write_all(line.as_bytes()).unwrap();
        let mut out = String::new();
        BufReader::new(s).read_line(&mut out).unwrap();
        serde_json::from_str(&out).unwrap()
    }

    fn snap(&self, topic: &str) -> Value {
        self.ask_as("w1", topic)
    }

    fn unlock(&self) {
        let mut s = UnixStream::connect(self.paths.socket()).unwrap();
        s.write_all(
            ow_proto::unlock_request(1, PASS.as_bytes())
                .unwrap()
                .as_bytes(),
        )
        .unwrap();
        let mut out = String::new();
        BufReader::new(s).read_line(&mut out).unwrap();
        assert!(out.contains("unlocked"), "{out}");
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// What the reply says about state, through the client-side parser.
fn state_of(reply: &Value) -> Option<StateInfo> {
    let body = reply.get("error").unwrap_or(reply);
    StateInfo::from_fields(body.as_object().unwrap())
}

#[test]
fn every_failure_category_reaches_the_widget_as_its_state() {
    let table = [
        (ProviderError::Unauthorized, State::Unauthorized, None),
        (ProviderError::Insufficient, State::Insufficient, None),
        (ProviderError::Offline, State::Offline, None),
        (ProviderError::Unavailable, State::Unavailable, None),
        (ProviderError::Format, State::Unavailable, None),
    ];
    for (err, want, until) in table {
        let p = Scripted::new(vec![(Err(err), None)]);
        let h = start(vec![p]);
        let r = h.snap("fake.state");
        assert_eq!(r["error"]["code"], "provider_error", "{err:?} {r}");
        assert_eq!(
            state_of(&r),
            Some(StateInfo { state: want, until }),
            "{err:?} {r}"
        );
    }
}

#[test]
fn rate_limited_carries_the_reset_time_and_keeps_serving_the_last_value_as_stale() {
    let reset = now() + 120;
    let p = Scripted::new(vec![(Ok(7), None)]);
    let h = start(vec![p.clone()]);
    let r = h.snap("fake.state");
    assert_eq!(
        (r["ok"].clone(), r["data"]["n"].clone(), r["stale"].clone()),
        (json!(true), json!(7), json!(false))
    );
    assert!(state_of(&r).is_none(), "healthy data has no state: {r}");
    // Undeclared fields never reach the widget.
    assert!(r["data"].get("secret_extra").is_none());

    p.push((Err(ProviderError::RateLimited { until: Some(reset) }), None));
    std::thread::sleep(Duration::from_millis(120)); // past the 50 ms interval
    let r = h.snap("fake.state");
    assert_eq!(r["ok"], true, "last good value must still be served: {r}");
    assert_eq!(r["data"]["n"], 7);
    assert_eq!(r["stale"], true);
    assert_eq!(
        state_of(&r),
        Some(StateInfo {
            state: State::RateLimited,
            until: Some(reset)
        })
    );
    // The reset time moves the next fetch out: the source is left alone until then.
    let n = p.fetches.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(400));
    let _ = h.snap("fake.state");
    assert_eq!(
        p.fetches.load(Ordering::SeqCst),
        n,
        "polled while rate limited"
    );
}

#[test]
fn an_implausible_reset_time_is_dropped_not_forwarded() {
    for bad in [
        Some(1),
        Some(now() - 5),
        Some(now() + 400 * 86_400),
        Some(u64::MAX),
        None,
    ] {
        let p = Scripted::new(vec![(Err(ProviderError::RateLimited { until: bad }), None)]);
        let h = start(vec![p]);
        let r = h.snap("fake.state");
        assert_eq!(
            state_of(&r),
            Some(StateInfo {
                state: State::RateLimited,
                until: None
            }),
            "{bad:?} {r}"
        );
    }
}

#[test]
fn expiring_credential_still_serves_fresh_data_with_the_expiry() {
    let at = now() + 3 * 86_400;
    let p = Scripted::new(vec![(Ok(4), Some(Notice::ExpiresSoon { at }))]);
    let h = start(vec![p]);
    let r = h.snap("fake.state");
    assert_eq!(r["ok"], true);
    assert_eq!(
        (r["data"]["n"].clone(), r["stale"].clone()),
        (json!(4), json!(false))
    );
    assert_eq!(
        state_of(&r),
        Some(StateInfo {
            state: State::ExpiredSoon,
            until: Some(at)
        })
    );
}

#[test]
fn a_success_clears_the_failure_state() {
    let p = Scripted::new(vec![(Err(ProviderError::Offline), None)]);
    let h = start(vec![p.clone()]);
    assert_eq!(
        state_of(&h.snap("fake.state")).unwrap().state,
        State::Offline
    );
    p.push((Ok(1), None));
    // Wait for the backoff (interval x2) to pass, then the next request refetches.
    let mut r = Value::Null;
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(100));
        r = h.snap("fake.state");
        if r["ok"] == true {
            break;
        }
    }
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["stale"], false);
    assert!(state_of(&r).is_none(), "{r}");
}

#[test]
fn locked_and_missing_secret_are_told_apart() {
    let h = start(vec![Arc::new(Needs)]);
    let r = h.snap("fake.needs");
    assert_eq!(r["error"]["code"], "locked");
    assert_eq!(state_of(&r).unwrap().state, State::Locked);
    h.unlock();
    // Unlocked, but the bound name is not in the vault.
    let r = h.snap("fake.needs");
    assert_eq!(r["error"]["code"], "provider_error", "{r}");
    assert_eq!(state_of(&r).unwrap().state, State::NoCredential);
}

#[test]
fn an_unconfigured_credential_topic_says_no_credential_but_only_to_granted_widgets() {
    // No provider serves github.prs (config.toml binds nothing).
    let h = start(vec![]);
    let r = h.snap("github.prs");
    assert_eq!(r["error"]["code"], "provider_error", "{r}");
    assert_eq!(state_of(&r).unwrap().state, State::NoCredential);
    assert_eq!(r["error"]["topic"], "github.prs");
    // Not granted: the same answer as for any ungranted topic, no hint that it exists.
    let r = h.ask_as("stranger", "github.prs");
    assert_eq!(r["error"]["code"], "denied");
    assert!(state_of(&r).is_none());
    // A topic nobody knows stays unknown_topic (the grant check passes first for granted ones).
    assert_eq!(h.snap("github.nope")["error"]["code"], "denied");
}

fn read_lines(s: &mut BufReader<UnixStream>, n: usize) -> Vec<Value> {
    (0..n)
        .map(|_| {
            let mut l = String::new();
            s.read_line(&mut l).unwrap();
            serde_json::from_str(&l).unwrap_or(Value::Null)
        })
        .collect()
}

#[test]
fn a_subscriber_hears_the_state_once_then_the_data_when_it_recovers() {
    let p = Scripted::new(vec![(Err(ProviderError::Unauthorized), None)]);
    let h = start(vec![p.clone()]);
    let mut raw = UnixStream::connect(h.paths.socket()).unwrap();
    raw.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    raw.write_all(
        b"{\"v\":0,\"id\":1,\"op\":\"subscribe\",\"widget\":\"w1\",\"topic\":\"fake.state\"}\n",
    )
    .unwrap();
    let mut rd = BufReader::new(raw.try_clone().unwrap());
    let reply = read_lines(&mut rd, 1).remove(0);
    assert_eq!(reply["ok"], true, "{reply}");
    // First push: no data, only why.
    let push = read_lines(&mut rd, 1).remove(0);
    assert_eq!(push["topic"], "fake.state");
    assert!(push.get("data").is_none(), "{push}");
    assert_eq!(state_of(&push).unwrap().state, State::Unauthorized);
    // Repeated identical failures are not re-announced: the next line is the recovery.
    std::thread::sleep(Duration::from_millis(700));
    p.push((Ok(9), None));
    let next = read_lines(&mut rd, 1).remove(0);
    assert_eq!(next["data"]["n"], 9, "{next}");
    assert!(state_of(&next).is_none());
    assert_eq!(next["stale"], false);
}

#[test]
fn a_late_subscriber_is_told_why_there_is_no_data() {
    let p = Scripted::new(vec![(Err(ProviderError::Insufficient), None)]);
    let h = start(vec![p]);
    assert_eq!(
        state_of(&h.snap("fake.state")).unwrap().state,
        State::Insufficient
    );
    let mut raw = UnixStream::connect(h.paths.socket()).unwrap();
    raw.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    raw.write_all(
        b"{\"v\":0,\"id\":1,\"op\":\"subscribe\",\"widget\":\"w1\",\"topic\":\"fake.state\"}\n",
    )
    .unwrap();
    let mut rd = BufReader::new(raw);
    let lines = read_lines(&mut rd, 2);
    assert_eq!(lines[0]["ok"], true);
    assert_eq!(
        state_of(&lines[1]).unwrap().state,
        State::Insufficient,
        "{}",
        lines[1]
    );
}

/// Whatever the providers do, the only text-like thing on the wire is one of the eight names.
#[test]
fn no_reply_carries_text_from_the_provider_side() {
    let p = Scripted::new(vec![(
        Err(ProviderError::RateLimited {
            until: Some(now() + 60),
        }),
        None,
    )]);
    let h = start(vec![p]);
    let r = h.snap("fake.state");
    let raw = r.to_string();
    assert!(!raw.contains("FAKE"), "{raw}");
    let err = r["error"].as_object().unwrap();
    assert!(
        err.keys()
            .all(|k| ["code", "topic", "state", "until"].contains(&k.as_str())),
        "{raw}"
    );
}

/// Holds a vault secret in `with_secret` for a few seconds, like an HTTPS request would.
struct Holder;

static HOLD: [TopicSpec; 1] = [TopicSpec::new(
    "fake.hold",
    Sensitivity::High,
    Duration::from_millis(50),
    &["n"],
)];

impl Provider for Holder {
    fn id(&self) -> &'static str {
        "holder"
    }
    fn topics(&self) -> &'static [TopicSpec] {
        &HOLD
    }
    fn secrets(&self) -> &'static [&'static str] {
        &["other"]
    }
    fn fetch(
        &self,
        _t: &str,
        _p: &Map<String, Value>,
        ctx: &FetchCtx<'_>,
    ) -> Result<Map<String, Value>, ProviderError> {
        ctx.with_secret("other", |_| std::thread::sleep(Duration::from_secs(3)))?;
        Ok(json!({"n": 1}).as_object().cloned().unwrap())
    }
}

fn ask_raw(h: &H, line: &str) -> (Value, Duration) {
    let t = std::time::Instant::now();
    let mut s = UnixStream::connect(h.paths.socket()).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(line.as_bytes()).unwrap();
    let mut out = String::new();
    BufReader::new(s).read_line(&mut out).unwrap();
    (serde_json::from_str(&out).unwrap(), t.elapsed())
}

#[test]
fn a_slow_request_holding_a_secret_does_not_stall_lock_status_or_other_topics() {
    let h = start(vec![Arc::new(Holder), Scripted::new(vec![(Ok(1), None)])]);
    h.unlock();
    let slow = {
        let path = h.paths.socket();
        std::thread::spawn(move || {
            let mut s = UnixStream::connect(path).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            s.write_all(b"{\"v\":0,\"id\":3,\"op\":\"snapshot\",\"widget\":\"w1\",\"topic\":\"fake.hold\"}\n")
                .unwrap();
            let mut out = String::new();
            BufReader::new(s).read_line(&mut out).unwrap();
            out
        })
    };
    std::thread::sleep(Duration::from_millis(500)); // the fetch is inside the 3 s closure now
    let (st, t) = ask_raw(&h, "{\"v\":0,\"id\":2,\"op\":\"vault.status\"}\n");
    assert_eq!(st["state"], "unlocked");
    assert!(t < Duration::from_secs(1), "status stalled {t:?}");
    let t0 = std::time::Instant::now();
    assert_eq!(h.snap("fake.state")["data"]["n"], 1);
    assert!(
        t0.elapsed() < Duration::from_secs(1),
        "local snapshot stalled"
    );
    let (lk, t) = ask_raw(&h, "{\"v\":0,\"id\":5,\"op\":\"vault.lock\"}\n");
    assert_eq!(lk["state"], "locked");
    assert!(t < Duration::from_secs(1), "vault.lock stalled {t:?}");
    // The in-flight result belongs to the old key: it is not served.
    let out = slow.join().unwrap();
    assert!(!out.contains("\"data\""), "{out}");
}
