#![allow(clippy::unwrap_used, clippy::expect_used)] // test code
use ow_core::Paths;
use ow_core::{FetchCtx, Provider, ProviderError, Sensitivity, TopicSpec};
use owd::Server;
use serde_json::{Map, Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

struct Harness {
    reload: Arc<AtomicBool>,
    paths: Paths,
    stop: Arc<AtomicBool>,
    th: Option<JoinHandle<()>>,
    _dir: tempfile::TempDir,
}

impl Harness {
    fn start() -> Harness {
        Self::start_with(None, Vec::new())
    }

    fn start_with(grants: Option<&str>, providers: Vec<Arc<dyn Provider>>) -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        if let Some(g) = grants {
            std::fs::create_dir_all(&paths.config).unwrap();
            let p = paths.grants();
            std::fs::write(&p, g).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let server = Server::bind(&paths, providers).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = Arc::clone(&stop);
        let reload = Arc::new(AtomicBool::new(false));
        let r2 = Arc::clone(&reload);
        let th = std::thread::spawn(move || server.run(&s2, &r2));
        Harness {
            reload,
            paths,
            stop,
            th: Some(th),
            _dir: dir,
        }
    }

    fn connect(&self) -> UnixStream {
        let s = UnixStream::connect(self.paths.socket()).unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        s
    }

    fn ask(&self, line: &str) -> String {
        let mut s = self.connect();
        s.write_all(line.as_bytes()).unwrap();
        let mut out = String::new();
        BufReader::new(s).read_line(&mut out).unwrap();
        out
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.th.take() {
            let _ = t.join();
        }
    }
}

#[test]
fn modes_are_private() {
    let h = Harness::start();
    let dir = std::fs::metadata(&h.paths.runtime)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    let sock = std::fs::metadata(h.paths.socket())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(dir, 0o700);
    assert_eq!(sock, 0o600);
}

#[test]
fn hello_and_status() {
    let h = Harness::start();
    let r = h.ask("{\"v\":0,\"id\":1,\"op\":\"hello\",\"client\":\"t\",\"clientVersion\":\"0\"}\n");
    assert!(
        r.starts_with("{\"v\":0,\"id\":1,\"ok\":true,\"daemonVersion\":"),
        "{r}"
    );
    let r = h.ask("{\"v\":0,\"id\":2,\"op\":\"status\"}\n");
    assert!(r.contains("\"id\":2") && r.contains("\"ok\":true"), "{r}");
}

#[test]
fn data_ops_are_denied_by_default() {
    let h = Harness::start();
    let r =
        h.ask("{\"v\":0,\"id\":3,\"op\":\"snapshot\",\"widget\":\"w\",\"topic\":\"github.prs\"}\n");
    assert_eq!(
        r,
        "{\"v\":0,\"id\":3,\"error\":{\"code\":\"denied\",\"topic\":\"github.prs\"}}\n"
    );
}

#[test]
fn malformed_input_gets_error_and_connection_survives() {
    let h = Harness::start();
    let mut s = h.connect();
    s.write_all(b"garbage\n{\"v\":0,\"id\":4,\"op\":\"status\"}\n")
        .unwrap();
    let mut r = BufReader::new(s);
    let (mut a, mut b) = (String::new(), String::new());
    r.read_line(&mut a).unwrap();
    r.read_line(&mut b).unwrap();
    assert!(a.contains("bad_request"), "{a}");
    assert!(b.contains("\"id\":4") && b.contains("\"ok\":true"), "{b}");
}

#[test]
fn oversize_line_closes_connection() {
    let h = Harness::start();
    let mut s = h.connect();
    s.write_all(&vec![b'x'; ow_proto::MAX_LINE + 1]).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap(); // server replies then closes
    assert!(out.contains("bad_request"), "{out}");
}

#[test]
fn second_instance_refused_and_stale_socket_replaced() {
    let h = Harness::start();
    let e = Server::bind(&h.paths, Vec::new())
        .err()
        .expect("must refuse");
    assert_eq!(e.kind(), std::io::ErrorKind::AddrInUse);

    // A stale socket file (nobody listening) is replaced.
    let dir = tempfile::tempdir().unwrap();
    let p = Paths::under(dir.path());
    drop(Server::bind(&p, Vec::new()).unwrap()); // Drop removes the socket...
    std::os::unix::net::UnixListener::bind(p.socket())
        .map(drop)
        .unwrap(); // ...recreate a dead one
    assert!(Server::bind(&p, Vec::new()).is_ok());
}

#[test]
fn unsafe_socket_dir_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let p = Paths::under(dir.path());
    std::fs::create_dir_all(&p.runtime).unwrap();
    std::fs::set_permissions(&p.runtime, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Server::bind(&p, Vec::new()).is_err());
}

