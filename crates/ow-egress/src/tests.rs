//! One test per rule. Each would fail if the rule in the module docs of `lib.rs` were broken.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::target::{HOSTILE_HOSTS, LOOKALIKE_HOSTS};
use crate::testkit::{Fake, Reply, Seen, egress, egress_closed_port, egress_production_roots};
use crate::{Credential, Egress, EgressError, Limits, Request, StatusClass, USER_AGENT};
use std::time::{Duration, Instant};

const TOKEN: &str = "ghp_FAKEtokenFAKEtoken0123456789abcdefXYZ";
const A: &str = "api.test.example";
const B: &str = "evil.test.example";
const NAMES: &[&str] = &[A, B];

fn cred() -> Credential {
    Credential::from_bytes(TOKEN.as_bytes().to_vec()).unwrap()
}

fn fake(h: impl Fn(&Seen) -> Reply + Send + Sync + 'static) -> Fake {
    Fake::start(NAMES, h)
}

fn ok_fake() -> Fake {
    fake(|_| Reply::ok(b"{\"ok\":true}"))
}

fn quick() -> Limits {
    Limits {
        total: Duration::from_secs(1),
        ..Limits::default()
    }
}

/// The token occurs exactly once in everything the server received, as the Authorization value.
fn assert_token_only_in_auth(seen: &Seen) {
    assert!(!seen.request_line.contains(TOKEN), "token in request line");
    assert!(
        !String::from_utf8_lossy(&seen.body).contains(TOKEN),
        "token in body"
    );
    for (name, value) in &seen.headers {
        if name == "authorization" {
            assert_eq!(value, &format!("Bearer {TOKEN}"));
        } else {
            assert!(!value.contains(TOKEN), "token in header {name}");
        }
    }
    let raw = String::from_utf8_lossy(&seen.raw);
    assert_eq!(
        raw.matches(TOKEN).count(),
        1,
        "token must appear exactly once"
    );
}

fn no_secret_text(e: &EgressError) {
    for text in [e.to_string(), format!("{e:?}"), format!("{e:#?}")] {
        assert!(!text.contains(TOKEN));
        assert!(!text.contains("test.example"));
        assert!(!text.contains("http"));
    }
}

// ---- host and URL rules -------------------------------------------------------------------

#[test]
fn hostile_hosts_are_blocked_before_any_network_use() {
    let e = Egress::new(&["api.github.com"]).unwrap();
    let c = cred();
    let all: Vec<&str> = HOSTILE_HOSTS
        .iter()
        .chain(LOOKALIKE_HOSTS)
        .copied()
        .collect();
    assert!(all.len() >= 25);
    for h in all {
        for r in [
            Request::get(h, "/user"),
            Request::get(h, "/user").credential(&c),
        ] {
            assert_eq!(e.send(&r).err(), Some(EgressError::Blocked), "host {h:?}");
        }
    }
}

#[test]
fn only_the_exact_allow_listed_host_is_reached() {
    let f = ok_fake();
    let e = egress(&[A], Limits::default(), &[(A, &f), (B, &f)]);
    assert_eq!(
        e.send(&Request::get(B, "/x")).err(),
        Some(EgressError::Blocked)
    );
    assert_eq!(f.connection_count(), 0);
    assert!(e.send(&Request::get(A, "/x")).is_ok());
}

#[test]
fn hostile_paths_are_blocked_before_any_network_use() {
    let f = ok_fake();
    let e = egress(&[A], Limits::default(), &[(A, &f)]);
    for p in [
        "https://evil.test.example/x",
        "//evil.test.example/x",
        "/a\r\nHost: evil",
        "/a#frag",
        "/..",
        "/a b",
        "/\\x",
        "x",
        "",
    ] {
        assert_eq!(
            e.send(&Request::get(A, p)).err(),
            Some(EgressError::Blocked),
            "{p:?}"
        );
    }
    assert_eq!(f.connection_count(), 0);
}

#[test]
fn bad_allow_list_entries_are_refused() {
    for list in [
        &[][..],
        &["*"][..],
        &["API.github.com"][..],
        &["127.0.0.1"][..],
        &["api.github.com:443"][..],
        &["https://api.github.com"][..],
        &["api.github.com."][..],
        &["api.github.com", "evil.com/x"][..],
    ] {
        assert!(Egress::new(list).is_err(), "{list:?}");
    }
}

