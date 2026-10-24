#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::io::Write;
use std::process::{Command, Output, Stdio};

const PASS: &str = "correct horse battery";
const NEW_PASS: &str = "new passphrase long";
const SECRET: &str = "FAKE-SECRET-VALUE-9f3a";

/// Runs `ow` in a new session, so it has no controlling terminal even when the developer runs
/// the tests from one: a prompt must never reach (or wait on) the real terminal.
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
    // The process may exit before reading (expected failures): a broken pipe is fine.
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

fn walk(p: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    for e in std::fs::read_dir(p).unwrap() {
        let e = e.unwrap().path();
        if e.is_dir() {
            v.extend(walk(&e));
        } else {
            v.push(e);
        }
    }
    v
}

#[test]
fn full_session_never_prints_or_stores_secrets_outside_the_vault() {
    let d = tempfile::tempdir().unwrap();
    let all = [
        ow(d.path(), &["vault", "init"], &format!("{PASS}\n")),
        ow(d.path(), &["vault", "status"], ""),
        ow(
            d.path(),
            &["vault", "set", "github-main", "--passphrase-stdin"],
            &format!("{PASS}\n{SECRET}\n"),
        ),
        ow(d.path(), &["vault", "list"], &format!("{PASS}\n")),
        ow(
            d.path(),
            &["vault", "rotate"],
            &format!("{PASS}\n{NEW_PASS}\n"),
        ),
        ow(
            d.path(),
            &["vault", "remove", "github-main"],
            &format!("{NEW_PASS}\n"),
        ),
        ow(
            d.path(),
            &["vault", "set", "x", "--passphrase-stdin"],
            "wrong passphrase!!\nv\n",
        ),
    ];
    for o in &all[..6] {
        assert!(o.status.success(), "{}", text(o));
    }
    assert!(!all[6].status.success());
    let combined: String = all.iter().map(text).collect();
    for needle in [SECRET, PASS, NEW_PASS] {
        assert!(!combined.contains(needle), "{needle} leaked in output");
    }
    assert!(text(&all[3]).contains("github-main"));
    // No file anywhere (vault included, it is encrypted) holds a secret or passphrase in clear.
    for f in walk(d.path()) {
        let bytes = std::fs::read(&f).unwrap();
        for needle in [SECRET, PASS, NEW_PASS] {
            assert!(
                !bytes.windows(needle.len()).any(|w| w == needle.as_bytes()),
                "{f:?}"
            );
        }
    }
}

#[test]
fn secret_in_argv_is_rejected_not_stored() {
    let d = tempfile::tempdir().unwrap();
    assert!(
        ow(d.path(), &["vault", "init"], &format!("{PASS}\n"))
            .status
            .success()
    );
    let o = ow(
        d.path(),
        &["vault", "set", "zzz", SECRET],
        &format!("{PASS}\n"),
    );
    assert!(!o.status.success());
    assert!(
        text(&o).contains("usage") && !text(&o).contains(SECRET),
        "{}",
        text(&o)
    );
    let l = ow(d.path(), &["vault", "list"], &format!("{PASS}\n"));
    assert!(!text(&l).contains("zzz"), "nothing should be stored");
}

#[test]
fn weak_passphrase_and_wipe_wording() {
    let d = tempfile::tempdir().unwrap();
    assert!(!ow(d.path(), &["vault", "init"], "short\n").status.success());
    assert!(
        ow(d.path(), &["vault", "init"], &format!("{PASS}\n"))
            .status
            .success()
    );
    let w = ow(d.path(), &["vault", "wipe", "--yes"], "");
    assert!(
        w.status.success() && text(&w).contains("NOT erased"),
        "{}",
        text(&w)
    );
    assert!(text(&ow(d.path(), &["vault", "status"], "")).contains("not initialised"));
}