#[test]
fn non_socket_at_socket_path_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let p = Paths::under(dir.path());
    std::fs::create_dir_all(&p.runtime).unwrap();
    std::fs::set_permissions(&p.runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(p.socket(), b"x").unwrap();
    assert!(Server::bind(&p, Vec::new()).is_err());
    assert!(p.socket().exists(), "must not delete a foreign file");
}

#[test]
fn repeated_garbage_gets_disconnected() {
    let h = Harness::start();
    let mut s = h.connect();
    s.write_all(&b"x\n".repeat(20)).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap(); // must reach EOF, not hang
    assert_eq!(out.matches("bad_request").count(), 8, "{out}");
}

#[test]
fn concurrent_start_is_refused_by_lock() {
    let h = Harness::start();
    // Even with the socket file removed, the held lock blocks a second daemon.
    std::fs::remove_file(h.paths.socket()).unwrap();
    let e = Server::bind(&h.paths, Vec::new())
        .err()
        .expect("must refuse");
    assert_eq!(e.kind(), std::io::ErrorKind::AddrInUse);
}

#[test]
fn full_session_leaves_only_expected_files() {
    let h = Harness::start();
    let _ = h.ask("{\"v\":0,\"id\":1,\"op\":\"status\"}\n");
    let mut names: Vec<_> = std::fs::read_dir(&h.paths.runtime)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["owd.lock", "owd.sock"]);
}

const SECRET: &str = "FAKE-SECRET-VALUE-9f3a";

#[derive(Default)]
struct Fake {
    fetches: AtomicU64,
    fail: AtomicBool,
    panic: AtomicBool,
}

static FAKE_TOPICS: [TopicSpec; 1] = [TopicSpec::new(
    "github.prs",
    Sensitivity::High,
    Duration::from_millis(50),
    &["open", "review", "token", "seq"],
)];

impl Provider for Fake {
    fn id(&self) -> &'static str {
        "fake"
    }

    fn topics(&self) -> &'static [TopicSpec] {
        &FAKE_TOPICS
    }

    fn fetch(
        &self,
        _topic: &str,
        _p: &Map<String, Value>,
        _c: &FetchCtx<'_>,
    ) -> Result<Map<String, Value>, ProviderError> {
        let n = self.fetches.fetch_add(1, Ordering::Relaxed);
        assert!(!self.panic.load(Ordering::Relaxed), "provider bug");
        if self.fail.load(Ordering::Relaxed) {
            return Err(ProviderError::Unavailable);
        }
        Ok(json!({"open": 3, "review": 1, "token": SECRET, "seq": n})
            .as_object()
            .cloned()
            .unwrap())
    }
}

const GRANTS: &str =
    "[[grant]]\nwidget=\"w1\"\ntopics=[\"github.prs\"]\nfields=[\"open\",\"review\"]\n";

fn snap(h: &Harness, id: u32, widget: &str, topic: &str) -> String {
    h.ask(&format!("{{\"v\":0,\"id\":{id},\"op\":\"snapshot\",\"widget\":\"{widget}\",\"topic\":\"{topic}\"}}\n"))
}

fn audit(h: &Harness) -> String {
    std::fs::read_to_string(h.paths.audit()).unwrap_or_default()
}