#[test]
fn plain_http_is_never_sent() {
    let f = ok_fake();
    let e = egress(&[A], Limits::default(), &[(A, &f)]);
    assert!(e.agent().config().https_only());
    assert!(e.agent().get(format!("http://{A}/x")).call().is_err());
    assert_eq!(f.connection_count(), 0);
}

#[test]
fn agent_settings_match_the_rules() {
    let f = ok_fake();
    let e = egress(&[A], Limits::default(), &[(A, &f)]);
    let c = e.agent().config();
    assert!(c.https_only());
    assert!(c.proxy().is_none());
    assert_eq!(c.max_redirects(), 0);
    assert!(!c.http_status_as_error());
    assert_eq!(c.timeouts().connect, Some(Duration::from_secs(10)));
    assert_eq!(c.timeouts().global, Some(Duration::from_secs(20)));
    assert_eq!(Limits::default().max_body, 256 * 1024);
    // Limits cannot be switched off.
    let huge = Limits {
        max_body: usize::MAX,
        connect: Duration::from_secs(3600),
        total: Duration::from_secs(3600),
    };
    let e = egress(&[A], huge, &[(A, &f)]);
    assert_eq!(
        e.agent().config().timeouts().global,
        Some(Duration::from_secs(60))
    );
}

// ---- redirects ----------------------------------------------------------------------------

#[test]
fn redirects_are_never_followed_and_the_token_never_reaches_a_second_server() {
    for code in [301u16, 302, 303, 307, 308] {
        for location in [
            format!("https://{B}/steal"),
            "/other".to_owned(),
            format!("//{B}/x"),
        ] {
            let b = fake(|_| Reply::ok(b"B"));
            let loc = location.clone();
            let a = fake(move |_| Reply::with(code, vec![("location", loc.clone())], b""));
            let e = egress(&[A], Limits::default(), &[(A, &a), (B, &b)]);
            let err = e
                .send(&Request::get(A, "/user").credential(&cred()))
                .err()
                .unwrap();
            assert_eq!(err, EgressError::Redirect, "{code} {location}");
            no_secret_text(&err);
            assert_eq!(a.requests().len(), 1, "{code} {location}");
            assert_token_only_in_auth(&a.requests()[0]);
            assert_eq!(b.connection_count(), 0, "{code} {location}");
            assert!(b.requests().is_empty());
        }
    }
}

// ---- proxies and process-level leaks --------------------------------------------------------

const CHILD_ENV: &str = "OW_EGRESS_TEST_CHILD";
const CHILD_TEST: &str = "tests::child_scenarios";
const PROXY_VARS: &[&str] = &[
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
];

/// This test binary again, with proxy variables pointing at `listener`.
fn child_command(mode: &str, listener: &std::net::TcpListener) -> std::process::Command {
    let url = format!("http://{}", listener.local_addr().unwrap());
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", CHILD_TEST, "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, mode)
        .env_remove("NO_PROXY")
        .env_remove("no_proxy");
    for v in PROXY_VARS {
        cmd.env(v, &url);
    }
    cmd
}

fn accepted(listener: &std::net::TcpListener) -> usize {
    listener.set_nonblocking(true).unwrap();
    let mut n = 0;
    while listener.accept().is_ok() {
        n += 1;
    }
    n
}

#[test]
fn proxy_variables_are_ignored_and_nothing_leaks_to_stdout_or_stderr() {
    // Control: a stock ureq agent does honour the variables (it connects to the listener and then
    // waits for a proxy answer that never comes), so the zero below is meaningful.
    let control = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    control.set_nonblocking(true).unwrap();
    let mut child = child_command("control", &control).spawn().unwrap();
    let start = Instant::now();
    let reached = loop {
        if control.accept().is_ok() {
            break true;
        }
        if start.elapsed() > Duration::from_secs(20) {
            break false;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(reached, "control did not reach the proxy");

    let proxy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let out = child_command("scenarios", &proxy).output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "child scenarios failed");
    assert!(
        text.contains("child-scenarios-complete"),
        "child did not run"
    );
    assert!(!text.contains(TOKEN), "token reached stdout or stderr");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        accepted(&proxy),
        0,
        "a proxy listener received a connection"
    );
}

