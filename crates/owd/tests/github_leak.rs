//! AUTH_PLAN 5.2 items 4 and 7 for the `github` provider: a full session through the real daemon
//! code (config.toml, vault unlock, hub, provider) with a FAKE token and a fake `Http`. No
//! network socket is involved anywhere: the transport is a script, so nothing here can reach
//! api.github.com.
//!
//! Afterwards everything the daemon left behind is searched for the fake token and the fake
//! passphrases in raw, hex, base64 and percent-encoded form: stdout, stderr, every file in the
//! tree (audit log, vault ciphertext, config), `/proc/<pid>/cmdline`/`environ`, every reply the
//! client received, and the `Display`/`Debug` text of every error the provider can produce.
//!
//! The fake transport also checks, inside the daemon, that the token arrives only as the
//! credential argument and never in the host, path, body or any request header. A violation makes
//! it answer `Blocked`, which the session would then report as the wrong state.
//!
//! Mutation check (documented in docs/DECISIONS.md GITHUB-PROVIDER): make the provider print the
//! token inside the `with_secret` closure and `a_full_session_never_leaks_...` fails.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use ow_core::{
    Bearer, Config, FetchCtx, Http, HttpError, HttpRequest, HttpResponse, Paths, Provider,
    StatusClass,
};
use ow_vault::{Profile, Vault};
use owd::Server;
use serde_json::{Map, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod common;
use common::{drain, files, spellings};

const CHILD_ENV: &str = "OWD_GITHUB_LEAK_CHILD";
const PASS: &str = "FAKE-passphrase-correct-horse";
const WRONG: &str = "FAKE-passphrase-wrong-horse!";
const TOKEN: &str = "FAKE-pat-0123456789abcdef";
const CONFIG: &str = "[providers.github]\nsecret = \"github-main\"\n";
const GRANTS: &str =
    "[[grant]]\nwidget=\"w1\"\ntopics=[\"github.prs\"]\nfields=[\"open\",\"review\"]\n";
const GOOD: &str = r#"{"data":{"open":{"issueCount":4},"review":{"issueCount":2}}}"#;
const EXPIRY: &str = "github-authentication-token-expiration";

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// `YYYY-MM-DD HH:MM:SS UTC` for an epoch second (civil-from-days, no date crate in the test).
fn utc(secs: u64) -> String {
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn resp(body: &str, headers: &[(&str, String)]) -> Result<HttpResponse, HttpError> {
    Ok(HttpResponse {
        status: 200,
        headers: headers
            .iter()
            .map(|(n, v)| ((*n).to_owned(), v.clone()))
            .collect(),
        body: body.as_bytes().to_vec(),
    })
}

fn status(code: u16, headers: &[(&str, String)]) -> Result<HttpResponse, HttpError> {
    Err(HttpError::Status {
        class: if code >= 500 {
            StatusClass::Server
        } else {
            StatusClass::Client
        },
        code,
        headers: headers
            .iter()
            .map(|(n, v)| ((*n).to_owned(), v.clone()))
            .collect(),
    })
}

/// The scripted upstream. Hostile on purpose: error bodies and headers repeat the token, so a
/// provider that forwarded any upstream text would leak it into replies.
struct Script {
    calls: AtomicUsize,
}

impl Script {
    fn answer(n: usize) -> Result<HttpResponse, HttpError> {
        let evil = ("x-evil", TOKEN.to_owned());
        match n {
            0 => resp(GOOD, &[(EXPIRY, utc(now() + 3 * 86_400))]),
            1 => status(401, &[evil]),
            2 => status(403, &[evil]),
            3 => status(
                429,
                &[("x-ratelimit-reset", (now() + 300).to_string()), evil],
            ),
            4 => Err(HttpError::Offline),
            5 => resp(
                &format!(r#"{{"errors":[{{"message":"bad {TOKEN}"}}]}}"#),
                &[evil],
            ),
            6 => resp(&format!("<html>{TOKEN}</html>"), &[]),
            7 => status(500, &[evil]),
            _ => resp(GOOD, &[]),
        }
    }
}

impl Http for Script {
    fn post_json(&self, r: &HttpRequest<'_>, c: &Bearer<'_>) -> Result<HttpResponse, HttpError> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        let elsewhere = [r.host, r.path, &r.body.to_string()]
            .iter()
            .any(|s| s.contains(TOKEN))
            || r.headers
                .iter()
                .any(|(h, v)| h.contains(TOKEN) || v.contains(TOKEN));
        if c.expose() != TOKEN || elsewhere || r.host != "api.github.com" {
            return Err(HttpError::Blocked);
        }
        Self::answer(n)
    }
}

/// Build the providers exactly as `owd`'s main does, with the script as transport.
fn daemon_providers(paths: &Paths, http: Arc<dyn Http>) -> Vec<Arc<dyn Provider>> {
    let config = Config::load(&paths.config_file()).unwrap();
    owd::providers(&config, |hosts| {
        assert_eq!(hosts, ["api.github.com"]);
        Ok(Arc::clone(&http))
    })
    .unwrap()
}

/// Only does anything when started by the test below: becomes the daemon.
#[test]
fn child_daemon() {
    let Ok(dir) = std::env::var(CHILD_ENV) else {
        return;
    };
    ow_core::harden_process().unwrap();
    let paths = Paths::from_config_dir(&dir).unwrap();
    let http = Arc::new(Script {
        calls: AtomicUsize::new(0),
    });
    let server = Server::bind(&paths, daemon_providers(&paths, http)).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop)).unwrap();
    eprintln!("child daemon listening");
    server.run(&stop, &AtomicBool::new(false));
}

