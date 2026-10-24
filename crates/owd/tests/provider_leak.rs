//! AUTH_PLAN 5.2 item 4, provider half: run a daemon whose provider really resolves a FAKE vault
//! secret, drive a session, then search everything the process left behind for the fake
//! passphrase and the fake secret in raw, hex, base64 and percent-encoded form.
//!
//! `tests/leak.rs` runs the shipped `owd` binary, which has no secret-backed provider yet. Here
//! the test binary starts itself as the daemon (child mode, same pattern as the egress proxy
//! test), so the real server loop, the real `SecretHandle` path and the real process hardening
//! run with a provider that holds the secret in memory while it works.
//! Mutation check: make `LeakProvider::fetch` print the secret and this test fails.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use ow_core::{FetchCtx, Paths, Provider, ProviderError, Sensitivity, TopicSpec};
use ow_vault::{Profile, Vault};
use owd::Server;
use serde_json::{Map, Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

mod common;
use common::{b64, drain, files, spellings};

const CHILD_ENV: &str = "OWD_PROVIDER_LEAK_CHILD";
const PASS: &str = "FAKE-passphrase-correct-horse";
const WRONG: &str = "FAKE-passphrase-wrong-horse!";
const TOKEN: &str = "FAKE-TOKEN-VALUE-77c1e";
const GRANTS: &str =
    "[[grant]]\nwidget=\"w1\"\ntopics=[\"fake.secret\"]\nfields=[\"len\",\"same\"]\n";

struct LeakProvider;

static TOPICS: [TopicSpec; 1] = [TopicSpec::new(
    "fake.secret",
    Sensitivity::High,
    Duration::from_millis(50),
    &["len", "same"],
)];

impl Provider for LeakProvider {
    fn id(&self) -> &'static str {
        "leak"
    }
    fn topics(&self) -> &'static [TopicSpec] {
        &TOPICS
    }
    fn secrets(&self) -> &'static [&'static str] {
        &["fake-token"]
    }
    fn fetch(
        &self,
        _t: &str,
        _p: &Map<String, Value>,
        ctx: &FetchCtx<'_>,
    ) -> Result<Map<String, Value>, ProviderError> {
        // The provider really sees the value (so it sits in this process while it works) and
        // reports only facts about it.
        let (len, same) = ctx.with_secret("fake-token", |s| (s.len(), s == TOKEN))?;
        Ok(json!({"len": len, "same": same})
            .as_object()
            .cloned()
            .unwrap())
    }
}

/// Only does anything when started by the test below: becomes the daemon.
#[test]
fn child_daemon() {
    let Ok(dir) = std::env::var(CHILD_ENV) else {
        return;
    };
    ow_core::harden_process().unwrap();
    let paths = Paths::from_config_dir(&dir).unwrap();
    let server = Server::bind(&paths, vec![Arc::new(LeakProvider)]).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop)).unwrap();
    eprintln!("child daemon listening");
    server.run(&stop, &AtomicBool::new(false));
}

fn ask(paths: &Paths, line: &str) -> Value {
    let mut s = UnixStream::connect(paths.socket()).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(line.as_bytes()).unwrap();
    let mut out = String::new();
    BufReader::new(s).read_line(&mut out).unwrap();
    serde_json::from_str(&out).unwrap()
}