/// Only does anything when started by the test above; then it runs every request path with the
/// proxy variables set. Panic messages never print the token.
#[test]
fn child_scenarios() {
    match std::env::var(CHILD_ENV).as_deref() {
        Ok("control") => {
            let _ = ureq::Agent::new_with_defaults()
                .get("https://api.test.example/")
                .call();
        }
        Ok("scenarios") => {
            scenarios();
            eprintln!("child-scenarios-complete");
        }
        _ => {}
    }
}

fn scenarios() {
    let c = cred();
    let f = fake(|s| match s.request_line.split(' ').nth(1).unwrap_or("") {
        "/404" => Reply::status(404, b"not found"),
        "/500" => Reply::status(500, b"boom"),
        "/redirect" => Reply::with(302, vec![("location", format!("https://{B}/x"))], b""),
        "/big" => Reply::UntilClose {
            status: 200,
            body: vec![b'z'; 300_000],
        },
        "/gzip" => Reply::with(200, vec![("content-encoding", "gzip".to_owned())], b"x"),
        _ => Reply::ok(b"{\"ok\":true}"),
    });
    let e = egress(&[A], Limits::default(), &[(A, &f)]);
    let body = serde_json::json!({"state": "open"});
    let hdr = ("x-github-api-version", "2022-11-28");
    let mut errors = Vec::new();
    assert!(
        e.send(
            &Request::get(A, "/user?per_page=1")
                .credential(&c)
                .header(hdr.0, hdr.1)
        )
        .is_ok()
    );
    assert!(
        e.send(&Request::post(A, "/graphql", &body).credential(&c))
            .is_ok()
    );
    for p in ["/404", "/500", "/redirect", "/big", "/gzip", "/a#x"] {
        errors.push(e.send(&Request::get(A, p).credential(&c)).err().unwrap());
    }
    errors.push(e.send(&Request::get(B, "/").credential(&c)).err().unwrap());
    errors.push(
        e.send(&Request::get(A, &format!("/p/{TOKEN}")).credential(&c))
            .err()
            .unwrap(),
    );
    errors.push(
        egress_closed_port(A)
            .send(&Request::get(A, "/").credential(&c))
            .err()
            .unwrap(),
    );
    errors.push(
        egress_production_roots(&[A], Limits::default(), &[(A, &f)])
            .unwrap()
            .send(&Request::get(A, "/").credential(&c))
            .err()
            .unwrap(),
    );
    assert_eq!(errors.len(), 10);
    for err in &errors {
        no_secret_text(err);
    }
    for seen in f.requests() {
        if seen.header("authorization").is_some() {
            assert_token_only_in_auth(&seen);
        }
    }
}

// ---- token placement ---------------------------------------------------------------------

#[test]
fn token_appears_only_in_the_authorization_header() {
    let f = ok_fake();
    let e = egress(&[A], Limits::default(), &[(A, &f)]);
    let c = cred();
    let body = serde_json::json!({"query": "{ viewer { login } }"});
    e.send(
        &Request::get(A, "/search/issues?q=is%3Apr&per_page=1")
            .credential(&c)
            .header("x-github-api-version", "2022-11-28")
            .header("accept", "application/vnd.github+json"),
    )
    .unwrap();
    e.send(&Request::post(A, "/graphql", &body).credential(&c))
        .unwrap();
    let seen = f.requests();
    assert_eq!(seen.len(), 2);
    for s in &seen {
        assert_token_only_in_auth(s);
        assert_eq!(s.header("user-agent"), Some(USER_AGENT));
        assert_eq!(s.header("accept-encoding"), Some("identity"));
        assert_eq!(s.header("host"), Some(A));
        assert!(
            !s.headers
                .iter()
                .any(|(n, _)| n == "cookie" || n == "proxy-authorization")
        );
    }
    assert_eq!(seen[1].header("content-type"), Some("application/json"));
    assert_eq!(seen[1].body, serde_json::to_vec(&body).unwrap());
}