#[test]
fn vault_file_and_dir_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    assert!(
        ow(d.path(), &["vault", "init"], &format!("{PASS}\n"))
            .status
            .success()
    );
    let p = d.path().join("data/owd/vault.owv");
    assert_eq!(
        std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(p.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn truncated_empty_and_crlf_input() {
    let d = tempfile::tempdir().unwrap();
    // CRLF line endings are accepted (one trailing CR stripped).
    assert!(
        ow(d.path(), &["vault", "init"], &format!("{PASS}\r\n"))
            .status
            .success()
    );
    // EOF mid-line: no trailing newline must not become a secret.
    let o = ow(
        d.path(),
        &["vault", "set", "a", "--passphrase-stdin"],
        &format!("{PASS}\nhalf-written"),
    );
    assert!(
        !o.status.success() && text(&o).contains("unexpected end of input"),
        "{}",
        text(&o)
    );
    // Passphrase line only, value missing.
    assert!(
        !ow(
            d.path(),
            &["vault", "set", "a", "--passphrase-stdin"],
            &format!("{PASS}\n")
        )
        .status
        .success()
    );
    // Empty value.
    let o = ow(
        d.path(),
        &["vault", "set", "a", "--passphrase-stdin"],
        &format!("{PASS}\n\n"),
    );
    assert!(
        !o.status.success() && text(&o).contains("empty"),
        "{}",
        text(&o)
    );
    assert!(!text(&ow(d.path(), &["vault", "list"], &format!("{PASS}\n"))).contains('a'));
}

#[test]
fn status_does_not_hide_permission_errors() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    assert!(
        ow(d.path(), &["vault", "init"], &format!("{PASS}\n"))
            .status
            .success()
    );
    let dir = d.path().join("data/owd");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).unwrap();
    let o = ow(d.path(), &["vault", "status"], "");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(!text(&o).contains("not initialised"), "{}", text(&o));
    assert!(!o.status.success());
}

#[test]
fn wipe_absent_vault_and_stray_temp_files() {
    let d = tempfile::tempdir().unwrap();
    let w = ow(d.path(), &["vault", "wipe", "--yes"], "");
    assert!(!w.status.success());
    assert!(
        !d.path().join("data/owd/vault.owv.lock").exists(),
        "wipe must not create a lock"
    );
    assert!(
        ow(d.path(), &["vault", "init"], &format!("{PASS}\n"))
            .status
            .success()
    );
    let dir = d.path().join("data/owd");
    std::fs::write(dir.join("vault.owv.0123456789abcdef.tmp"), b"stray").unwrap();
    std::fs::write(dir.join("vault.owv.notmine.tmp"), b"keep").unwrap();
    assert!(
        ow(d.path(), &["vault", "wipe", "--yes"], "")
            .status
            .success()
    );
    assert!(!dir.join("vault.owv").exists());
    assert!(!dir.join("vault.owv.0123456789abcdef.tmp").exists());
    assert!(
        dir.join("vault.owv.notmine.tmp").exists(),
        "must not delete unrelated files"
    );
}

#[test]
fn relative_config_dir_and_loose_data_dir_are_refused() {
    use std::os::unix::fs::PermissionsExt;
    let o = Command::new(env!("CARGO_BIN_EXE_ow"))
        .args(["--config-dir", "relative/dir", "vault", "status"])
        .output()
        .unwrap();
    assert!(
        !o.status.success() && text(&o).contains("absolute"),
        "{}",
        text(&o)
    );
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("data/owd");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let o = ow(d.path(), &["vault", "init"], &format!("{PASS}\n"));
    assert!(
        !o.status.success() && text(&o).contains("0700"),
        "{}",
        text(&o)
    );
    assert!(!dir.join("vault.owv").exists());
}