#[test]
fn a_secret_backed_provider_never_leaks_the_fake_secret_or_passphrase() {
    if std::env::var(CHILD_ENV).is_ok() {
        return; // inside the child: only `child_daemon` runs
    }
    let dir = tempfile::Builder::new()
        .prefix("owd.")
        .tempdir_in("/tmp")
        .unwrap();
    let paths = Paths::under(dir.path());
    std::fs::create_dir_all(&paths.config).unwrap();
    std::fs::write(paths.grants(), GRANTS).unwrap();
    std::fs::set_permissions(paths.grants(), std::fs::Permissions::from_mode(0o600)).unwrap();
    ow_core::private_dir(&paths.data).unwrap();
    let mut v = Vault::create(&paths.vault(), PASS.as_bytes(), Profile::Standard).unwrap();
    v.set("fake-token", TOKEN).unwrap();
    drop(v);

    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_daemon", "--nocapture", "--test-threads=1"])
        .env_clear()
        .env(CHILD_ENV, dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = drain(child.stdout.take().unwrap());
    let err = drain(child.stderr.take().unwrap());
    for _ in 0..200 {
        if String::from_utf8_lossy(&err.lock().unwrap()).contains("child daemon listening") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let snap = || {
        ask(
            &paths,
            "{\"v\":0,\"id\":4,\"op\":\"snapshot\",\"widget\":\"w1\",\"topic\":\"fake.secret\"}\n",
        )
    };
    let unlock = |p: &str| ask(&paths, &ow_proto::unlock_request(1, p.as_bytes()).unwrap());
    // Locked: no data, a category only.
    assert_eq!(snap()["error"]["code"], "locked");
    assert_eq!(unlock(WRONG)["error"]["code"], "denied");
    assert_eq!(unlock(PASS)["state"], "unlocked");
    // The provider used the secret, and the widget got facts about it, not the value.
    let s = snap();
    assert_eq!(s["ok"], true, "{s}");
    assert_eq!(s["data"]["len"], TOKEN.len());
    assert_eq!(s["data"]["same"], true);
    assert_eq!(
        ask(&paths, "{\"v\":0,\"id\":5,\"op\":\"vault.lock\"}\n")["ok"],
        true
    );
    assert_eq!(snap()["error"]["code"], "locked");
    assert_eq!(unlock(PASS)["state"], "unlocked"); // left unlocked: the secret is resident now
    assert_eq!(snap()["data"]["same"], true);

    // Inspect while the secret is resident in the daemon.
    let pid = child.id();
    let mut proc_text = Vec::new();
    for f in ["cmdline", "environ", "comm", "status"] {
        // environ is root-owned for a non-dumpable process: unreadable is expected.
        if let Ok(b) = std::fs::read(format!("/proc/{pid}/{f}")) {
            proc_text.extend_from_slice(&b);
        }
    }
    assert!(
        String::from_utf8_lossy(&proc_text).contains("provider_leak"),
        "cmdline should be readable"
    );
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(pid as i32).unwrap(),
        rustix::process::Signal::TERM,
    )
    .unwrap();
    child.wait().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let stdout = out.lock().unwrap().clone();
    let stderr = err.lock().unwrap().clone();

    let mut all: Vec<(String, Vec<u8>)> = vec![
        ("child stdout".into(), stdout),
        ("child stderr".into(), stderr),
        ("/proc/<pid>".into(), proc_text),
    ];
    let mut fs = Vec::new();
    files(dir.path(), &mut fs);
    assert!(fs.iter().any(|f| f.ends_with("audit.jsonl")));
    for f in fs {
        all.push((f.display().to_string(), std::fs::read(&f).unwrap()));
    }
    let mut searched = 0;
    for (name, bytes) in &all {
        for needle in [PASS, WRONG, TOKEN] {
            for (form, pat) in spellings(needle) {
                searched += 1;
                assert!(
                    !bytes.windows(pat.len()).any(|w| w == pat.as_slice()),
                    "{needle} ({form}) found in {name}"
                );
            }
        }
    }
    assert!(searched > 100);
}

#[test]
fn the_search_itself_finds_what_it_should() {
    // Control: each spelling is really produced and really found inside a larger buffer, so a
    // broken encoder cannot make the leak test pass by never matching.
    for needle in [PASS, TOKEN] {
        for (form, pat) in spellings(needle) {
            assert!(pat.len() >= 10, "{form}");
            let mut hay = vec![b'#'; 7];
            hay.extend_from_slice(&pat);
            hay.extend_from_slice(b"==");
            assert!(
                hay.windows(pat.len()).any(|w| w == pat.as_slice()),
                "{form}"
            );
        }
    }
    assert_eq!(b64(b"Man"), "TWFu");
    assert_eq!(b64(b"Ma"), "TWE");
    // Stream alignment: the base64 of two bytes + needle contains the spelling for shift 2.
    let full = b64(format!("ab{TOKEN}cd").as_bytes());
    let (_, trimmed) = &spellings(TOKEN)[8]; // base64+2
    let t = String::from_utf8(trimmed.clone()).unwrap();
    assert!(full.contains(&t), "{full} !~ {t}");
}