#[test]
fn no_authorization_header_without_a_credential() {
    let f = ok_fake();
    let e = egress(&[A], Limits::default(), &[(A, &f)]);
    e.send(&Request::get(A, "/zen")).unwrap();
    assert!(f.requests()[0].header("authorization").is_none());
}

#[test]
fn smuggling_the_token_anywhere_else_is_refused() {
    let f = ok_fake();
    let e = egress(&[A], Limits::default(), &[(A, &f)]);
    let c = cred();
    let in_body = serde_json::json!({ "note": format!("x{TOKEN}y") });
    let ok_body = serde_json::json!({});
    let path = format!("/search?q={TOKEN}");
    let value = format!("v {TOKEN}");
    for r in [
        Request::get(A, &path).credential(&c),
        Request::get(A, "/x")
            .header("x-note", &value)
            .credential(&c),
        Request::post(A, "/x", &in_body).credential(&c),
    ] {
        assert_eq!(e.send(&r).err(), Some(EgressError::Blocked));
    }
    // Method and body must agree.
    let mut bad = Request::get(A, "/x");
    bad.body = Some(&ok_body);
    assert_eq!(e.send(&bad).err(), Some(EgressError::Blocked));
    assert_eq!(f.connection_count(), 0);
}

#[test]
fn header_injection_is_impossible() {
    let f = ok_fake();
    let e = egress(&[A], Limits::default(), &[(A, &f)]);
    for v in [
        "a\r\nX-Evil: 1",
        "a\nb",
        "a\rb",
        "a\0b",
        "a\tb",
        "caf\u{e9}",
    ] {
        let r = Request::get(A, "/x").header("x-note", v);
        assert_eq!(e.send(&r).err(), Some(EgressError::Blocked), "{v:?}");
    }
    for n in [
        "X-Upper",
        "a b",
        "a:b",
        "a\r\nb",
        "authorization",
        "host",
        "cookie",
        "user-agent",
        "content-length",
        "transfer-encoding",
        "",
    ] {
        let r = Request::get(A, "/x").header(n, "v");
        assert_eq!(e.send(&r).err(), Some(EgressError::Blocked), "{n:?}");
    }
    for n in ["Bad Name", "a\r\nb", "authorization"] {
        let r = Request::get(A, "/x").expiry_header(n);
        assert_eq!(e.send(&r).err(), Some(EgressError::Blocked), "{n:?}");
    }
    for bytes in [
        &b"ab\r\ncd"[..],
        b"ab\ncd",
        b"ab\0cd",
        b"ab cd",
        b"",
        "t\u{e9}".as_bytes(),
    ] {
        assert!(Credential::from_bytes(bytes.to_vec()).is_err(), "{bytes:?}");
    }
    assert!(Credential::from_bytes(vec![b'a'; 4097]).is_err());
    assert_eq!(f.connection_count(), 0);
}

macro_rules! implements {
    ($t:ty, $tr:path) => {{
        #[allow(dead_code)]
        trait No {
            const V: bool = false;
        }
        struct W<T: ?Sized>(std::marker::PhantomData<T>);
        impl<T: ?Sized> No for W<T> {}
        impl<T: ?Sized + $tr> W<T> {
            #[allow(dead_code)] // unused when the bound does not hold: that is the probe
            const V: bool = true;
        }
        <W<$t>>::V
    }};
}

#[test]
fn credential_has_no_debug_display_clone_or_serialize() {
    assert!(implements!(String, std::fmt::Debug), "probe is broken");
    assert!(!implements!(Credential, std::fmt::Debug));
    assert!(!implements!(Credential, std::fmt::Display));
    assert!(!implements!(Credential, Clone));
    assert!(!implements!(Credential, serde::Serialize));
    assert!(!implements!(Credential, Copy));
}

// ---- limits -------------------------------------------------------------------------------

