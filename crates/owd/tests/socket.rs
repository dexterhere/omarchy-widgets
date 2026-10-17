#![allow(clippy::unwrap_used, clippy::expect_used)] // test code
use ow_core::Paths;
use owd::Server;
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
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        let server = Server::bind(&paths).unwrap();
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
    let e = Server::bind(&h.paths).err().expect("must refuse");
    assert_eq!(e.kind(), std::io::ErrorKind::AddrInUse);

    // A stale socket file (nobody listening) is replaced.
    let dir = tempfile::tempdir().unwrap();
    let p = Paths::under(dir.path());
    drop(Server::bind(&p).unwrap()); // Drop removes the socket...
    std::os::unix::net::UnixListener::bind(p.socket())
        .map(drop)
        .unwrap(); // ...recreate a dead one
    assert!(Server::bind(&p).is_ok());
}

#[test]
fn unsafe_socket_dir_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let p = Paths::under(dir.path());
    std::fs::create_dir_all(&p.runtime).unwrap();
    std::fs::set_permissions(&p.runtime, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Server::bind(&p).is_err());
}

#[test]
fn non_socket_at_socket_path_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let p = Paths::under(dir.path());
    std::fs::create_dir_all(&p.runtime).unwrap();
    std::fs::set_permissions(&p.runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(p.socket(), b"x").unwrap();
    assert!(Server::bind(&p).is_err());
    assert!(p.socket().exists(), "must not delete a foreign file");
}
