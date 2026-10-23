//! `ow vault unlock|lock|status` against a real in-process daemon. Fake values only.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use ow_core::Paths;
use ow_vault::{Profile, Vault};
use owd::Server;
use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

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