#[test]
fn response_size_cap_is_enforced() {
    let f = fake(|s| match s.request_line.split(' ').nth(1).unwrap_or("") {
        "/exact" => Reply::ok(&[b'a'; 1024]),
        "/over" => Reply::ok(&[b'a'; 1025]),
        _ => Reply::UntilClose {
            status: 200,
            body: vec![b'a'; 200_000],
        },
    });
    let limits = Limits {
        max_body: 1024,
        ..Limits::default()
    };
    let e = egress(&[A], limits, &[(A, &f)]);
    assert_eq!(
        e.send(&Request::get(A, "/exact")).unwrap().body().len(),
        1024
    );
    assert_eq!(
        e.send(&Request::get(A, "/over")).err(),
        Some(EgressError::TooLarge)
    );
    assert_eq!(
        e.send(&Request::get(A, "/no-length")).err(),
        Some(EgressError::TooLarge)
    );
}

#[test]
fn slow_drip_body_hits_the_total_timeout() {
    let f = fake(|_| Reply::Drip {
        interval: Duration::from_millis(50),
        for_: Duration::from_secs(20),
    });
    let e = egress(&[A], quick(), &[(A, &f)]);
    let start = Instant::now();
    let err = e.send(&Request::get(A, "/x")).err();
    assert_eq!(err, Some(EgressError::Timeout));
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn silent_server_hits_the_total_timeout() {
    let f = fake(|_| Reply::Hang {
        for_: Duration::from_secs(10),
    });
    let e = egress(&[A], quick(), &[(A, &f)]);
    let start = Instant::now();
    assert_eq!(
        e.send(&Request::get(A, "/x")).err(),
        Some(EgressError::Timeout)
    );
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn refused_connection_is_offline() {
    let e = egress_closed_port(A);
    assert_eq!(
        e.send(&Request::get(A, "/x")).err(),
        Some(EgressError::Offline)
    );
}

// ---- TLS ---------------------------------------------------------------------------------

#[test]
fn untrusted_certificate_is_a_tls_error() {
    let f = ok_fake();
    let e = egress_production_roots(&[A], Limits::default(), &[(A, &f)]).unwrap();
    assert_eq!(e.send(&Request::get(A, "/x")).err(), Some(EgressError::Tls));
    // The handshake failed, so no request bytes (and no token) were sent.
    assert!(f.requests().is_empty());
}

#[test]
fn certificate_for_another_name_is_a_tls_error() {
    let f = Fake::start(&[B], |_| Reply::ok(b"x"));
    let e = egress(&[A], Limits::default(), &[(A, &f)]);
    let c = cred();
    let r = Request::get(A, "/x").credential(&c);
    assert_eq!(e.send(&r).err(), Some(EgressError::Tls));
    assert!(f.requests().is_empty());
}

#[test]
fn expired_certificate_is_a_tls_error() {
    let f = Fake::start_expired(&[A]);
    let e = egress(&[A], Limits::default(), &[(A, &f)]);
    assert_eq!(e.send(&Request::get(A, "/x")).err(), Some(EgressError::Tls));
}

// ---- responses -----------------------------------------------------------------------------

#[test]
fn only_whitelisted_response_headers_are_returned() {
    let f = fake(|_| {
        Reply::with(
            200,
            vec![
                ("x-ratelimit-remaining", "4999".to_owned()),
                ("retry-after", "30".to_owned()),
                ("content-type", "application/json".to_owned()),
                (
                    "github-authentication-token-expiration",
                    "2027-01-01 00:00:00 UTC".to_owned(),
                ),
                ("set-cookie", "s=1".to_owned()),
                ("x-secret", "nope".to_owned()),
                ("x-ratelimit-reset", "9".repeat(300)),
            ],
            b"{}",
        )
    });
    let e = egress(&[A], Limits::default(), &[(A, &f)]);
    let r = e
        .send(&Request::get(A, "/user").expiry_header("github-authentication-token-expiration"))
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.header("x-ratelimit-remaining"), Some("4999"));
    assert_eq!(r.header("retry-after"), Some("30"));
    assert_eq!(r.header("content-type"), Some("application/json"));
    assert_eq!(
        r.header("github-authentication-token-expiration"),
        Some("2027-01-01 00:00:00 UTC")
    );
    assert_eq!(r.headers().len(), 4);
    let dbg = format!("{r:?}");
    assert!(!dbg.contains("{}"));
    // Without naming it, the expiry header is not returned.
    let r = e.send(&Request::get(A, "/user")).unwrap();
    assert!(r.header("github-authentication-token-expiration").is_none());
}

#[test]
fn compressed_responses_are_refused() {
    let f = fake(|_| {
        Reply::with(
            200,
            vec![("content-encoding", "gzip".to_owned())],
            b"\x1f\x8b",
        )
    });
    let e = egress(&[A], Limits::default(), &[(A, &f)]);
    assert_eq!(
        e.send(&Request::get(A, "/x")).err(),
        Some(EgressError::Format)
    );
}

#[test]
fn http_errors_become_status_categories_without_the_body() {
    let f = fake(|s| {
        let code = if s.request_line.contains("/s500") {
            500
        } else {
            403
        };
        Reply::with(
            code,
            vec![
                ("retry-after", "60".to_owned()),
                ("x-ratelimit-remaining", "0".to_owned()),
            ],
            b"SECRETBODY",
        )
    });
    let e = egress(&[A], Limits::default(), &[(A, &f)]);
    let err = e
        .send(&Request::get(A, "/x").credential(&cred()))
        .err()
        .unwrap();
    let EgressError::Status {
        class,
        code,
        ref headers,
    } = err
    else {
        panic!("wrong category");
    };
    assert_eq!((class, code), (StatusClass::Client, 403));
    assert!(headers.contains(&("retry-after".to_owned(), "60".to_owned())));
    no_secret_text(&err);
    assert!(!format!("{err:?}").contains("SECRETBODY"));
    let err = e.send(&Request::get(A, "/s500")).err().unwrap();
    assert!(matches!(
        err,
        EgressError::Status {
            class: StatusClass::Server,
            code: 500,
            ..
        }
    ));
}

#[test]
fn every_error_variant_prints_without_secrets() {
    let all = [
        EgressError::Blocked,
        EgressError::Tls,
        EgressError::Offline,
        EgressError::Timeout,
        EgressError::Redirect,
        EgressError::TooLarge,
        EgressError::Format,
        EgressError::Status {
            class: StatusClass::Client,
            code: 404,
            headers: vec![],
        },
        EgressError::Status {
            class: StatusClass::Server,
            code: 502,
            headers: vec![],
        },
        EgressError::Status {
            class: StatusClass::Other,
            code: 100,
            headers: vec![],
        },
    ];
    for e in &all {
        no_secret_text(e);
    }
}

// ---- no logging, no forbidden dependencies --------------------------------------------------

#[test]
fn log_macros_are_compiled_out() {
    assert_eq!(log::STATIC_MAX_LEVEL, log::LevelFilter::Off);
}

#[test]
fn source_has_no_output_logging_env_or_forbidden_dependencies() {
    let forbidden_code = [
        concat!("print", "ln!"),
        concat!("e", "print", "ln!"),
        concat!("print", "!("),
        concat!("e", "print", "!("),
        concat!("db", "g!"),
        concat!("lo", "g::"),
        concat!("trac", "ing"),
        concat!("env", "::var"),
        concat!("set", "_var"),
        concat!("Stdio", "::inherit"),
        concat!("unsa", "fe "),
    ];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let skip = ["tests.rs", "testkit.rs"];
    let mut scanned = 0;
    for entry in std::fs::read_dir(dir.join("src")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        if skip.contains(&name.as_str()) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        // Comments and docs may mention words; only code lines are checked.
        for line in text.lines().filter(|l| !l.trim_start().starts_with("//")) {
            for bad in forbidden_code {
                assert!(!line.contains(bad), "{name}: forbidden `{bad}`");
            }
        }
        scanned += 1;
    }
    assert!(scanned >= 6);
    let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    let deps = manifest.split("[dev-dependencies]").next().unwrap();
    for bad in [
        "tracing",
        "openssl",
        "native-tls",
        "flate2",
        "brotli",
        "env_logger",
        "platform-verifier",
        "rustls-native-certs",
        "reqwest",
        "hyper",
        "gzip",
        "\"json\"",
        "cookies",
        "socks-proxy",
    ] {
        assert!(!deps.contains(bad), "dependency `{bad}` is not allowed");
    }
    assert!(deps.contains("default-features = false"));
}