#[test]
fn granted_snapshot_is_field_filtered() {
    let h = Harness::start_with(Some(GRANTS), vec![Arc::new(Fake::default())]);
    let r = snap(&h, 1, "w1", "github.prs");
    assert!(
        r.contains("\"open\":3") && r.contains("\"review\":1"),
        "{r}"
    );
    assert!(
        !r.contains("token") && !r.contains(SECRET),
        "ungranted field leaked: {r}"
    );
}

#[test]
fn ungranted_widget_or_topic_is_denied_and_audited() {
    let h = Harness::start_with(Some(GRANTS), vec![Arc::new(Fake::default())]);
    assert!(snap(&h, 1, "w2", "github.prs").contains("denied"));
    assert!(snap(&h, 2, "w1", "other.topic").contains("denied"));
    let a = audit(&h);
    assert_eq!(a.matches("\"decision\":\"deny\"").count(), 2, "{a}");
}

#[test]
fn granted_but_unknown_topic_reports_unknown_topic() {
    let g = "[[grant]]\nwidget=\"w1\"\ntopics=[\"nope\"]\n";
    let h = Harness::start_with(Some(g), vec![Arc::new(Fake::default())]);
    assert!(snap(&h, 1, "w1", "nope").contains("unknown_topic"));
}

#[test]
fn unknown_topic_replies_do_not_count_as_bad_frames() {
    let g = "[[grant]]\nwidget=\"w1\"\ntopics=[\"nope\"]\n";
    let h = Harness::start_with(Some(g), vec![Arc::new(Fake::default())]);
    let mut s = h.connect();
    let line = "{\"v\":0,\"id\":1,\"op\":\"snapshot\",\"widget\":\"w1\",\"topic\":\"nope\"}\n";
    s.write_all(line.repeat(9).as_bytes()).unwrap();
    let mut r = BufReader::new(s);
    for _ in 0..9 {
        let mut l = String::new();
        assert!(r.read_line(&mut l).unwrap() > 0, "connection closed early");
        assert!(
            l.contains("unknown_topic") || l.contains("rate_limited"),
            "{l}"
        );
    }
}

#[test]
fn rate_limit_kicks_in() {
    let h = Harness::start_with(Some(GRANTS), vec![Arc::new(Fake::default())]);
    let mut limited = 0;
    for i in 0..30 {
        if snap(&h, i, "w1", "github.prs").contains("rate_limited") {
            limited += 1;
        }
    }
    assert!(limited >= 15, "only {limited} limited");
}

