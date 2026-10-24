//! `ow doctor` driven as the real binary against throwaway trees: a healthy one served by an
//! in-process `owd`, three broken ones, and a hostile fake daemon. Fake values only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)] // prints reports for --nocapture evidence
mod install_common;
use install_common::*;
use ow_core::Paths;
use ow_vault::{Profile, Vault};
use owd::Server;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::net::UnixListener;
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Tests here spawn children while another test may hold a vault lock file open (see daemon_cli).
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
const PASS: &str = "FAKE-passphrase-correct-horse";
const TOKEN: &str = "FAKE-TOKEN-VALUE-77c1e";
const CLOCK: &str = "io.github.dexterhere.clock";

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

fn out(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn chmod(p: &std::path::Path, m: u32) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
}

/// A tree with private dirs, one installed widget, a good grants file, a vault, and a unit.
fn healthy() -> (tempfile::TempDir, Paths) {
    let (d, paths) = tree();
    for p in [&paths.data, &paths.state] {
        ow_core::private_dir(p).unwrap();
    }
    let src = world("clock", SHA_A, "alpha", "community", |_| {});
    install(&paths, &src, &[CLOCK], &mut Script::says(&["yes"]))
        .result
        .unwrap();
    let mut v = Vault::create(&paths.vault(), PASS.as_bytes(), Profile::Standard).unwrap();
    v.set("github-main", TOKEN).unwrap();
    drop(v);
    // Unit, its binary, and the enable link, all inside the tree (config home = <root>/config).
    let ud = d.path().join("config/systemd/user");
    std::fs::create_dir_all(ud.join("default.target.wants")).unwrap();
    let bin = d.path().join("bin-owd");
    std::fs::write(&bin, "#!/bin/sh\n").unwrap();
    chmod(&bin, 0o755);
    std::fs::write(
        ud.join("owd.service"),
        format!("[Service]\nExecStart=\"{}\"\n", bin.display()),
    )
    .unwrap();
    symlink(
        ud.join("owd.service"),
        ud.join("default.target.wants/owd.service"),
    )
    .unwrap();
    (d, paths)
}

fn serve(paths: &Paths) -> (Arc<AtomicBool>, std::thread::JoinHandle<()>) {
    let server = Server::bind(paths, Vec::new()).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (s2, r2) = (Arc::clone(&stop), Arc::new(AtomicBool::new(false)));
    (stop, std::thread::spawn(move || server.run(&s2, &r2)))
}

