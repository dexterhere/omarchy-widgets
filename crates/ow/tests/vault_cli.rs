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
    c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
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