#[test]
fn audit_log_never_contains_payload_values() {
    let h = Harness::start_with(Some(GRANTS), vec![Arc::new(Fake::default())]);
    let _ = snap(&h, 1, "w1", "github.prs");
    let _ = snap(&h, 2, "w2", "github.prs");
    let a = audit(&h);
    assert!(
        a.contains("\"op\":\"snapshot\"") && a.contains("\"bytes\":"),
        "{a}"
    );
    assert!(
        !a.contains(SECRET) && !a.contains("\"open\":3"),
        "payload in audit: {a}"
    );
    let mode = std::fs::metadata(h.paths.audit())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn insecure_or_broken_grants_file_stops_startup() {
    for (body, mode) in [(GRANTS, 0o644), ("this is = not [valid", 0o600)] {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        std::fs::create_dir_all(&paths.config).unwrap();
        std::fs::write(paths.grants(), body).unwrap();
        std::fs::set_permissions(paths.grants(), std::fs::Permissions::from_mode(mode)).unwrap();
        assert!(Server::bind(&paths, Vec::new()).is_err());
    }
}

// ---- subscriptions and the scheduler ----

const SUB_GRANTS: &str =
    "[[grant]]\nwidget=\"w1\"\ntopics=[\"github.prs\"]\nfields=[\"open\",\"seq\"]\n";

fn sub_line(id: u32, widget: &str) -> String {
    format!(
        "{{\"v\":0,\"id\":{id},\"op\":\"subscribe\",\"widget\":\"{widget}\",\"topic\":\"github.prs\"}}\n"
    )
}

fn read_json(r: &mut BufReader<UnixStream>) -> Value {
    let mut l = String::new();
    assert!(r.read_line(&mut l).unwrap() > 0, "connection closed");
    serde_json::from_str(&l).unwrap()
}

/// Reader that gives up quickly, for asserting that nothing more arrives.
fn quiet(r: &mut BufReader<UnixStream>) -> bool {
    r.get_ref()
        .set_read_timeout(Some(Duration::from_millis(400)))
        .unwrap();
    let mut l = String::new();
    let got = r.read_line(&mut l);
    r.get_ref()
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    got.is_err()
}

fn harness_with(fake: &Arc<Fake>, grants: &str) -> Harness {
    Harness::start_with(Some(grants), vec![Arc::clone(fake) as Arc<dyn Provider>])
}

#[test]
fn subscribe_pushes_initial_value_then_updates_filtered() {
    let fake = Arc::new(Fake::default());
    let h = harness_with(&fake, SUB_GRANTS);
    let mut s = h.connect();
    s.write_all(sub_line(1, "w1").as_bytes()).unwrap();
    let mut r = BufReader::new(s);
    let ack = read_json(&mut r);
    assert_eq!(ack["ok"], true);
    let sub = ack["sub"].as_u64().unwrap();
    let first = read_json(&mut r);
    assert_eq!(first["sub"], sub);
    assert_eq!(first["stale"], false);
    assert!(
        first["data"].get("token").is_none(),
        "ungranted field pushed: {first}"
    );
    assert_eq!(first["data"]["open"], 3);
    let second = read_json(&mut r);
    assert!(
        second["data"]["seq"].as_u64() > first["data"]["seq"].as_u64(),
        "no update arrived"
    );
}

#[test]
fn no_polling_without_subscribers_and_after_disconnect() {
    let fake = Arc::new(Fake::default());
    let h = harness_with(&fake, SUB_GRANTS);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        fake.fetches.load(Ordering::Relaxed),
        0,
        "polled with nobody subscribed"
    );
    let mut s = h.connect();
    s.write_all(sub_line(1, "w1").as_bytes()).unwrap();
    let mut r = BufReader::new(s);
    read_json(&mut r);
    read_json(&mut r);
    drop(r); // disconnect
    std::thread::sleep(Duration::from_millis(400));
    let after = fake.fetches.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        fake.fetches.load(Ordering::Relaxed),
        after,
        "still polling after disconnect"
    );
}

#[test]
fn unsubscribe_stops_pushes_and_cannot_touch_others_subs() {
    let fake = Arc::new(Fake::default());
    let h = harness_with(&fake, SUB_GRANTS);
    let mut a = h.connect();
    a.write_all(sub_line(1, "w1").as_bytes()).unwrap();
    let mut ra = BufReader::new(a.try_clone().unwrap());
    let sub = read_json(&mut ra)["sub"].as_u64().unwrap();
    read_json(&mut ra);
    // Another connection cannot unsubscribe it.
    let r = h.ask(&format!(
        "{{\"v\":0,\"id\":9,\"op\":\"unsubscribe\",\"sub\":{sub}}}\n"
    ));
    assert!(r.contains("bad_request"), "{r}");
    a.write_all(format!("{{\"v\":0,\"id\":2,\"op\":\"unsubscribe\",\"sub\":{sub}}}\n").as_bytes())
        .unwrap();
    // Drain whatever was in flight, then require silence.
    loop {
        let m = read_json(&mut ra);
        if m["id"] == 2 {
            assert_eq!(m["ok"], true);
            break;
        }
    }
    assert!(quiet(&mut ra), "push after unsubscribe");
}

