#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::io::Write;
use std::process::{Command, Output, Stdio};

const PASS: &str = "correct horse battery";
const NEW_PASS: &str = "new passphrase long";
const SECRET: &str = "FAKE-SECRET-VALUE-9f3a";

fn ow(dir: &std::path::Path, args: &[&str], stdin: &str) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_ow"))
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
            &["vault", "set", "github-main"],
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
        ow(d.path(), &["vault", "set", "x"], "wrong passphrase!!\nv\n"),
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
        &["vault", "set", "a"],
        &format!("{PASS}\nhalf-written"),
    );
    assert!(
        !o.status.success() && text(&o).contains("unexpected end of input"),
        "{}",
        text(&o)
    );
    // Passphrase line only, value missing.
    assert!(
        !ow(d.path(), &["vault", "set", "a"], &format!("{PASS}\n"))
            .status
            .success()
    );
    // Empty value.
    let o = ow(d.path(), &["vault", "set", "a"], &format!("{PASS}\n\n"));
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
        &["vault", "set", "x"],
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
