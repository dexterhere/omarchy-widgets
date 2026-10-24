//! `ow connect github` against throwaway directories. Fake values only; no network (the one
//! HTTP seam the daemon has is a fake here), no systemctl, nothing outside the temp dirs.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "../../owd/tests/common/mod.rs"]
mod leakcommon;

use ow_core::{Bearer, Config, Http, HttpError, HttpRequest, HttpResponse, Paths};
use ow_vault::Vault;
use std::io::{Read, Write};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Children are spawned while another test may hold a vault lock; run the tests one at a time.
static SERIAL: Mutex<()> = Mutex::new(());
fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

const PASS: &str = "FAKE-passphrase-correct-horse";
const TOKEN: &str = "github_pat_FAKE0Z9y8x7w6v5u4t3s2r1q0p_ZZZ";

fn cmd(dir: &Path, args: &[&str]) -> Command {
    // New session: no controlling terminal, so a prompt can never reach the developer's tty.
    let mut c = Command::new("setsid");
    c.arg("-w")
        .arg(env!("CARGO_BIN_EXE_ow"))
        .arg("--config-dir")
        .arg(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    c
}

fn ow(dir: &Path, args: &[&str], stdin: &str) -> Output {
    let mut c = cmd(dir, args).spawn().unwrap();
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

fn init_vault(d: &Path) {
    assert!(
        ow(d, &["vault", "init"], &format!("{PASS}\n"))
            .status
            .success()
    );
}

fn connect(d: &Path, extra: &[&str], token_line: &str) -> Output {
    let mut a = vec!["connect", "github", "--passphrase-stdin"];
    a.extend_from_slice(extra);
    ow(d, &a, &format!("{PASS}\n{token_line}\n"))
}

fn stored(d: &Path, name: &str) -> Option<String> {
    let v = Vault::open(&Paths::under(d).vault(), PASS.as_bytes()).unwrap();
    v.get(name).map(str::to_owned)
}

fn walk(p: &Path) -> Vec<PathBuf> {
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

/// Every place and spelling where any needle shows up. The detector the leak tests rely on.
fn leaks(places: &[(String, Vec<u8>)], needles: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for n in needles {
        for (kind, pat) in leakcommon::spellings(n) {
            for (place, hay) in places {
                if hay.windows(pat.len()).any(|w| w == pat.as_slice()) {
                    found.push(format!("{place}: {kind}"));
                }
            }
        }
    }
    found
}

fn output_places(o: &Output) -> Vec<(String, Vec<u8>)> {
    vec![
        ("stdout".into(), o.stdout.clone()),
        ("stderr".into(), o.stderr.clone()),
    ]
}

fn file_places(d: &Path) -> Vec<(String, Vec<u8>)> {
    walk(d)
        .into_iter()
        // The daemon's socket is not a regular file.
        .filter_map(|f| Some((f.display().to_string(), std::fs::read(&f).ok()?)))
        .collect()
}

#[test]
fn the_detector_sees_every_spelling_so_a_printing_mutation_would_fail_the_tests() {
    let b = TOKEN.as_bytes();
    let b64 = leakcommon::b64(b);
    let spelled = [
        TOKEN.to_owned(),
        leakcommon::hex(b),
        leakcommon::hex(b).to_uppercase(),
        leakcommon::pct(b),
        b64.trim_end_matches('=').to_owned(),
        format!("log line: token={TOKEN} end"),
    ];
    for s in spelled {
        let fake_output = Output {
            status: std::process::ExitStatus::default(),
            stdout: s.clone().into_bytes(),
            stderr: Vec::new(),
        };
        assert!(
            !leaks(&output_places(&fake_output), &[TOKEN]).is_empty(),
            "detector missed {s}"
        );
    }
    assert!(leaks(&[("x".into(), b"clean output".to_vec())], &[TOKEN]).is_empty());
}

#[test]
fn full_run_stores_binds_and_leaks_nothing() {
    let _s = serial();
    let d = tempfile::tempdir().unwrap();
    init_vault(d.path());

    // While it waits for the token, look at the running processes' argv and environment.
    let mut child = cmd(d.path(), &["connect", "github", "--passphrase-stdin"])
        .env("OW_TEST_MARKER", "1")
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(format!("{PASS}\n").as_bytes()).unwrap();
    std::thread::sleep(Duration::from_millis(800));
    let mut proc_text: Vec<(String, Vec<u8>)> = Vec::new();
    let me = d.path().to_str().unwrap().as_bytes().to_vec();
    for e in std::fs::read_dir("/proc").unwrap().flatten() {
        let p = e.path();
        let Ok(argv) = std::fs::read(p.join("cmdline")) else {
            continue;
        };
        if argv.windows(me.len()).any(|w| w == me.as_slice()) {
            proc_text.push((format!("{}/cmdline", p.display()), argv));
            if let Ok(env) = std::fs::read(p.join("environ")) {
                proc_text.push((format!("{}/environ", p.display()), env));
            }
        }
    }
    assert!(!proc_text.is_empty(), "never saw the running ow process");
    stdin.write_all(format!("{TOKEN}\n").as_bytes()).unwrap();
    drop(stdin);
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    let t = text(&o);

    // The guidance and the next steps are on screen, as text only.
    for s in [
        "https://github.com/settings/personal-access-tokens/new",
        "Pull requests: Read-only",
        "vault is unlocked",
        "systemctl --user restart owd.service",
        "ow vault unlock",
        "ow connect github --check",
        "ow does not restart",
    ] {
        assert!(
            t.replace("  ", " ").contains(&s.replace("  ", " ")),
            "missing {s:?} in {t}"
        );
    }
    // Stored, bound, private.
    assert_eq!(stored(d.path(), "github-main").as_deref(), Some(TOKEN));
    let paths = Paths::under(d.path());
    let md = std::fs::metadata(paths.config_file()).unwrap();
    assert_eq!(md.permissions().mode() & 0o777, 0o600);
    assert!(md.uid_is_ours());
    let cfg = Config::load(&paths.config_file()).unwrap();
    let g = cfg.github().unwrap();
    assert_eq!((g.secret(), g.interval().as_secs()), ("github-main", 120));
    assert!(
        walk(&paths.config).iter().all(|f| !f
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".tmp")),
        "temp file left behind"
    );

    // Nothing leaks, anywhere, in any spelling: output, every file (vault ciphertext included),
    // argv and environment of the running process.
    let mut places = output_places(&o);
    places.extend(file_places(d.path()));
    places.extend(proc_text);
    let found = leaks(&places, &[TOKEN, PASS]);
    assert!(found.is_empty(), "leaked: {found:?}");
    // Its length is not shown either.
    assert!(!t.contains(&TOKEN.len().to_string()), "{t}");
}

trait OwnedByUs {
    fn uid_is_ours(&self) -> bool;
}
impl OwnedByUs for std::fs::Metadata {
    fn uid_is_ours(&self) -> bool {
        use std::os::unix::fs::MetadataExt;
        self.uid() == rustix::process::geteuid().as_raw()
    }
}

#[test]
fn bad_tokens_are_refused_and_nothing_changes() {
    let _s = serial();
    let d = tempfile::tempdir().unwrap();
    init_vault(d.path());
    let long = "a".repeat(16 * 1024 + 1);
    for bad in [
        "".to_owned(),
        "github_pat_FAKE with space".into(),
        "github_pat_FAKE\u{e9}".into(),
        "github_pat_FAKE\x01ctl".into(),
        "ghp_FAKEclassic0123456789".into(),
        long,
    ] {
        let o = connect(d.path(), &[], &bad);
        assert!(!o.status.success(), "accepted {bad:?}");
        if !bad.is_empty() {
            let found = leaks(&output_places(&o), &[bad.as_str()]);
            assert!(found.is_empty(), "echoed a refused token: {found:?}");
        }
    }
    assert_eq!(stored(d.path(), "github-main"), None);
    assert!(!Paths::under(d.path()).config_file().exists());
}

#[test]
fn bad_names_and_options_are_refused_without_echo() {
    let _s = serial();
    let d = tempfile::tempdir().unwrap();
    for args in [
        vec!["connect", "github", "--secret-name", TOKEN],
        vec!["connect", "github", "--secret-name", "gmail-app-pw"],
        vec!["connect", "github", "--secret-name", "github-UPPER"],
        vec!["connect", "github", TOKEN],
        vec!["connect", "github", "--interval", TOKEN],
        vec!["connect", "gitlab"],
        vec!["connect", "github", "--check", "--remove"],
    ] {
        let o = ow(d.path(), &args, "");
        assert!(!o.status.success(), "{args:?}");
        let found = leaks(&output_places(&o), &[TOKEN]);
        assert!(found.is_empty(), "{args:?}: {found:?}");
    }
}

#[test]
fn no_vault_without_a_terminal_points_at_vault_init() {
    let _s = serial();
    let d = tempfile::tempdir().unwrap();
    let o = connect(d.path(), &[], TOKEN);
    assert!(!o.status.success());
    assert!(text(&o).contains("ow vault init"), "{}", text(&o));
    assert!(!Paths::under(d.path()).config_file().exists());
    assert!(!Paths::under(d.path()).vault().exists());
}

#[test]
fn wrong_passphrase_changes_nothing() {
    let _s = serial();
    let d = tempfile::tempdir().unwrap();
    init_vault(d.path());
    let o = ow(
        d.path(),
        &["connect", "github", "--passphrase-stdin"],
        &format!("FAKE-wrong-passphrase!!\n{TOKEN}\n"),
    );
    assert!(!o.status.success());
    assert!(!Paths::under(d.path()).config_file().exists());
    assert_eq!(stored(d.path(), "github-main"), None);
}

#[test]
fn a_config_the_loader_would_refuse_is_never_overwritten() {
    let _s = serial();
    let paths_of = |d: &Path| Paths::under(d);
    let setup = |content: &[u8], mode: u32| {
        let d = tempfile::tempdir().unwrap();
        init_vault(d.path());
        let p = paths_of(d.path());
        ow_core::private_dir(&p.config).unwrap();
        std::fs::write(p.config_file(), content).unwrap();
        std::fs::set_permissions(p.config_file(), std::fs::Permissions::from_mode(mode)).unwrap();
        d
    };
    let cases: [(&[u8], u32); 4] = [
        (b"this is [not toml", 0o600),
        (b"[providers.gmail]\nsecret = \"gmail-x\"\n", 0o600),
        (b"[providers.github]\nsecret = \"github-ok\"\n", 0o644),
        (
            b"[providers.github]\nsecret = \"github-ok\"\nextra = 1\n",
            0o600,
        ),
    ];
    for (content, mode) in cases {
        let d = setup(content, mode);
        let o = connect(d.path(), &[], TOKEN);
        assert!(!o.status.success(), "{}", text(&o));
        assert!(text(&o).contains("will not overwrite"), "{}", text(&o));
        assert_eq!(
            std::fs::read(paths_of(d.path()).config_file()).unwrap(),
            content
        );
        assert_eq!(
            stored(d.path(), "github-main"),
            None,
            "token stored despite refusal"
        );
    }
    // A symlink is not followed, read or replaced.
    let d = setup(b"", 0o600);
    let p = paths_of(d.path());
    let real = d.path().join("real.toml");
    std::fs::write(&real, "[providers.github]\nsecret = \"github-ok\"\n").unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::remove_file(p.config_file()).unwrap();
    symlink(&real, p.config_file()).unwrap();
    let o = connect(d.path(), &[], TOKEN);
    assert!(!o.status.success());
    assert!(
        std::fs::symlink_metadata(p.config_file())
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read_to_string(&real).unwrap(),
        "[providers.github]\nsecret = \"github-ok\"\n"
    );
}

#[test]
fn an_existing_binding_is_updated_in_place_and_the_interval_is_clamped() {
    let _s = serial();
    let d = tempfile::tempdir().unwrap();
    init_vault(d.path());
    let p = Paths::under(d.path());
    ow_core::private_dir(&p.config).unwrap();
    std::fs::write(
        p.config_file(),
        "# my note\n[providers.github]\nsecret = \"github-old\"\ninterval_secs = 600\n",
    )
    .unwrap();
    std::fs::set_permissions(p.config_file(), std::fs::Permissions::from_mode(0o600)).unwrap();
    // Keeps the existing interval when none is given.
    let o = connect(d.path(), &["--secret-name", "github-new"], TOKEN);
    assert!(o.status.success(), "{}", text(&o));
    let c = std::fs::read_to_string(p.config_file()).unwrap();
    assert!(
        c.starts_with("# my note\n") && !c.contains("github-old"),
        "{c}"
    );
    let g = Config::load(&p.config_file()).unwrap();
    assert_eq!(g.github().unwrap().secret(), "github-new");
    assert_eq!(g.github().unwrap().interval().as_secs(), 600);
    assert!(text(&o).contains("another secret name"));
    assert_eq!(stored(d.path(), "github-new").as_deref(), Some(TOKEN));
    // Out-of-range intervals are clamped and said so.
    for (given, want) in [("5", 60), ("99999", 3600)] {
        let o = connect(d.path(), &["--interval", given], TOKEN);
        assert!(o.status.success(), "{}", text(&o));
        assert!(
            text(&o).contains(&format!("using {want} s")),
            "{}",
            text(&o)
        );
        let g = Config::load(&p.config_file()).unwrap();
        assert_eq!(g.github().unwrap().interval().as_secs(), want);
    }
}

#[test]
fn remove_needs_consent_then_removes_binding_and_secret() {
    let _s = serial();
    let d = tempfile::tempdir().unwrap();
    init_vault(d.path());
    assert!(connect(d.path(), &[], TOKEN).status.success());
    // No terminal and no --yes: refused, nothing changed.
    let o = ow(
        d.path(),
        &["connect", "github", "--remove", "--passphrase-stdin"],
        &format!("{PASS}\n"),
    );
    assert!(
        !o.status.success() && text(&o).contains("--yes"),
        "{}",
        text(&o)
    );
    assert!(stored(d.path(), "github-main").is_some());
    assert!(
        Config::load(&Paths::under(d.path()).config_file())
            .unwrap()
            .github()
            .is_some()
    );
    // With --yes (the same escape hatch `ow remove` has).
    let o = ow(
        d.path(),
        &[
            "connect",
            "github",
            "--remove",
            "--yes",
            "--passphrase-stdin",
        ],
        &format!("{PASS}\n"),
    );
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(stored(d.path(), "github-main"), None);
    assert!(
        Config::load(&Paths::under(d.path()).config_file())
            .unwrap()
            .github()
            .is_none()
    );
    assert!(text(&o).contains("systemctl --user restart owd.service"));
    assert!(leaks(&output_places(&o), &[TOKEN, PASS]).is_empty());
}

// ---- --check, against a real in-process daemon whose HTTP seam is a fake ----

struct FakeGithub {
    seen: Mutex<Vec<bool>>,
}
impl Http for FakeGithub {
    fn post_json(&self, _: &HttpRequest<'_>, b: &Bearer<'_>) -> Result<HttpResponse, HttpError> {
        self.seen.lock().unwrap().push(b.expose() == TOKEN);
        Ok(HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: br#"{"data":{"open":{"issueCount":4},"review":{"issueCount":2}}}"#.to_vec(),
        })
    }
}

#[test]
fn check_asks_the_running_daemon_and_prints_only_category_and_counts() {
    let _s = serial();
    let d = tempfile::tempdir().unwrap();
    let paths = Paths::under(d.path());
    init_vault(d.path());
    // Nothing to ask with yet: no widget holds a grant.
    let o = ow(d.path(), &["connect", "github", "--check"], "");
    assert!(
        !o.status.success() && text(&o).contains("holds a grant"),
        "{}",
        text(&o)
    );

    ow_core::private_dir(&paths.config).unwrap();
    std::fs::write(
        paths.grants(),
        "[[grant]]\nwidget = \"io.github.fake.prs\"\ntopics = [\"github.prs\"]\napproved_at = \"2026-10-24T00:00:00Z\"\n",
    )
    .unwrap();
    std::fs::set_permissions(paths.grants(), std::fs::Permissions::from_mode(0o600)).unwrap();
    let o = ow(d.path(), &["connect", "github", "--check"], "");
    assert!(
        !o.status.success() && text(&o).contains("not reachable"),
        "{}",
        text(&o)
    );

    // Wizard first (no daemon yet), then the daemon starts from the file it wrote.
    assert!(connect(d.path(), &[], TOKEN).status.success());
    let fake = Arc::new(FakeGithub {
        seen: Mutex::new(Vec::new()),
    });
    let cfg = Config::load(&paths.config_file()).unwrap();
    let f2: Arc<dyn Http> = fake.clone();
    let providers = owd::providers(&cfg, move |_| Ok(Arc::clone(&f2))).unwrap();
    let server = owd::Server::bind(&paths, providers).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (s2, r2) = (Arc::clone(&stop), Arc::new(AtomicBool::new(false)));
    let th = std::thread::spawn(move || server.run(&s2, &r2));

    let locked = ow(d.path(), &["connect", "github", "--check"], "");
    assert_eq!(text(&locked).trim(), "github.prs: locked");
    assert!(
        ow(d.path(), &["vault", "unlock"], &format!("{PASS}\n"))
            .status
            .success()
    );
    let ok = ow(d.path(), &["connect", "github", "--check"], "");
    assert_eq!(text(&ok).trim(), "github.prs: ok open=4 review=2");
    // The token reached the (fake) transport, and only there.
    assert_eq!(fake.seen.lock().unwrap().as_slice(), &[true]);
    let mut places = output_places(&locked);
    places.extend(output_places(&ok));
    places.extend(file_places(d.path()));
    assert!(leaks(&places, &[TOKEN, PASS]).is_empty());

    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = std::os::unix::net::UnixStream::connect(paths.socket());
    let _ = th.join();
}

// ---- the real prompts, on a pseudo-terminal ----

struct Pty {
    master: std::fs::File,
    out: Arc<Mutex<Vec<u8>>>,
}

impl Pty {
    fn spawn(dir: &Path, args: &[&str]) -> (Pty, std::process::Child) {
        use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
        let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
        grantpt(&master).unwrap();
        unlockpt(&master).unwrap();
        let name = ptsname(&master, Vec::new()).unwrap();
        let slave = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(name.to_str().unwrap())
            .unwrap();
        let master = std::fs::File::from(master);
        let out = Arc::new(Mutex::new(Vec::new()));
        let (o2, mut r) = (Arc::clone(&out), master.try_clone().unwrap());
        std::thread::spawn(move || {
            let mut b = [0u8; 256];
            while let Ok(n) = r.read(&mut b) {
                if n == 0 {
                    break;
                }
                o2.lock().unwrap().extend_from_slice(&b[..n]);
            }
        });
        let child = Command::new(env!("CARGO_BIN_EXE_ow"))
            .arg("--config-dir")
            .arg(dir)
            .args(args)
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave))
            .spawn()
            .unwrap();
        (Pty { master, out }, child)
    }

    /// Wait until `needle` has appeared `n` times, then type `line`.
    fn expect_then(&mut self, needle: &str, n: usize, line: &str) {
        let t = Instant::now();
        loop {
            let seen = String::from_utf8_lossy(&self.out.lock().unwrap())
                .matches(needle)
                .count();
            if seen >= n {
                break;
            }
            assert!(
                t.elapsed() < Duration::from_secs(60),
                "never saw {needle:?} x{n}"
            );
            std::thread::sleep(Duration::from_millis(30));
        }
        std::thread::sleep(Duration::from_millis(100));
        self.master
            .write_all(format!("{line}\n").as_bytes())
            .unwrap();
    }

    fn screen(&self) -> Vec<u8> {
        self.out.lock().unwrap().clone()
    }
}