#[test]
fn revoked_grant_stops_pushes_and_is_audited() {
    let fake = Arc::new(Fake::default());
    let h = harness_with(&fake, SUB_GRANTS);
    let mut s = h.connect();
    s.write_all(sub_line(1, "w1").as_bytes()).unwrap();
    let mut r = BufReader::new(s);
    read_json(&mut r);
    read_json(&mut r);
    std::fs::write(h.paths.grants(), "").unwrap();
    h.reload.store(true, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(300));
    // Drain pushes already queued before the reload, then require silence.
    r.get_ref()
        .set_read_timeout(Some(Duration::from_millis(400)))
        .unwrap();
    let mut l = String::new();
    while r.read_line(&mut l).is_ok_and(|n| n > 0) {
        l.clear();
    }
    assert!(quiet(&mut r), "data still flowing after revocation");
    assert!(
        audit(&h).contains("\"op\":\"push\",\"decision\":\"deny\""),
        "{}",
        audit(&h)
    );
}

#[test]
fn provider_failure_yields_stale_push_with_last_good_data_and_recovers() {
    let fake = Arc::new(Fake::default());
    let h = harness_with(&fake, SUB_GRANTS);
    let mut s = h.connect();
    s.write_all(sub_line(1, "w1").as_bytes()).unwrap();
    let mut r = BufReader::new(s);
    read_json(&mut r);
    let good = read_json(&mut r);
    fake.fail.store(true, Ordering::Relaxed);
    let stale = loop {
        let m = read_json(&mut r);
        if m["stale"] == true {
            break m;
        }
    };
    assert_eq!(
        stale["data"]["open"], 3,
        "stale push must carry the last good data"
    );
    assert!(stale["ts"].as_u64() >= good["ts"].as_u64());
    fake.fail.store(false, Ordering::Relaxed);
    loop {
        if read_json(&mut r)["stale"] == false {
            break;
        }
    }
}

#[test]
fn panicking_provider_does_not_kill_the_scheduler() {
    let fake = Arc::new(Fake::default());
    let h = harness_with(&fake, SUB_GRANTS);
    let mut s = h.connect();
    s.write_all(sub_line(1, "w1").as_bytes()).unwrap();
    let mut r = BufReader::new(s);
    read_json(&mut r);
    read_json(&mut r);
    fake.panic.store(true, Ordering::Relaxed);
    loop {
        if read_json(&mut r)["stale"] == true {
            break;
        }
    }
    fake.panic.store(false, Ordering::Relaxed);
    loop {
        if read_json(&mut r)["stale"] == false {
            break;
        }
    }
}

#[test]
fn subscribe_requires_a_grant_and_audits_allow() {
    let fake = Arc::new(Fake::default());
    let h = harness_with(&fake, SUB_GRANTS);
    assert!(h.ask(&sub_line(1, "stranger")).contains("denied"));
    assert_eq!(
        fake.fetches.load(Ordering::Relaxed),
        0,
        "ungranted subscribe must not trigger a fetch"
    );
    let mut s = h.connect();
    s.write_all(sub_line(2, "w1").as_bytes()).unwrap();
    let mut r = BufReader::new(s);
    read_json(&mut r);
    let a = audit(&h);
    assert!(
        a.contains("\"op\":\"subscribe\",\"decision\":\"allow\"") && a.contains("deny"),
        "{a}"
    );
    assert!(!a.contains(SECRET));
}

#[test]
fn subscriptions_per_connection_are_capped() {
    let fake = Arc::new(Fake::default());
    let mut g = String::new();
    for i in 0..40 {
        g += &format!("[[grant]]\nwidget=\"w{i}\"\ntopics=[\"github.prs\"]\n");
    }
    let h = harness_with(&fake, &g);
    let mut s = h.connect();
    for i in 0..40 {
        s.write_all(sub_line(i + 1, &format!("w{i}")).as_bytes())
            .unwrap();
    }
    let mut r = BufReader::new(s);
    let mut capped = false;
    for _ in 0..200 {
        let m = read_json(&mut r);
        if m["error"]["code"] == "rate_limited" {
            capped = true;
            break;
        }
    }
    assert!(capped, "no cap on subscriptions per connection");
}

