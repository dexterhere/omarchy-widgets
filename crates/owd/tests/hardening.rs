//! Runs the real binary: process-wide settings (umask, dumpable, core limit) cannot be
//! asserted in-process without affecting other tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

#[test]
fn daemon_process_is_hardened() {
    let dir = tempfile::Builder::new()
        .prefix("owd")
        .tempdir_in("/tmp")
        .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_owd"))
        .arg("--config-dir")
        .arg(dir.path())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stderr.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(line.contains("listening"), "{line}");
    let pid = child.id();
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
    let limits = std::fs::read_to_string(format!("/proc/{pid}/limits")).unwrap();
    let umask = status.lines().find(|l| l.starts_with("Umask:")).unwrap();
    assert!(umask.ends_with("0077"), "{umask}");
    let core = limits
        .lines()
        .find(|l| l.starts_with("Max core file size"))
        .unwrap();
    assert!(core.split_whitespace().rev().nth(1) == Some("0"), "{core}");
    // PR_SET_DUMPABLE=0 makes /proc/<pid>/{environ,mem} root-owned for the user's other processes.
    let md = std::fs::metadata(format!("/proc/{pid}/environ")).unwrap();
    assert_eq!(
        std::os::unix::fs::MetadataExt::uid(&md),
        0,
        "process is still dumpable"
    );
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn hostile_tz_stops_startup_with_exit_2_and_never_prints_the_value() {
    let dir = tempfile::Builder::new()
        .prefix("owd.")
        .tempdir_in("/tmp")
        .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_owd"))
        .arg("--config-dir")
        .arg(dir.path())
        .env_clear()
        .env("TZ", "/dev/zero")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("TZ") && !err.contains("/dev/zero") && err.lines().count() == 1);
}

/// Start the real binary on `dir`; return its exit code and stderr if it stops by itself.
fn start_with_config(text: &str, mode: u32) -> (Option<i32>, String) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::Builder::new()
        .prefix("owd.")
        .tempdir_in("/tmp")
        .unwrap();
    let cfg = dir.path().join("config/owd");
    std::fs::create_dir_all(&cfg).unwrap();
    let file = cfg.join("config.toml");
    std::fs::write(&file, text).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_owd"))
        .arg("--config-dir")
        .arg(dir.path())
        .env_clear()
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut err = String::new();
    let mut rd = BufReader::new(child.stderr.take().unwrap());
    let mut line = String::new();
    rd.read_line(&mut line).unwrap();
    err.push_str(&line);
    if line.contains("listening") {
        child.kill().unwrap();
        child.wait().unwrap();
        return (None, err);
    }
    let code = child.wait().unwrap().code();
    (code, err)
}

#[test]
fn a_bad_config_file_stops_startup_without_echoing_it() {
    const SECRET_LOOKING: &str = "FAKE-pat-0123456789abcdef";
    let bad: [(String, u32); 6] = [
        // world-writable / group-readable
        ("[providers.github]\nsecret = \"github-main\"\n".into(), 0o666),
        ("[providers.github]\nsecret = \"github-main\"\n".into(), 0o640),
        // a secret name outside the provider's pattern
        ("[providers.github]\nsecret = \"gmail-app-pw\"\n".into(), 0o600),
        // unknown key, and a token pasted where a name belongs
        (format!("[providers.github]\nsecret = \"github-a\"\ntoken = \"{SECRET_LOOKING}\"\n"), 0o600),
        (format!("[providers.github]\nsecret = \"{SECRET_LOOKING}\"\n"), 0o600),
        // duplicate section
        ("[providers.github]\nsecret = \"github-a\"\n[providers.github]\nsecret = \"github-b\"\n".into(), 0o600),
    ];
    for (text, mode) in bad {
        let (code, err) = start_with_config(&text, mode);
        assert_eq!(code, Some(1), "{text:?} {mode:o}: {err}");
        assert!(err.starts_with("owd: config: "), "{err}");
        assert!(!err.contains("FAKE") && !err.contains("gmail"), "{err}");
    }
}

#[test]
fn a_valid_config_starts_the_daemon_and_a_missing_one_does_too() {
    let (code, err) = start_with_config(
        "[providers.github]\nsecret = \"github-main\"\ninterval_secs = 300\n",
        0o600,
    );
    assert_eq!(code, None, "{err}");
    let (code, err) = start_with_config("", 0o600);
    assert_eq!(code, None, "{err}");
}
