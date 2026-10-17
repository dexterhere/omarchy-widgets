#![allow(clippy::unwrap_used, clippy::expect_used)] // test code
use ow_core::Paths;
use owd::{NoTopics, Server, TopicSource};
use serde_json::{Map, Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

struct Harness {
    paths: Paths,
    stop: Arc<AtomicBool>,
    th: Option<JoinHandle<()>>,
    _dir: tempfile::TempDir,
}

impl Harness {
    fn start() -> Harness {
        Self::start_with(None, Box::new(NoTopics))
    }

    fn start_with(grants: Option<&str>, source: Box<dyn TopicSource>) -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        if let Some(g) = grants {
            std::fs::create_dir_all(&paths.config).unwrap();
            let p = paths.grants();
            std::fs::write(&p, g).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let server = Server::bind(&paths, source).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = Arc::clone(&stop);
        let th = std::thread::spawn(move || server.run(&s2));
        Harness {
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
    let e = Server::bind(&h.paths, Box::new(NoTopics))
        .err()
        .expect("must refuse");
    assert_eq!(e.kind(), std::io::ErrorKind::AddrInUse);

    // A stale socket file (nobody listening) is replaced.
    let dir = tempfile::tempdir().unwrap();
    let p = Paths::under(dir.path());
    drop(Server::bind(&p, Box::new(NoTopics)).unwrap()); // Drop removes the socket...
    std::os::unix::net::UnixListener::bind(p.socket())
        .map(drop)
        .unwrap(); // ...recreate a dead one
    assert!(Server::bind(&p, Box::new(NoTopics)).is_ok());
}

#[test]
fn unsafe_socket_dir_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let p = Paths::under(dir.path());
    std::fs::create_dir_all(&p.runtime).unwrap();
    std::fs::set_permissions(&p.runtime, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Server::bind(&p, Box::new(NoTopics)).is_err());
}

#[test]
fn non_socket_at_socket_path_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let p = Paths::under(dir.path());
    std::fs::create_dir_all(&p.runtime).unwrap();
    std::fs::set_permissions(&p.runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(p.socket(), b"x").unwrap();
    assert!(Server::bind(&p, Box::new(NoTopics)).is_err());
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
    let e = Server::bind(&h.paths, Box::new(NoTopics))
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

struct Fake;

impl TopicSource for Fake {
    fn snapshot(&self, topic: &str) -> Option<Map<String, Value>> {
        (topic == "github.prs").then(|| {
            json!({"open": 3, "review": 1, "token": SECRET})
                .as_object()
                .cloned()
                .unwrap()
        })
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
    let h = Harness::start_with(Some(GRANTS), Box::new(Fake));
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
    let h = Harness::start_with(Some(GRANTS), Box::new(Fake));
    assert!(snap(&h, 1, "w2", "github.prs").contains("denied"));
    assert!(snap(&h, 2, "w1", "other.topic").contains("denied"));
    let a = audit(&h);
    assert_eq!(a.matches("\"decision\":\"deny\"").count(), 2, "{a}");
}

#[test]
fn granted_but_unknown_topic_reports_unknown_topic() {
    let g = "[[grant]]\nwidget=\"w1\"\ntopics=[\"nope\"]\n";
    let h = Harness::start_with(Some(g), Box::new(Fake));
    assert!(snap(&h, 1, "w1", "nope").contains("unknown_topic"));
}

#[test]
fn rate_limit_kicks_in() {
    let h = Harness::start_with(Some(GRANTS), Box::new(Fake));
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
    let h = Harness::start_with(Some(GRANTS), Box::new(Fake));
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
        assert!(Server::bind(&paths, Box::new(NoTopics)).is_err());
    }
}