#[test]
fn undeclared_params_are_rejected_and_cannot_mint_cache_keys() {
    let fake = Arc::new(Fake::default());
    let h = harness_with(&fake, SUB_GRANTS);
    let mut s = h.connect();
    let mut r = BufReader::new(s.try_clone().unwrap());
    let big = format!(r#"{{"big":"{}"}}"#, "z".repeat(400));
    for (i, params) in [r#"{"x":1}"#, r#"{"a":[1]}"#, big.as_str()]
        .iter()
        .enumerate()
    {
        let l = format!(
            "{{\"v\":0,\"id\":{},\"op\":\"subscribe\",\"widget\":\"w1\",\"topic\":\"github.prs\",\"params\":{params}}}\n",
            i + 1
        );
        s.write_all(l.as_bytes()).unwrap();
        let m = read_json(&mut r);
        assert_eq!(m["error"]["code"], "bad_request", "{m}");
    }
    assert_eq!(
        fake.fetches.load(Ordering::Relaxed),
        0,
        "rejected params must not reach the provider"
    );
}

// ---- review round: slow providers, ordering, revocation during fetch ----

struct Slow(Duration);

static SLOW_TOPICS: [TopicSpec; 1] = [TopicSpec::new(
    "slow.t",
    Sensitivity::Low,
    Duration::from_millis(50),
    &["v"],
)];

impl Provider for Slow {
    fn id(&self) -> &'static str {
        "slow"
    }

    fn topics(&self) -> &'static [TopicSpec] {
        &SLOW_TOPICS
    }

    fn fetch(
        &self,
        _t: &str,
        _p: &Map<String, Value>,
        _c: &FetchCtx<'_>,
    ) -> Result<Map<String, Value>, ProviderError> {
        std::thread::sleep(self.0);
        Ok(json!({"v": 1}).as_object().cloned().unwrap())
    }
}

const BOTH: &str = "[[grant]]\nwidget=\"w1\"\ntopics=[\"github.prs\",\"slow.t\"]\nfields=[\"open\",\"seq\",\"v\"]\n";

#[test]
fn a_blocked_provider_does_not_starve_other_topics() {
    let fake = Arc::new(Fake::default());
    let h = Harness::start_with(
        Some(BOTH),
        vec![
            Arc::new(Slow(Duration::from_secs(3))),
            Arc::clone(&fake) as Arc<dyn Provider>,
        ],
    );
    let mut s = h.connect();
    let slow = "{\"v\":0,\"id\":1,\"op\":\"subscribe\",\"widget\":\"w1\",\"topic\":\"slow.t\"}\n";
    s.write_all(slow.as_bytes()).unwrap();
    s.write_all(sub_line(2, "w1").as_bytes()).unwrap();
    let mut r = BufReader::new(s);
    let start = std::time::Instant::now();
    let mut github_pushes = 0;
    while github_pushes < 3 {
        let m = read_json(&mut r);
        if m["topic"] == "github.prs" {
            github_pushes += 1;
        }
    }
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "github.prs waited on the slow provider"
    );
}

#[test]
fn shutdown_is_not_blocked_by_a_stuck_provider() {
    let fake = Arc::new(Fake::default());
    let h = Harness::start_with(
        Some(BOTH),
        vec![Arc::new(Slow(Duration::from_secs(30))), fake],
    );
    let mut s = h.connect();
    let slow = "{\"v\":0,\"id\":1,\"op\":\"subscribe\",\"widget\":\"w1\",\"topic\":\"slow.t\"}\n";
    s.write_all(slow.as_bytes()).unwrap();
    let mut r = BufReader::new(s);
    read_json(&mut r);
    std::thread::sleep(Duration::from_millis(200)); // fetch is now stuck
    let t = std::time::Instant::now();
    drop(h); // sets stop and joins the server thread
    assert!(
        t.elapsed() < Duration::from_secs(3),
        "shutdown waited on the provider"
    );
}

