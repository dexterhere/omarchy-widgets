//! `ow` must not send a passphrase to a socket it cannot vouch for. A different-uid listener
//! cannot be created without a second uid (or root), so that branch (SO_PEERCRED) is not
//! tested here; the directory checks, which run first, are.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use ow_core::Paths;
use std::io::{Read, Write};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::net::UnixListener;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

const PASS: &str = "FAKE-passphrase-correct-horse";

fn unlock(dir: &std::path::Path) -> Output {
    let mut c = Command::new("setsid")
        .arg("-w")
        .arg(env!("CARGO_BIN_EXE_ow"))
        .arg("--config-dir")
        .arg(dir)
        .args(["vault", "unlock"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = c
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{PASS}\n").as_bytes());
    c.wait_with_output().unwrap()
}

/// Listener that records everything it is sent, then answers like a daemon would.
fn listen(sock: &std::path::Path) -> std::thread::JoinHandle<Vec<u8>> {
    let l = UnixListener::bind(sock).unwrap();
    l.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let mut got = Vec::new();
        for _ in 0..40 {
            if let Ok((mut s, _)) = l.accept() {
                s.set_read_timeout(Some(Duration::from_millis(300)))
                    .unwrap();
                let mut buf = [0u8; 4096];
                if let Ok(n) = s.read(&mut buf) {
                    got.extend_from_slice(&buf[..n]);
                }
                let _ = s.write_all(b"{\"v\":0,\"id\":1,\"ok\":true,\"state\":\"unlocked\"}\n");
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        got
    })
}

fn refused(o: &Output) -> bool {
    let t = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(!t.contains(PASS));
    !o.status.success() && t.contains("refusing to use the daemon socket")
}

fn setup() -> (tempfile::TempDir, Paths) {
    let d = tempfile::tempdir().unwrap();
    let p = Paths::under(d.path());
    std::fs::create_dir_all(p.runtime.parent().unwrap()).unwrap();
    (d, p)
}

#[test]
fn control_a_private_dir_gets_the_request() {
    let (d, p) = setup();
    std::fs::create_dir(&p.runtime).unwrap();
    std::fs::set_permissions(&p.runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
    let t = listen(&p.socket());
    let o = unlock(d.path());
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(!t.join().unwrap().is_empty());
}

#[test]
fn world_writable_socket_dir_is_refused_and_nothing_is_sent() {
    let (d, p) = setup();
    std::fs::create_dir(&p.runtime).unwrap();
    std::fs::set_permissions(&p.runtime, std::fs::Permissions::from_mode(0o777)).unwrap();
    let t = listen(&p.socket());
    assert!(refused(&unlock(d.path())));
    assert!(t.join().unwrap().is_empty(), "listener received bytes");
}

#[test]
fn group_accessible_socket_dir_is_refused() {
    let (d, p) = setup();
    std::fs::create_dir(&p.runtime).unwrap();
    std::fs::set_permissions(&p.runtime, std::fs::Permissions::from_mode(0o750)).unwrap();
    let t = listen(&p.socket());
    assert!(refused(&unlock(d.path())));
    assert!(t.join().unwrap().is_empty());
}

#[test]
fn symlinked_socket_dir_is_refused_and_nothing_is_sent() {
    let (d, p) = setup();
    let real = d.path().join("elsewhere");
    std::fs::create_dir(&real).unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o700)).unwrap();
    symlink(&real, &p.runtime).unwrap();
    let t = listen(&real.join("owd.sock"));
    assert!(refused(&unlock(d.path())));
    assert!(t.join().unwrap().is_empty());
}