/// Held while a vault file is open for writing and while the child daemon is forked. A fork in
/// one test thread copies the vault lock descriptor another thread has open, and the copy keeps
/// the flock alive until the child execs, so the other thread's next lock failed with `Busy`.
static FORK: Mutex<()> = Mutex::new(());

fn tree(vault_secret: Option<&str>) -> (tempfile::TempDir, Paths) {
    let _fork = FORK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::Builder::new()
        .prefix("owd.")
        .tempdir_in("/tmp")
        .unwrap();
    let paths = Paths::under(dir.path());
    std::fs::create_dir_all(&paths.config).unwrap();
    for (p, text) in [(paths.grants(), GRANTS), (paths.config_file(), CONFIG)] {
        std::fs::write(&p, text).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    ow_core::private_dir(&paths.data).unwrap();
    let mut v = Vault::create(&paths.vault(), PASS.as_bytes(), Profile::Standard).unwrap();
    if let Some(name) = vault_secret {
        v.set(name, TOKEN).unwrap();
    }
    drop(v);
    (dir, paths)
}

fn ask(paths: &Paths, line: &str) -> (String, Value) {
    let mut s = UnixStream::connect(paths.socket()).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(line.as_bytes()).unwrap();
    let mut out = String::new();
    BufReader::new(s).read_line(&mut out).unwrap();
    let v = serde_json::from_str(&out).unwrap();
    (out, v)
}

fn snap(paths: &Paths) -> (String, Value) {
    ask(
        paths,
        "{\"v\":0,\"id\":4,\"op\":\"snapshot\",\"widget\":\"w1\",\"topic\":\"github.prs\"}\n",
    )
}

fn unlock(paths: &Paths, pass: &str) -> Value {
    ask(
        paths,
        &ow_proto::unlock_request(1, pass.as_bytes()).unwrap(),
    )
    .1
}

fn lock(paths: &Paths) {
    ask(paths, "{\"v\":0,\"id\":5,\"op\":\"vault.lock\"}\n");
}

fn state(v: &Value) -> Option<ow_proto::StateInfo> {
    ow_proto::StateInfo::from_fields(v.get("error").unwrap_or(v).as_object().unwrap())
}

fn expect(v: &Value, want: ow_proto::State) -> ow_proto::StateInfo {
    let info = state(v).unwrap_or_else(|| panic!("no state in {v}"));
    assert_eq!(info.state, want, "{v}");
    info
}

#[test]
fn a_full_session_never_leaks_the_fake_token_or_passphrase() {
    use ow_proto::State as S;
    if std::env::var(CHILD_ENV).is_ok() {
        return; // inside the child: only `child_daemon` runs
    }
    let (dir, paths) = tree(Some("github-main"));
    let mut child = {
        let _fork = FORK.lock().unwrap_or_else(|e| e.into_inner());
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "child_daemon", "--nocapture", "--test-threads=1"])
            .env_clear()
            .env(CHILD_ENV, dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let out = drain(child.stdout.take().unwrap());
    let err = drain(child.stderr.take().unwrap());
    for _ in 0..200 {
        if String::from_utf8_lossy(&err.lock().unwrap()).contains("child daemon listening") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut replies = String::new();

    // Locked first: a category, no data, nothing requested upstream.
    let (raw, v) = snap(&paths);
    replies += &raw;
    assert_eq!(v["error"]["code"], "locked", "{v}");
    expect(&v, S::Locked);
    let wrong = unlock(&paths, WRONG);
    assert_eq!(wrong["error"]["code"], "denied");

    // Each step: lock and unlock again (which drops the cache, so the next snapshot fetches),
    // then read what the scripted upstream's step N produced.
    let steps: [(&str, Option<S>); 9] = [
        ("expiring", Some(S::ExpiredSoon)),
        ("401", Some(S::Unauthorized)),
        ("403", Some(S::Insufficient)),
        ("429", Some(S::RateLimited)),
        ("offline", Some(S::Offline)),
        ("graphql error", Some(S::Unavailable)),
        ("not json", Some(S::Unavailable)),
        ("500", Some(S::Unavailable)),
        ("healthy", None),
    ];
    for (i, (name, want)) in steps.iter().enumerate() {
        lock(&paths);
        assert_eq!(unlock(&paths, PASS)["state"], "unlocked");
        let (raw, v) = snap(&paths);
        replies += &raw;
        match want {
            Some(w) => {
                let info = expect(&v, *w);
                if *w == S::RateLimited {
                    let until = info.until.expect("reset time");
                    assert!(until > now() && until <= now() + 301, "{name}: {v}");
                }
                if *w == S::ExpiredSoon {
                    // data still served, fresh, with the expiry as a number
                    assert_eq!(
                        (v["data"]["open"].as_u64(), v["data"]["review"].as_u64()),
                        (Some(4), Some(2)),
                        "{v}"
                    );
                    assert_eq!(v["stale"], false);
                    assert!(info.until.is_some());
                } else {
                    assert_eq!(v["error"]["code"], "provider_error", "step {i} {name}: {v}");
                    assert!(v.get("data").is_none());
                }
            }
            None => {
                assert_eq!(
                    (v["data"]["open"].as_u64(), v["data"]["review"].as_u64()),
                    (Some(4), Some(2)),
                    "{v}"
                );
                assert!(state(&v).is_none(), "{v}");
                // only the two granted numbers, nothing else
                assert_eq!(v["data"].as_object().unwrap().len(), 2);
            }
        }
    }
    // Resident and unlocked: inspect the live process.
    let pid = child.id();
    let mut proc_text = Vec::new();
    for f in ["cmdline", "environ", "comm", "status"] {
        // environ is root-owned for a non-dumpable process: unreadable is expected.
        if let Ok(b) = std::fs::read(format!("/proc/{pid}/{f}")) {
            proc_text.extend_from_slice(&b);
        }
    }
    assert!(
        String::from_utf8_lossy(&proc_text).contains("github_leak"),
        "cmdline should be readable"
    );
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(pid as i32).unwrap(),
        rustix::process::Signal::TERM,
    )
    .unwrap();
    child.wait().unwrap();
    std::thread::sleep(Duration::from_millis(100));

    let mut all: Vec<(String, Vec<u8>)> = vec![
        ("child stdout".into(), out.lock().unwrap().clone()),
        ("child stderr".into(), err.lock().unwrap().clone()),
        ("/proc/<pid>".into(), proc_text),
        ("replies".into(), replies.into_bytes()),
        ("error text".into(), error_text().into_bytes()),
    ];
    let mut fs = Vec::new();
    files(dir.path(), &mut fs);
    assert!(fs.iter().any(|f| f.ends_with("audit.jsonl")));
    assert!(fs.iter().any(|f| f.ends_with("config.toml")));
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
    assert!(searched > 200);
}

/// Display and Debug of everything the provider can return, under the hostile script. Run
/// in-process; the provider is the real one.
fn error_text() -> String {
    let (_dir, paths) = tree(Some("github-main"));
    let cfg = Config::load(&paths.config_file()).unwrap();
    let mut text = format!("{cfg:?} {:?}", cfg.github());
    let src = Src;
    for n in 0..9 {
        struct One(usize);
        impl Http for One {
            fn post_json(
                &self,
                _: &HttpRequest<'_>,
                c: &Bearer<'_>,
            ) -> Result<HttpResponse, HttpError> {
                text_guard(c);
                Script::answer(self.0)
            }
        }
        let http: Arc<dyn Http> = Arc::new(One(n));
        let p = daemon_providers(&paths, http);
        let gh = p.iter().find(|p| p.id() == "github").unwrap();
        let ctx = FetchCtx::new(gh.secrets(), &src);
        let r = gh.fetch("github.prs", &Map::new(), &ctx);
        text += &format!(" {r:?} {:?}", ctx.take_notice());
        if let Err(e) = r {
            text += &format!(" {e} {e:?}");
        }
        text += &format!(" {:?}", ctx.secret("github-main").map(|h| format!("{h:?}")));
    }
    text
}

fn text_guard(c: &Bearer<'_>) {
    assert_eq!(c.expose(), TOKEN);
    assert!(!format!("{c:?}").contains("FAKE"));
}

struct Src;
impl ow_core::SecretSource for Src {
    fn with_secret(
        &self,
        _name: &str,
        f: &mut dyn FnMut(&str),
    ) -> Result<(), ow_core::ProviderError> {
        f(TOKEN);
        Ok(())
    }
}

/// A started in-process daemon with the scripted upstream; used for the cases that need no
/// process inspection.
struct H {
    paths: Paths,
    http: Arc<Script>,
    stop: Arc<AtomicBool>,
    th: Option<std::thread::JoinHandle<()>>,
    _dir: tempfile::TempDir,
}

fn start(vault_secret: Option<&str>, config: Option<&str>) -> H {
    let (dir, paths) = tree(vault_secret);
    match config {
        Some(c) => {
            std::fs::write(paths.config_file(), c).unwrap();
        }
        None => std::fs::remove_file(paths.config_file()).unwrap(),
    }
    let http = Arc::new(Script {
        calls: AtomicUsize::new(0),
    });
    let server = Server::bind(&paths, daemon_providers(&paths, http.clone())).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (s2, r2) = (Arc::clone(&stop), Arc::new(AtomicBool::new(false)));
    let th = std::thread::spawn(move || server.run(&s2, &r2));
    H {
        paths,
        http,
        stop,
        th: Some(th),
        _dir: dir,
    }
}

impl Drop for H {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.th.take() {
            let _ = t.join();
        }
    }
}

