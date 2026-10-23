//! `ow vault unlock|lock|status` against a real in-process daemon. Fake values only.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use ow_core::{FetchCtx, Paths, Provider, ProviderError, Sensitivity, TopicSpec};
use ow_vault::{Profile, Vault};
use owd::Server;
use serde_json::{Map, Value, json};
use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// Tests here spawn children while another test may hold a vault lock file open; a fork in that
/// window briefly shares the lock and makes `Vault` report Busy. Run them one at a time.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

const PASS: &str = "FAKE-passphrase-correct-horse";
const WRONG: &str = "FAKE-passphrase-wrong-horse!";
const TOKEN: &str = "FAKE-TOKEN-VALUE-77c1e";

fn ow(dir: &std::path::Path, args: &[&str], stdin: &str) -> Output {
    let mut c = Command::new("setsid")
        .arg("-w")
        .arg(env!("CARGO_BIN_EXE_ow"))
        .arg("--config-dir")
        .arg(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = c.stdin.take().unwrap().write_all(stdin.as_bytes());
    c.wait_with_output().unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn unlock_status_lock_roundtrip_never_prints_secrets() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let d = tempfile::tempdir().unwrap();
    let paths = Paths::under(d.path());
    // Not running yet: status still reports the file, and lock fails clearly.
    ow_core::private_dir(&paths.data).unwrap();
    let mut v = Vault::create(&paths.vault(), PASS.as_bytes(), Profile::Standard).unwrap();
    v.set("github-main", TOKEN).unwrap();
    drop(v);
    let o = ow(d.path(), &["vault", "status"], "");
    assert!(
        o.status.success() && text(&o).contains("daemon: not running"),
        "{}",
        text(&o)
    );
    let o = ow(d.path(), &["vault", "lock"], "");
    assert!(!o.status.success() && text(&o).contains("not reachable"));

    let server = Server::bind(&paths, Vec::new()).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (s2, r2) = (Arc::clone(&stop), Arc::new(AtomicBool::new(false)));
    let th = std::thread::spawn(move || server.run(&s2, &r2));

    let all = [
        ow(d.path(), &["vault", "status"], ""),
        ow(d.path(), &["vault", "unlock"], &format!("{WRONG}\n")),
        ow(d.path(), &["vault", "unlock"], &format!("{PASS}\n")),
        ow(d.path(), &["vault", "status"], ""),
        ow(d.path(), &["vault", "lock"], ""),
        ow(d.path(), &["vault", "status"], ""),
    ];
    assert!(
        text(&all[0]).contains("daemon: locked"),
        "{}",
        text(&all[0])
    );
    assert!(!all[1].status.success());
    assert!(
        text(&all[1]).contains("wrong passphrase, or the vault cannot be opened"),
        "{}",
        text(&all[1])
    );
    assert!(all[2].status.success() && text(&all[2]).contains("daemon unlocked"));
    let st = text(&all[3]);
    assert!(
        st.contains("daemon: unlocked, 1 secret(s) [github-main]"),
        "{st}"
    );
    assert!(all[4].status.success() && text(&all[5]).contains("daemon: locked"));
    let combined: String = all.iter().map(text).collect();
    for needle in [PASS, WRONG, TOKEN] {
        assert!(!combined.contains(needle), "{needle} leaked");
    }

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    th.join().unwrap();
}

struct Secretive;

static TOPICS: [TopicSpec; 1] = [TopicSpec::new(
    "fake.secret",
    Sensitivity::High,
    std::time::Duration::from_millis(50),
    &["len"],
)];

impl Provider for Secretive {
    fn id(&self) -> &'static str {
        "fake"
    }
    fn topics(&self) -> &'static [TopicSpec] {
        &TOPICS
    }
    fn secrets(&self) -> &'static [&'static str] {
        &["github-main"]
    }
    fn fetch(
        &self,
        _t: &str,
        _p: &Map<String, Value>,
        ctx: &FetchCtx<'_>,
    ) -> Result<Map<String, Value>, ProviderError> {
        let len = ctx.secret("github-main")?.expose().len();
        Ok(json!({"len": len}).as_object().cloned().unwrap())
    }
}

fn snapshot(paths: &Paths) -> String {
    use std::io::{BufRead, BufReader};
    let mut s = std::os::unix::net::UnixStream::connect(paths.socket()).unwrap();
    s.write_all(
        b"{\"v\":0,\"id\":1,\"op\":\"snapshot\",\"widget\":\"w1\",\"topic\":\"fake.secret\"}\n",
    )
    .unwrap();
    let mut l = String::new();
    BufReader::new(s).read_line(&mut l).unwrap();
    l
}

/// Changing the vault file under an unlocked daemon must not leave a decrypted copy serving.
#[test]
fn file_changing_commands_lock_the_daemon() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let paths = Paths::under(d.path());
    std::fs::create_dir_all(&paths.config).unwrap();
    std::fs::write(
        paths.grants(),
        "[[grant]]\nwidget=\"w1\"\ntopics=[\"fake.secret\"]\nfields=[\"len\"]\n",
    )
    .unwrap();
    std::fs::set_permissions(paths.grants(), std::fs::Permissions::from_mode(0o600)).unwrap();
    ow_core::private_dir(&paths.data).unwrap();
    let mut v = Vault::create(&paths.vault(), PASS.as_bytes(), Profile::Standard).unwrap();
    v.set("github-main", TOKEN).unwrap();
    v.set("other", "FAKE-other-value").unwrap();
    drop(v);
    let server = Server::bind(&paths, vec![Arc::new(Secretive)]).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (s2, r2) = (Arc::clone(&stop), Arc::new(AtomicBool::new(false)));
    let th = std::thread::spawn(move || server.run(&s2, &r2));

    let unlocked = |expect: bool| {
        let st = text(&ow(d.path(), &["vault", "status"], ""));
        assert_eq!(st.contains("daemon: unlocked"), expect, "{st}");
    };
    let unlock = || {
        assert!(
            ow(d.path(), &["vault", "unlock"], &format!("{PASS}\n"))
                .status
                .success()
        );
        assert!(snapshot(&paths).contains("\"len\""));
        unlocked(true);
    };
    let locked_now = || {
        unlocked(false);
        assert!(snapshot(&paths).contains("\"locked\""));
    };

    unlock();
    let o = ow(
        d.path(),
        &["vault", "remove", "other"],
        &format!("{PASS}\n"),
    );
    assert!(o.status.success(), "{}", text(&o));
    locked_now();

    unlock();
    let o = ow(
        d.path(),
        &["vault", "set", "github-main", "--passphrase-stdin"],
        &format!("{PASS}\nFAKE-new-token-value\n"),
    );
    assert!(o.status.success(), "{}", text(&o));
    locked_now();

    unlock();
    let o = ow(
        d.path(),
        &["vault", "rotate"],
        &format!("{PASS}\nFAKE-new-passphrase-long\n"),
    );
    assert!(o.status.success(), "{}", text(&o));
    locked_now();

    assert!(
        ow(d.path(), &["vault", "unlock"], "FAKE-new-passphrase-long\n")
            .status
            .success()
    );
    assert!(snapshot(&paths).contains("\"len\""));
    let o = ow(d.path(), &["vault", "wipe", "--yes"], "");
    assert!(o.status.success(), "{}", text(&o));
    locked_now();

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    th.join().unwrap();
}