#[test]
fn healthy_tree_exits_zero_and_never_prints_secrets() {
    let _s = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (d, paths) = healthy();
    let (stop, th) = serve(&paths);
    let locked = ow(d.path(), &["doctor"], "");
    ow(d.path(), &["vault", "unlock"], &format!("{PASS}\n"));
    let unlocked = ow(d.path(), &["doctor"], "");
    let json = ow(d.path(), &["doctor", "--json", "--names"], "");
    let json_plain = ow(d.path(), &["doctor", "--json"], "");
    println!("{}", out(&unlocked));
    for o in [&locked, &unlocked] {
        assert!(o.status.success(), "{}", out(o));
        assert!(!out(o).contains("FAIL"), "{}", out(o));
    }
    assert!(out(&locked).contains("Vault is locked"));
    let t = out(&unlocked);
    // Reports get pasted: names stay hidden unless asked for.
    assert!(
        t.contains("Vault is unlocked; 1 secret(s), names hidden, add --names"),
        "{t}"
    );
    assert!(!t.contains("github-main") && !out(&json_plain).contains("github-main"));
    assert!(out(&json).contains("secret name(s): [github-main]"));
    assert!(
        t.contains(CLOCK) && t.contains("Files are intact and valid"),
        "{t}"
    );
    assert!(
        t.contains("SKIP  plugin") && t.contains("Nothing was changed"),
        "{t}"
    );
    let all = format!("{}{}{}{}", out(&locked), t, out(&json), out(&json_plain));
    for needle in [PASS, TOKEN] {
        assert!(!all.contains(needle), "{needle} leaked");
    }
    let j: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(j["ok"], true);
    assert!(
        j["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["level"] != "fail")
    );
    stop.store(true, Ordering::Relaxed);
    th.join().unwrap();
}

#[test]
fn doctor_is_read_only() {
    let _s = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (d, paths) = healthy();
    let (stop, th) = serve(&paths);
    let before = snapshot(d.path());
    let o = ow(d.path(), &["doctor"], "");
    assert!(o.status.success(), "{}", out(&o));
    // Ignore the daemon's own files (lock file, audit log) by comparing only what we made.
    let after = snapshot(d.path());
    for (k, v) in &before {
        if k.starts_with("run") || k.starts_with("state") {
            continue;
        }
        assert_eq!(after.get(k), Some(v), "{k:?} changed");
    }
    stop.store(true, Ordering::Relaxed);
    th.join().unwrap();
}

#[test]
fn broken_one_daemon_down_and_loose_data_dir() {
    let _s = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (d, paths) = healthy();
    chmod(&paths.data, 0o755);
    let o = ow(d.path(), &["doctor"], "");
    let t = out(&o);
    println!("{t}");
    assert_eq!(o.status.code(), Some(1), "{t}");
    assert!(
        t.contains("FAIL  data-dir") && t.contains("mode 0755; it must be 0700"),
        "{t}"
    );
    assert!(t.contains("fix: chmod 700"), "{t}");
    assert!(
        t.contains("FAIL  daemon") && t.contains("not running"),
        "{t}"
    );
    assert!(t.contains("SKIP  version"), "{t}");
}

#[test]
fn broken_two_garbage_grants_and_open_vault_file() {
    let _s = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (d, paths) = healthy();
    let (stop, th) = serve(&paths);
    write_grants(&paths, "[[grant]]\nnonsense = true\n\u{0}\u{1}", 0o600);
    chmod(&paths.vault(), 0o644);
    let o = ow(d.path(), &["doctor"], "");
    let t = out(&o);
    println!("{t}");
    assert_eq!(o.status.code(), Some(1), "{t}");
    assert!(t.contains("FAIL  grants") && t.contains("not valid"), "{t}");
    assert!(t.contains("FAIL  vault-file") && t.contains("0644"), "{t}");
    assert!(t.contains("OK    daemon"), "{t}");
    stop.store(true, Ordering::Relaxed);
    th.join().unwrap();
}

#[test]
fn broken_three_tampered_widget_and_dead_unit_binary() {
    let _s = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (d, paths) = healthy();
    let layout = paths.widgets().join(CLOCK).join("layout.json");
    std::fs::write(&layout, b"{\"tampered\":true}").unwrap();
    // The unit points at a binary that is gone.
    std::fs::remove_file(d.path().join("bin-owd")).unwrap();
    let (stop, th) = serve(&paths);
    let o = ow(d.path(), &["doctor"], "");
    let t = out(&o);
    println!("{t}");
    assert_eq!(o.status.code(), Some(1), "{t}");
    assert!(
        t.contains(&format!("FAIL  widget {CLOCK}"))
            && t.contains("layout.json changed since it was installed"),
        "{t}"
    );
    assert!(
        t.contains("FAIL  unit") && t.contains("missing or not executable"),
        "{t}"
    );
    stop.store(true, Ordering::Relaxed);
    th.join().unwrap();
}

/// Whatever answers on the socket is another process: its text is cut and cleaned, and a version
/// that differs is a warning, not a crash.
#[test]
fn hostile_daemon_text_cannot_reach_the_terminal() {
    let _s = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (d, paths) = healthy();
    ow_core::private_dir(&paths.runtime).unwrap();
    let l = UnixListener::bind(paths.socket()).unwrap();
    let th = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut s, _) = l.accept().unwrap();
            let mut line = String::new();
            BufReader::new(s.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let reply = if line.contains("vault.status") {
                "{\"v\":0,\"id\":2,\"ok\":true,\"state\":\"unlocked\",\"count\":1,\"names\":[\"a\\u001b[2Jb\"]}\n"
            } else {
                "{\"v\":0,\"id\":1,\"ok\":true,\"daemonVersion\":\"9\\u001b]0;pwned\\u0007\",\"protocol\":0}\n"
            };
            s.write_all(reply.as_bytes()).unwrap();
        }
    });
    let o = ow(d.path(), &["doctor", "--names"], "");
    th.join().unwrap();
    let t = out(&o);
    assert!(!t.contains('\u{1b}') && !t.contains('\u{7}'), "{t:?}");
    assert!(t.contains("WARN  version"), "{t}");
}