#[test]
fn a_bound_name_missing_from_the_vault_is_no_credential_and_asks_nothing() {
    let h = start(Some("github-other"), Some(CONFIG));
    assert_eq!(unlock(&h.paths, PASS)["state"], "unlocked");
    let (_, v) = snap(&h.paths);
    assert_eq!(v["error"]["code"], "provider_error", "{v}");
    expect(&v, ow_proto::State::NoCredential);
    assert_eq!(
        h.http.calls.load(Ordering::SeqCst),
        0,
        "no request without a secret"
    );
}

#[test]
fn without_a_binding_the_topic_is_no_credential_and_nothing_is_requested() {
    let h = start(Some("github-main"), None);
    assert_eq!(unlock(&h.paths, PASS)["state"], "unlocked");
    let (_, v) = snap(&h.paths);
    assert_eq!(v["error"]["code"], "provider_error", "{v}");
    expect(&v, ow_proto::State::NoCredential);
    assert_eq!(h.http.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn removing_the_secret_and_unlocking_again_stops_serving_the_cached_counts() {
    // AUTH_PLAN 5.2 item 8: after the secret is gone from the vault, a fresh unlock purges the
    // cache and the topic reports no_credential instead of the old counts.
    let h = start(Some("github-main"), Some(CONFIG));
    assert_eq!(unlock(&h.paths, PASS)["state"], "unlocked");
    assert_eq!(snap(&h.paths).1["data"]["open"], 4);
    {
        let _fork = FORK.lock().unwrap_or_else(|e| e.into_inner());
        let mut v = Vault::open(&h.paths.vault(), PASS.as_bytes()).unwrap();
        v.remove("github-main").unwrap();
    }
    // Still unlocked and cached until the lock (`ow vault remove` locks the daemon afterwards).
    lock(&h.paths);
    assert_eq!(unlock(&h.paths, PASS)["state"], "unlocked");
    let (_, v) = snap(&h.paths);
    expect(&v, ow_proto::State::NoCredential);
    assert!(v.get("data").is_none());
    assert_eq!(h.http.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn the_token_is_never_a_request_part_other_than_the_credential() {
    // The Script answers `Blocked` (-> unavailable) if the token shows up anywhere but the
    // credential argument, so a healthy first answer proves it did not.
    let h = start(Some("github-main"), Some(CONFIG));
    assert_eq!(unlock(&h.paths, PASS)["state"], "unlocked");
    let (_, v) = snap(&h.paths);
    assert_eq!(v["data"]["open"], 4, "{v}");
    assert_eq!(v["data"]["review"], 2);
}
