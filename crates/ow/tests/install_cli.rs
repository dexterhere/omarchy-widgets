//! The real `ow` binary for the install verbs, on paths that need no network: usage and flag
//! refusals, no terminal, hostile ids, list and idempotent remove. Runs in a new session so it
//! has no controlling terminal (a prompt must never reach the developer's terminal).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn ow(dir: &std::path::Path, args: &[&str]) -> (Output, Duration) {
    let t = Instant::now();
    let mut c = Command::new("setsid")
        .arg("-w")
        .arg(env!("CARGO_BIN_EXE_ow"))
        .arg("--config-dir")
        .arg(dir)
        .args(args)
        // An environment variable must not be able to redirect the registry.
        .env("OW_REGISTRY", "https://attacker.example")
        .env("REGISTRY", "https://attacker.example")
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = c.stdin.take().unwrap().write_all(b"yes\nyes\n");
    let o = c.wait_with_output().unwrap();
    (o, t.elapsed())
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn usage_names_the_new_verbs() {
    let d = tempfile::tempdir().unwrap();
    let (o, _) = ow(d.path(), &[]);
    let t = text(&o);
    assert!(!o.status.success());
    for want in [
        "install <id>",
        "--registry https://<host>",
        "--allow-planned",
        "--dry-run",
        "--accept-access",
        "ow list",
        "ow remove <id>",
    ] {
        assert!(t.contains(want), "{want}\n{t}");
    }
}

#[test]
fn bad_flags_and_registries_are_refused_before_anything_happens() {
    let d = tempfile::tempdir().unwrap();
    let id = "io.github.dexterhere.clock";
    for args in [
        vec!["install"],
        vec!["install", id, "--bogus"],
        vec!["install", id, id],
        vec!["install", "../x"],
        vec!["install", "A.B"],
        vec!["install", id, "--registry"],
        vec!["install", id, "--registry", "http://registry.example.org"],
        vec!["install", id, "--registry", "https://127.0.0.1"],
        vec!["install", id, "--registry", "https://localhost"],
        vec!["install", id, "--registry", "https://Registry.Example.org"],
        vec![
            "install",
            id,
            "--registry",
            "https://registry.example.org:8443",
        ],
        vec![
            "install",
            id,
            "--registry",
            "https://registry.example.org/x",
        ],
        vec![
            "install",
            id,
            "--registry",
            "https://user@registry.example.org",
        ],
        vec!["install", id, "--registry", "https://registry.example.org/"],
        vec![
            "install",
            id,
            "--registry=https://a.b",
            "--registry=https://c.d",
        ],
        vec!["install", id, "--yes"],
        vec!["install", id, "--accept-access", &"0".repeat(64)],
        vec!["install", id, "--yes", "--accept-access", "x"],
        vec!["install", id, "--allow-planned", "--allow-planned"],
        vec!["list", "extra"],
        vec!["remove"],
        vec!["remove", id, id],
        vec!["remove", "../etc"],
        vec!["remove", ".."],
    ] {
        let (o, took) = ow(d.path(), &args);
        assert!(!o.status.success(), "{args:?} was accepted: {}", text(&o));
        assert!(
            took < Duration::from_secs(5),
            "{args:?} took {took:?}: did it try the network?"
        );
    }
    // Nothing was created by any refusal.
    assert!(std::fs::read_dir(d.path()).unwrap().next().is_none());
}

#[test]
fn without_a_terminal_install_refuses_before_the_network() {
    let d = tempfile::tempdir().unwrap();
    let (o, took) = ow(d.path(), &["install", "io.github.dexterhere.clock"]);
    assert!(!o.status.success());
    assert!(text(&o).contains("needs a terminal"), "{}", text(&o));
    assert!(took < Duration::from_secs(5));
    assert!(std::fs::read_dir(d.path()).unwrap().next().is_none());
}

#[test]
fn list_and_remove_on_an_empty_tree_are_calm_and_idempotent() {
    let d = tempfile::tempdir().unwrap();
    let (o, _) = ow(d.path(), &["list"]);
    assert!(
        o.status.success() && text(&o).contains("No widgets installed"),
        "{}",
        text(&o)
    );
    assert!(
        std::fs::read_dir(d.path()).unwrap().next().is_none(),
        "list must not create anything"
    );
    for _ in 0..2 {
        let (o, _) = ow(d.path(), &["remove", "io.github.dexterhere.clock"]);
        assert!(
            o.status.success() && text(&o).contains("not installed"),
            "{}",
            text(&o)
        );
    }
}

/// The registry origin is a compiled-in constant plus one flag. Nothing in the install code may
/// read the environment, spawn a program, or name another host.
#[test]
fn install_code_has_no_environment_process_or_extra_host() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/install");
    let mut seen = 0;
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let src = std::fs::read_to_string(&p).unwrap();
        // Strip test modules: they may mention these words in assertions.
        let code = src.split("#[cfg(test)]").next().unwrap();
        for banned in [
            "std::env",
            "env::var",
            "env::args",
            "Command",
            "process::Command",
            concat!("Tcp", "Stream"),
            "http://",
        ] {
            assert!(!code.contains(banned), "{} mentions {banned}", p.display());
        }
        let hosts =
            code.matches("raw.githubusercontent.com").count() + code.matches("vercel.app").count();
        assert!(hosts <= 2, "{}: unexpected host literals", p.display());
        seen += 1;
    }
    assert!(seen >= 10);
    let m = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/install/mod.rs"),
    )
    .unwrap();
    assert_eq!(m.matches("vercel.app").count(), 1);
    assert_eq!(m.matches("raw.githubusercontent.com").count(), 1);
}