#[test]
fn subscribe_reply_always_precedes_pushes() {
    let fake = Arc::new(Fake::default());
    let h = harness_with(&fake, SUB_GRANTS);
    for i in 0..8 {
        // stays within the rate-limit burst of 10
        let mut s = h.connect();
        s.write_all(sub_line(i + 1, "w1").as_bytes()).unwrap();
        let mut r = BufReader::new(s);
        let first = read_json(&mut r);
        assert!(
            first.get("id").is_some() && first.get("sub").is_some() && first.get("topic").is_none(),
            "push before ack: {first}"
        );
    }
}

#[test]
fn revocation_during_a_slow_snapshot_is_honoured() {
    let h = Harness::start_with(Some(BOTH), vec![Arc::new(Slow(Duration::from_millis(600)))]);
    let mut s = h.connect();
    let snap = "{\"v\":0,\"id\":1,\"op\":\"snapshot\",\"widget\":\"w1\",\"topic\":\"slow.t\"}\n";
    s.write_all(snap.as_bytes()).unwrap();
    std::thread::sleep(Duration::from_millis(150)); // request is inside the provider now
    std::fs::write(h.paths.grants(), "").unwrap();
    h.reload.store(true, Ordering::Relaxed);
    let mut r = BufReader::new(s);
    let m = read_json(&mut r);
    assert_eq!(
        m["error"]["code"], "denied",
        "data released after revocation: {m}"
    );
}

#[test]
fn oversized_provider_output_is_rejected() {
    struct Big;
    static BIG: [TopicSpec; 1] = [TopicSpec::new(
        "big.t",
        Sensitivity::Low,
        Duration::from_millis(50),
        &["blob"],
    )];
    impl Provider for Big {
        fn id(&self) -> &'static str {
            "big"
        }
        fn topics(&self) -> &'static [TopicSpec] {
            &BIG
        }
        fn fetch(
            &self,
            _t: &str,
            _p: &Map<String, Value>,
            _c: &FetchCtx<'_>,
        ) -> Result<Map<String, Value>, ProviderError> {
            Ok(json!({"blob": "x".repeat(200_000)})
                .as_object()
                .cloned()
                .unwrap())
        }
    }
    let g = "[[grant]]\nwidget=\"w1\"\ntopics=[\"big.t\"]\n";
    let h = Harness::start_with(Some(g), vec![Arc::new(Big)]);
    let r = h.ask("{\"v\":0,\"id\":1,\"op\":\"snapshot\",\"widget\":\"w1\",\"topic\":\"big.t\"}\n");
    assert!(
        r.contains("provider_error") && r.len() < 500,
        "{}",
        &r[..r.len().min(200)]
    );
}

#[test]
fn system_time_reaches_granted_widget_only() {
    let g = "[[grant]]\nwidget=\"clock\"\ntopics=[\"system.time\"]\nfields=[\"hm\",\"hms\",\"date\",\"tz\"]\n\n[[grant]]\nwidget=\"partial\"\ntopics=[\"system.time\"]\nfields=[\"hm\"]\n";
    let real: Vec<Arc<dyn Provider>> = vec![
        Arc::new(ow_providers::System::new()),
        Arc::new(ow_providers::Battery::new()),
    ];
    let h = Harness::start_with(Some(g), real);
    // granted: all four fields, with the widget's shapes
    let v: Value = serde_json::from_str(&snap(&h, 1, "clock", "system.time")).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["data"].as_object().unwrap().len(), 4, "{v}");
    assert_eq!(v["data"]["hm"].as_str().unwrap().len(), 5);
    assert_eq!(v["data"]["hms"].as_str().unwrap().len(), 8);
    assert!(v["data"]["date"].is_string() && v["data"]["tz"].is_string());
    // field-level grant: only hm
    let v: Value = serde_json::from_str(&snap(&h, 2, "partial", "system.time")).unwrap();
    assert_eq!(v["data"].as_object().unwrap().len(), 1, "{v}");
    // denied client and ungranted topic get nothing, not even field names
    for (w, t) in [("stranger", "system.time"), ("clock", "battery.status")] {
        let r = snap(&h, 3, w, t);
        assert!(r.contains("denied"), "{r}");
        assert!(!r.contains("\"hm\"") && !r.contains("percent"), "{r}");
    }
}
