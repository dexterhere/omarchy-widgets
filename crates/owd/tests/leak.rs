//! Plan item 4 (docs/AUTH_PLAN.md): run the real `owd` binary against a throwaway tree, unlock
//! it with a FAKE passphrase over the socket, drive a session, then search everything the
//! process could have left behind for the fake passphrase and the fake secret.
//!
//! What this covers: the passphrase path (socket, decoder, parser, audit, stderr/stdout, files,
//! /proc/<pid>/cmdline and environ) and the vault secret while it sits decrypted in daemon
//! memory. What it cannot cover: a secret-backed provider, because the shipped binary has none
//! yet; the provider path is covered in-process by tests/unlock.rs.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use ow_core::Paths;
use ow_vault::{Profile, Vault};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const PASS: &str = "FAKE-passphrase-correct-horse";
const WRONG: &str = "FAKE-passphrase-wrong-horse!";
const TOKEN: &str = "FAKE-TOKEN-VALUE-77c1e";

fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

fn ask(paths: &Paths, line: &str) -> String {
    let mut s = UnixStream::connect(paths.socket()).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(line.as_bytes()).unwrap();
    let mut out = String::new();
    BufReader::new(s).read_line(&mut out).unwrap();
    out
}

fn drain(mut r: impl Read + Send + 'static) -> Arc<Mutex<Vec<u8>>> {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let b2 = Arc::clone(&buf);
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = r.read(&mut chunk) {
            if n == 0 {
                break;
            }
            b2.lock().unwrap().extend_from_slice(&chunk[..n]);
        }
    });
    buf
}

fn files(p: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(p).unwrap() {
        let path = e.unwrap().path();
        let md = std::fs::symlink_metadata(&path).unwrap();
        if md.is_dir() {
            files(&path, out);
        } else if md.is_file() {
            out.push(path);
        }
    }
}

#[test]
fn daemon_never_leaks_the_fake_passphrase_or_secret() {
    // Short absolute path under /tmp (socket paths are limited to 108 bytes).
    let dir = tempfile::Builder::new()
        .prefix("owd.")
        .tempdir_in("/tmp")
        .unwrap();
    let paths = Paths::under(dir.path());
    std::fs::create_dir_all(&paths.config).unwrap();
    std::fs::write(
        paths.grants(),
        "[[grant]]\nwidget=\"w1\"\ntopics=[\"system.load\"]\nfields=[\"load1\"]\n",
    )
    .unwrap();
    std::fs::set_permissions(paths.grants(), std::fs::Permissions::from_mode(0o600)).unwrap();
    ow_core::private_dir(&paths.data).unwrap();
    let mut v = Vault::create(&paths.vault(), PASS.as_bytes(), Profile::Standard).unwrap();
    v.set("fake-token", TOKEN).unwrap();
    drop(v);

    let mut child = Command::new(env!("CARGO_BIN_EXE_owd"))
        .arg("--config-dir")
        .arg(dir.path())
        .env_clear()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = drain(child.stdout.take().unwrap());
    let err = drain(child.stderr.take().unwrap());
    for _ in 0..100 {
        if String::from_utf8_lossy(&err.lock().unwrap()).contains("listening") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    // A full session: refused attempts, hostile frames, unlock, use, lock, unlock again.
    let unlock = |p: &str| ask(&paths, &ow_proto::unlock_request(1, p.as_bytes()).unwrap());
    assert!(unlock(WRONG).contains("denied"));
    for bad in [
        format!(
            "{{\"v\":0,\"id\":2,\"op\":\"vault.unlock\",\"passphrase\":\"{}zz\"}}\n",
            hex(PASS)
        ),
        format!(
            "{{\"v\":0,\"id\":2,\"op\":\"vault.unlock\",\"widget\":\"w1\",\"passphrase\":\"{}\"}}\n",
            hex(PASS)
        ),
        format!(
            "{{\"v\":0,\"id\":2,\"op\":\"snapshot\",\"widget\":\"w1\",\"topic\":\"vault.unlock\",\"passphrase\":\"{PASS}\"}}\n"
        ),
        format!(
            "{{\"v\":9,\"id\":2,\"op\":\"vault.unlock\",\"passphrase\":\"{}\"}}\n",
            hex(PASS)
        ),
    ] {
        assert!(ask(&paths, &bad).contains("error"));
    }
    assert!(unlock(PASS).contains("unlocked"));
    assert!(ask(&paths, "{\"v\":0,\"id\":3,\"op\":\"vault.status\"}\n").contains("fake-token"));
    let snap = ask(
        &paths,
        "{\"v\":0,\"id\":4,\"op\":\"snapshot\",\"widget\":\"w1\",\"topic\":\"system.load\"}\n",
    );
    assert!(snap.contains("\"ok\":true"), "{snap}");
    assert!(ask(&paths, "{\"v\":0,\"id\":5,\"op\":\"vault.lock\"}\n").contains("locked"));
    assert!(unlock(PASS).contains("unlocked")); // left unlocked: the secret is in memory now

    // Inspect while the secret is resident.
    let pid = child.id();
    let mut proc_text = String::new();
    for f in ["cmdline", "environ", "comm", "status", "limits"] {
        // environ is root-owned for a non-dumpable process: unreadable is expected, and the
        // environment we gave the process was empty anyway.
        if let Ok(b) = std::fs::read(format!("/proc/{pid}/{f}")) {
            proc_text.push_str(&String::from_utf8_lossy(&b));
        }
    }
    assert!(proc_text.contains("owd"), "cmdline should be readable");

    rustix::process::kill_process(
        rustix::process::Pid::from_raw(pid as i32).unwrap(),
        rustix::process::Signal::TERM,
    )
    .unwrap();
    child.wait().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let stdout = String::from_utf8_lossy(&out.lock().unwrap()).into_owned();
    let stderr = String::from_utf8_lossy(&err.lock().unwrap()).into_owned();
    assert!(
        stderr.contains("listening"),
        "stderr capture works: {stderr}"
    );

    // Everything that could hold a secret, searched for every needle in plain and hex form.
    let mut all: Vec<(String, Vec<u8>)> = vec![
        ("stdout".into(), stdout.into_bytes()),
        ("stderr".into(), stderr.into_bytes()),
        ("/proc".into(), proc_text.into_bytes()),
    ];
    let mut fs = Vec::new();
    files(dir.path(), &mut fs);
    assert!(fs.iter().any(|f| f.ends_with("audit.jsonl")));
    for f in fs {
        all.push((f.display().to_string(), std::fs::read(&f).unwrap()));
    }
    let audit = std::fs::read_to_string(paths.audit()).unwrap();
    assert!(audit.contains("\"op\":\"unlock\"") && audit.contains("\"op\":\"lock\""));
    for (name, bytes) in &all {
        for needle in [PASS, WRONG, TOKEN] {
            for form in [needle.to_owned(), hex(needle)] {
                assert!(
                    !bytes.windows(form.len()).any(|w| w == form.as_bytes()),
                    "{needle} found in {name}"
                );
            }
        }
    }
}