#[test]
fn terminal_run_creates_the_vault_hides_every_keystroke_and_leaks_nothing() {
    let _s = serial();
    let d = tempfile::tempdir().unwrap();
    let (mut pty, mut child) = Pty::spawn(d.path(), &["connect", "github"]);
    pty.expect_then("Type yes to create one now", 1, "yes");
    pty.expect_then("New vault passphrase", 1, PASS);
    pty.expect_then("Repeat: ", 1, PASS);
    pty.expect_then("Vault passphrase: ", 1, PASS);
    pty.expect_then("GitHub token: ", 1, TOKEN);
    pty.expect_then("Repeat: ", 2, TOKEN);
    assert!(
        child.wait().unwrap().success(),
        "{}",
        String::from_utf8_lossy(&pty.screen())
    );
    std::thread::sleep(Duration::from_millis(200));
    let screen = pty.screen();
    assert!(String::from_utf8_lossy(&screen).contains("systemctl --user restart owd.service"));
    // The terminal never echoed the token or the passphrase.
    let found = leaks(&[("pty".into(), screen)], &[TOKEN, PASS]);
    assert!(found.is_empty(), "echoed: {found:?}");
    assert_eq!(stored(d.path(), "github-main").as_deref(), Some(TOKEN));
    assert!(leaks(&file_places(d.path()), &[TOKEN, PASS]).is_empty());
}

#[test]
fn terminal_remove_stops_at_anything_but_yes() {
    let _s = serial();
    let d = tempfile::tempdir().unwrap();
    init_vault(d.path());
    assert!(connect(d.path(), &[], TOKEN).status.success());
    let (mut pty, mut child) = Pty::spawn(d.path(), &["connect", "github", "--remove"]);
    pty.expect_then("Type yes to remove them", 1, "y");
    let st = child.wait().unwrap();
    assert!(!st.success());
    assert!(String::from_utf8_lossy(&pty.screen()).contains("nothing was changed"));
    assert!(stored(d.path(), "github-main").is_some());
    assert!(
        Config::load(&Paths::under(d.path()).config_file())
            .unwrap()
            .github()
            .is_some()
    );
}
