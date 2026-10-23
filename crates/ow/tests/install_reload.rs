//! What `ow install|remove` tell the person after asking the daemon to reload grants, against a
//! fake daemon socket that answers with scripted lines. A rate-limited answer must never be
//! reported as applied, and "denies every widget" is said only when the daemon says the load failed.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use ow::install::reload_message_with;
use ow_core::Paths;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

const FAST: Duration = Duration::from_millis(30);
const RATE: &str = r#"{"v":0,"id":1,"error":{"code":"rate_limited"}}"#;
const OK: &str = r#"{"v":0,"id":1,"ok":true,"state":"reloaded"}"#;
const INTERNAL: &str = r#"{"v":0,"id":1,"error":{"code":"internal"}}"#;

fn serve(
    paths: &Paths,
    script: Vec<&'static str>,
) -> (std::thread::JoinHandle<Vec<String>>, Arc<AtomicUsize>) {
    ow_core::private_dir(&paths.runtime).unwrap();
    let l = UnixListener::bind(paths.socket()).unwrap();
    let n = Arc::new(AtomicUsize::new(0));
    let n2 = Arc::clone(&n);
    let h = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for reply in script {
            let (s, _): (UnixStream, _) = l.accept().unwrap();
            n2.fetch_add(1, Ordering::SeqCst);
            let mut line = String::new();
            BufReader::new(&s).read_line(&mut line).unwrap();
            seen.push(line);
            (&s).write_all(format!("{reply}\n").as_bytes()).unwrap();
        }
        seen
    });
    (h, n)
}

fn run(script: Vec<&'static str>) -> (String, usize, Vec<String>) {
    let d = tempfile::tempdir().unwrap();
    let paths = Paths::under(d.path());
    let (h, n) = serve(&paths, script);
    let msg = reload_message_with(&paths, FAST);
    let seen = h.join().unwrap();
    (msg, n.load(Ordering::SeqCst), seen)
}

#[test]
fn applied() {
    let (m, n, seen) = run(vec![OK]);
    assert_eq!(
        (m.as_str(), n),
        ("The running daemon reloaded the grants.", 1)
    );
    assert!(seen[0].contains("\"op\":\"grants.reload\""));
}

#[test]
fn no_daemon_means_next_start() {
    let d = tempfile::tempdir().unwrap();
    let m = reload_message_with(&Paths::under(d.path()), FAST);
    assert!(
        m.contains("not running") && m.contains("apply when it starts"),
        "{m}"
    );
}

#[test]
fn one_retry_after_a_rate_limit_then_success() {
    let (m, n, _) = run(vec![RATE, OK]);
    assert_eq!(
        (m.as_str(), n),
        ("The running daemon reloaded the grants.", 2)
    );
}

#[test]
fn still_rate_limited_after_the_retry_is_said_plainly_and_asked_only_twice() {
    let (m, n, _) = run(vec![RATE, RATE]);
    assert_eq!(n, 2, "exactly one retry");
    assert!(m.contains("has NOT applied it yet (rate limited)"), "{m}");
    assert!(
        m.contains("revocation takes effect at the next reload or restart"),
        "{m}"
    );
    assert!(
        !m.contains("denies every widget") && !m.contains("reloaded the grants"),
        "{m}"
    );
}

#[test]
fn only_an_internal_error_means_the_daemon_denies_everything() {
    let (m, n, _) = run(vec![INTERNAL]);
    assert_eq!(n, 1, "no retry for a load failure");
    assert!(m.contains("denies every widget"), "{m}");
}

#[test]
fn other_answers_are_not_reported_as_applied_or_as_a_denial() {
    for reply in [
        r#"{"v":0,"id":1,"error":{"code":"denied"}}"#,
        r#"{"v":0,"id":1,"error":{"code":"bad_request"}}"#,
        r#"{"v":0,"id":1,"ok":false}"#,
        "not json at all",
        "{}",
    ] {
        let (m, _, _) = run(vec![reply]);
        assert!(m.contains("did not confirm"), "{reply}: {m}");
        assert!(
            !m.contains("denies every widget") && !m.contains("reloaded the grants"),
            "{reply}: {m}"
        );
    }
}