#[test]
fn usage_errors() {
    let (d, _p) = tree();
    let o = ow(d.path(), &["doctor", "--bogus"], "");
    assert_eq!(o.status.code(), Some(1));
    assert!(out(&o).contains("usage: ow doctor [--json]"));
}

/// A daemon that accepts and never answers must cost seconds, not the 60 s of other verbs.
#[test]
fn mute_daemon_is_a_fail_within_seconds() {
    let _s = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (d, paths) = healthy();
    ow_core::private_dir(&paths.runtime).unwrap();
    let l = UnixListener::bind(paths.socket()).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = Arc::clone(&stop);
    let th = std::thread::spawn(move || {
        l.set_nonblocking(true).unwrap();
        let mut held = Vec::new();
        while !s2.load(Ordering::Relaxed) {
            if let Ok((s, _)) = l.accept() {
                held.push(s); // keep it open, say nothing
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    });
    let t0 = std::time::Instant::now();
    let o = ow(d.path(), &["doctor"], "");
    let took = t0.elapsed();
    stop.store(true, Ordering::Relaxed);
    th.join().unwrap();
    let t = out(&o);
    assert_eq!(o.status.code(), Some(1), "{t}");
    assert!(t.contains("FAIL  daemon"), "{t}");
    assert!(took < std::time::Duration::from_secs(7), "took {took:?}");
}

/// No HOME and no XDG_RUNTIME_DIR: still a report, with SKIP and the reason, exit 0.
#[test]
fn missing_home_and_runtime_dir_are_skips_not_a_crash() {
    for env in [
        vec![],
        vec![("HOME", "relative"), ("XDG_RUNTIME_DIR", "run")],
    ] {
        let o = Command::new(env!("CARGO_BIN_EXE_ow"))
            .arg("doctor")
            .env_clear()
            .envs(env)
            .output()
            .unwrap();
        let t = out(&o);
        assert!(o.status.success(), "{t}");
        assert!(
            t.contains("SKIP  daemon") && t.contains("XDG_RUNTIME_DIR is unset or not absolute"),
            "{t}"
        );
        assert!(t.contains("SKIP  data-dir"), "{t}");
        assert!(!t.contains("FAIL"), "{t}");
    }
}

/// The `fix:` text is meant to be pasted: run it in a shell inside a sandbox and prove that a
/// hostile file name is only ever an argument.
#[test]
fn quoted_fix_commands_cannot_inject() {
    let d = tempfile::tempdir().unwrap();
    let victim = d.path().join("q'; touch PWNED #");
    std::fs::write(&victim, "x").unwrap();
    chmod(&victim, 0o644);
    let line = ow::doctor::cmd_path("chmod 600", &victim).unwrap();
    let st = Command::new("sh")
        .arg("-c")
        .arg(&line)
        .current_dir(d.path())
        .status()
        .unwrap();
    assert!(st.success());
    assert!(!d.path().join("PWNED").exists());
    assert_eq!(
        std::fs::metadata(&victim).unwrap().permissions().mode() & 0o777,
        0o600
    );
}
