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