#[test]
fn cli_process_is_hardened_while_holding_secrets() {
    use std::os::unix::fs::MetadataExt;
    let d = tempfile::tempdir().unwrap();
    assert!(
        ow(d.path(), &["vault", "init"], &format!("{PASS}\n"))
            .status
            .success()
    );
    // `list` blocks reading the passphrase from the open stdin pipe.
    let mut c = Command::new(env!("CARGO_BIN_EXE_ow"))
        .arg("--config-dir")
        .arg(d.path())
        .args(["vault", "list"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = c.id();
    let mut ok = false;
    for _ in 0..200 {
        // Root-owned environ means PR_SET_DUMPABLE is 0 (hardening has run).
        if std::fs::metadata(format!("/proc/{pid}/environ")).is_ok_and(|m| m.uid() == 0) {
            ok = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let limits = std::fs::read_to_string(format!("/proc/{pid}/limits")).unwrap_or_default();
    c.kill().unwrap();
    c.wait().unwrap();
    assert!(ok, "ow stayed dumpable");
    let core = limits
        .lines()
        .find(|l| l.starts_with("Max core file size"))
        .unwrap();
    assert_eq!(core.split_whitespace().rev().nth(1), Some("0"), "{core}");
}

#[test]
fn every_vault_command_refuses_a_loose_vault_dir() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("data/owd");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let stdin = format!("{PASS}\n{SECRET}\n{SECRET}\n");
    for args in [
        &["vault", "status"][..],
        &["vault", "list"],
        &["vault", "set", "x", "--passphrase-stdin"],
        &["vault", "remove", "x"],
        &["vault", "rotate"],
        &["vault", "wipe", "--yes"],
    ] {
        let o = ow(d.path(), args, &stdin);
        assert!(
            !o.status.success() && text(&o).contains("0700"),
            "{args:?}: {}",
            text(&o)
        );
    }
}

// ---- `vault set`: passphrase from the terminal, value from a pipe ----

fn init(d: &std::path::Path) {
    assert!(
        ow(d, &["vault", "init"], &format!("{PASS}\n"))
            .status
            .success()
    );
}

#[test]
fn set_without_a_terminal_fails_instead_of_reading_the_passphrase_from_the_pipe() {
    let d = tempfile::tempdir().unwrap();
    init(d.path());
    // `ow` runs in a new session here: no controlling terminal. The pipe holds only a value.
    let o = ow(d.path(), &["vault", "set", "tok"], &format!("{SECRET}\n"));
    assert!(!o.status.success());
    let t = text(&o);
    assert!(
        t.contains("no terminal available") && t.contains("--passphrase-stdin"),
        "{t}"
    );
    assert!(!t.contains(SECRET));
    // Nothing was stored, and the value was not tried as a passphrase.
    let l = ow(d.path(), &["vault", "list"], &format!("{PASS}\n"));
    assert!(!text(&l).contains("tok"));
}

/// Real pseudo-terminal: the child's controlling terminal is the pty (setsid -c), its stdin is a
/// file holding the value. The passphrase is typed at the prompt; the value comes from stdin.
#[test]
fn set_takes_the_passphrase_from_the_tty_and_the_value_from_stdin() {
    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
    use std::io::Read;
    use std::os::unix::ffi::OsStrExt;
    let d = tempfile::tempdir().unwrap();
    init(d.path());
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
    grantpt(&master).unwrap();
    unlockpt(&master).unwrap();
    let name = ptsname(&master, Vec::new()).unwrap();
    let slave = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(std::ffi::OsStr::from_bytes(name.to_bytes()))
        .unwrap();
    let valdir = tempfile::tempdir().unwrap();
    let valfile = valdir.path().join("value");
    std::fs::write(&valfile, format!("{SECRET}\n")).unwrap();
    let child = Command::new("setsid")
        .args([
            "-c",
            "-w",
            "sh",
            "-c",
            r#"exec "$0" "$@" < "$OW_VALUE_FILE""#,
        ])
        .arg(env!("CARGO_BIN_EXE_ow"))
        .arg("--config-dir")
        .arg(d.path())
        .args(["vault", "set", "tok"])
        .env("OW_VALUE_FILE", &valfile)
        .stdin(slave.try_clone().unwrap())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut master = std::fs::File::from(master);
    let mut writer = master.try_clone().unwrap();
    // Terminal output arrives on a thread so a misbehaving child fails the test, not hangs it.
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 256];
        while let Ok(n) = master.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let wait = std::time::Duration::from_secs(10);
    let mut seen = Vec::new();
    while !String::from_utf8_lossy(&seen).contains("Vault passphrase: ") {
        seen.extend(rx.recv_timeout(wait).unwrap_or_else(|_| {
            panic!(
                "prompt never appeared: {:?}",
                String::from_utf8_lossy(&seen)
            )
        }));
    }
    // Ctrl-U is refused with a message and the prompt comes back; nothing is submitted.
    writer.write_all(b"abc\x15").unwrap();
    while String::from_utf8_lossy(&seen)
        .matches("Vault passphrase: ")
        .count()
        < 2
    {
        seen.extend(
            rx.recv_timeout(wait)
                .unwrap_or_else(|_| panic!("no re-prompt: {:?}", String::from_utf8_lossy(&seen))),
        );
    }
    assert!(String::from_utf8_lossy(&seen).contains("Try again"));
    writer.write_all(format!("{PASS}\n").as_bytes()).unwrap();
    let o = child.wait_with_output().unwrap();
    while let Ok(more) = rx.recv_timeout(std::time::Duration::from_millis(300)) {
        seen.extend(more);
    }
    assert!(
        o.status.success(),
        "{} / {:?}",
        text(&o),
        String::from_utf8_lossy(&seen)
    );
    assert!(text(&o).contains("stored tok"));
    // No echo of what was typed, and the value never touched the terminal or the output.
    let term = String::from_utf8_lossy(&seen).into_owned();
    for needle in [PASS, SECRET] {
        assert!(!term.contains(needle) && !text(&o).contains(needle));
    }
    drop(slave);
    // Stored: list shows the name, using the explicit stdin alternative for the passphrase.
    let l = ow(d.path(), &["vault", "list"], &format!("{PASS}\n"));
    assert!(text(&l).contains("tok"));
}
