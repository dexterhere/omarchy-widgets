//! Build rule (DECISIONS AUTH-EGRESS): no crate except `ow-egress` may depend on an HTTP, TLS or
//! socket-client crate or open an internet socket. `deny.toml` states the same rule for
//! `cargo deny`; this test needs only `cargo test`, so it also runs where cargo-deny does not.
//! It is a text scan, not a dependency-graph check: a crate pulled in indirectly is `cargo deny`'s
//! job, and a determined author can evade a grep, so review still matters.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

const BANNED_CRATES: &[&str] = &[
    "ureq",
    "ureq-proto",
    "rustls",
    "rustls-webpki",
    "webpki-roots",
    "rustls-native-certs",
    "rustls-platform-verifier",
    "tokio-rustls",
    "hyper-rustls",
    "native-tls",
    "tokio-native-tls",
    "openssl",
    "openssl-sys",
    "aws-lc-rs",
    "aws-lc-sys",
    "hyper",
    "hyper-util",
    "reqwest",
    "isahc",
    "curl",
    "curl-sys",
    "attohttpc",
    "minreq",
    "surf",
    "h2",
    "httparse",
    "socket2",
    "mio",
    "tokio",
    "async-std",
    "smol",
];

/// Source patterns that open sockets or start programs, matched after removing all whitespace so
/// line breaks and spacing cannot hide them. A use outside `AUDITED` fails the test.
const BANNED_SOURCE: &[&str] = &[
    "rustix::net",
    "nix::sys::socket",
    "SocketAddr",
    "Command::new",
    "std::process::Command",
    "TcpStream",
    "TcpListener",
    "UdpSocket",
    "UnixStream",
    "UnixListener",
    "ToSocketAddrs",
    "libc::socket",
    "libc::connect",
    "AF_INET",
];

/// The only existing uses, each reviewed by hand. Adding a line here is a security decision:
/// say what the use is. Paths are relative to the workspace root.
const AUDITED: &[(&str, &[&str])] = &[
    // `ow` talks to the local daemon over its Unix-domain socket and checks the peer uid
    // (`ow_core::peer_is_self`) before sending anything.
    ("crates/ow/src/daemon.rs", &["UnixStream"]),
    // The one peer-uid check (SO_PEERCRED via rustix::net::sockopt), used by `ow` and `owd`.
    ("crates/ow-core/src/peer.rs", &["UnixStream", "rustix::net"]),
    // `owd` serves its Unix-domain socket and probes it for a stale instance.
    ("crates/owd/src/server.rs", &["UnixStream", "UnixListener"]),
    // `owd` serves Unix-domain connections and checks the peer uid with `ow_core::peer_is_self`.
    ("crates/owd/src/conn.rs", &["UnixStream"]),
    // Integration tests of the daemon socket.
    (
        "crates/owd/tests/socket.rs",
        &["UnixStream", "UnixListener"],
    ),
    // Tests that run the freshly built `ow` and `owd` binaries (CARGO_BIN_EXE_*).
    ("crates/ow/tests/vault_cli.rs", &["Command::new"]),
    ("crates/owd/tests/hardening.rs", &["Command::new"]),
    // Runs the freshly built `ow` binary (CARGO_BIN_EXE_ow) for the install verbs, with an
    // environment that tries to redirect the registry. It uses no network on any path it takes
    // (every case is refused before a request, or needs none), and no internet socket.
    ("crates/ow/tests/install_cli.rs", &["Command::new"]),
    // Tests of the unlock path: local Unix-domain sockets and fake listeners, and child processes
    // that run the freshly built binaries. No internet sockets.
    (
        "crates/ow/tests/daemon_cli.rs",
        &["Command::new", "UnixStream"],
    ),
    (
        "crates/ow/tests/socket_trust.rs",
        &["Command::new", "UnixListener"],
    ),
    ("crates/owd/tests/leak.rs", &["Command::new", "UnixStream"]),
    // Same search, but the test binary re-runs itself as a daemon with a secret-backed provider.
    (
        "crates/owd/tests/provider_leak.rs",
        &["Command::new", "UnixStream"],
    ),
    ("crates/owd/tests/unlock.rs", &["UnixStream"]),
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Walks `dir`, skipping only `<root>/target`, `<root>/fuzz/target` and `<root>/.git`, and the
/// exempt crate directory `<root>/crates/ow-egress`. Matching is by exact path, not by name, so a
/// directory called `target` or `ow-egress` anywhere else is scanned.
fn files(root: &Path, dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let skip = [
        root.join("target"),
        root.join("fuzz").join("target"),
        root.join(".git"),
        root.join("crates").join("ow-egress"),
    ];
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.map(|e| e.unwrap().path()) {
        if skip.contains(&e) {
            continue;
        }
        if e.is_dir() {
            files(root, &e, ext, out);
        } else if e.extension().and_then(|x| x.to_str()) == Some(ext) {
            out.push(e);
        }
    }
}

fn collect(root: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    files(root, root, ext, &mut out);
    out
}

/// Every crate name a manifest depends on, with renames resolved through `package`.
fn dependency_names(manifest: &str) -> Vec<String> {
    fn tables(t: &toml::Table, out: &mut Vec<String>) {
        for (k, v) in t {
            if k.ends_with("dependencies") {
                for (name, spec) in v.as_table().into_iter().flatten() {
                    out.push(name.clone());
                    if let Some(p) = spec.get("package").and_then(|p| p.as_str()) {
                        out.push(p.to_owned());
                    }
                }
            } else if let Some(t) = v.as_table() {
                tables(t, out);
            }
        }
    }
    let mut out = Vec::new();
    tables(&manifest.parse::<toml::Table>().unwrap(), &mut out);
    out
}

fn manifests_with_banned_crates(root: &Path) -> Vec<String> {
    let mut bad = Vec::new();
    for path in collect(root, "toml") {
        if path.file_name().unwrap() != "Cargo.toml" {
            continue;
        }
        for dep in dependency_names(&fs::read_to_string(&path).unwrap()) {
            if BANNED_CRATES.contains(&dep.as_str()) {
                bad.push(format!("{} depends on `{dep}`", path.display()));
            }
        }
    }
    bad
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap().to_string_lossy().into_owned()
}

/// Text of `use crate::{...}` groups starting at each `prefix{`, braces balanced.
fn groups<'a>(text: &'a str, prefix: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = text[from..].find(prefix) {
        let start = from + i + prefix.len();
        let mut depth = 1;
        let mut end = start;
        for (j, c) in text[start..].char_indices() {
            depth += (c == '{') as i32 - (c == '}') as i32;
            if depth == 0 {
                end = start + j;
                break;
            }
        }
        out.push(&text[start..end]);
        from = start;
    }
    out
}

/// Banned patterns found in one source file (whitespace removed, so layout cannot hide them).
fn banned_in(source: &str) -> Vec<String> {
    let text: String = source.chars().filter(|c| !c.is_whitespace()).collect();
    let mut found: Vec<String> = BANNED_SOURCE
        .iter()
        .filter(|p| text.contains(**p))
        .map(|p| (*p).to_owned())
        .collect();
    if text.to_lowercase().contains("addressfamily")
        || text.to_lowercase().contains("address_family")
    {
        found.push("AddressFamily".to_owned());
    }
    // `use rustix::{fd::X, net::Y}` and `use nix::{sys::socket::Z}` hide the path behind braces.
    if groups(&text, "rustix::{")
        .iter()
        .any(|g| g.starts_with("net") || g.contains(",net"))
    {
        found.push("rustix::net".to_owned());
    }
    if groups(&text, "nix::{").iter().any(|g| g.contains("socket")) {
        found.push("nix::sys::socket".to_owned());
    }
    found
}

fn sources_with_banned_use(root: &Path) -> Vec<String> {
    let mut bad = Vec::new();
    for path in collect(root, "rs") {
        let name = rel(root, &path);
        let allowed: &[&str] = AUDITED
            .iter()
            .find(|(f, _)| *f == name)
            .map_or(&[], |(_, p)| *p);
        for pat in banned_in(&fs::read_to_string(&path).unwrap()) {
            if !allowed.contains(&pat.as_str()) {
                bad.push(format!("{name} uses `{pat}`"));
            }
        }
    }
    bad
}

#[test]
fn no_other_crate_depends_on_an_http_tls_or_socket_client_crate() {
    let bad = manifests_with_banned_crates(&root());
    assert!(
        bad.is_empty(),
        "network crates belong to ow-egress only: {bad:?}"
    );
}

#[test]
fn no_other_crate_opens_a_socket_or_starts_a_program_unaudited() {
    let bad = sources_with_banned_use(&root());
    assert!(bad.is_empty(), "unaudited use (see AUDITED): {bad:?}");
}

/// Crates allowed to depend on `ow-egress` (the one network door). A new user is a security
/// decision: add it here with the reason. `ow`: `ow install` fetches the registry index and the
/// widget files (two fixed hosts, https, no redirects, size caps) through it.
const EGRESS_USERS: &[&str] = &["crates/ow/Cargo.toml"];

#[test]
fn only_listed_crates_depend_on_ow_egress() {
    let root = root();
    let mut users: Vec<String> = collect(&root, "toml")
        .into_iter()
        .filter(|p| p.file_name().unwrap() == "Cargo.toml")
        .filter(|p| {
            dependency_names(&fs::read_to_string(p).unwrap()).contains(&"ow-egress".to_owned())
        })
        .map(|p| rel(&root, &p))
        .collect();
    users.sort();
    assert_eq!(users, EGRESS_USERS, "update EGRESS_USERS and say why");
}

#[test]
fn the_audited_list_has_no_stale_entries() {
    let root = root();
    for (file, pats) in AUDITED {
        let found = banned_in(&fs::read_to_string(root.join(file)).unwrap());
        for p in *pats {
            assert!(
                found.iter().any(|f| f == p),
                "{file} no longer uses `{p}`; remove it"
            );
        }
    }
}

#[test]
fn the_scan_sees_the_workspace() {
    let root = root();
    assert!(collect(&root, "toml").len() >= 7);
    assert!(collect(&root, "rs").len() >= 10);
}

#[test]
fn dependency_parsing_handles_every_spelling() {
    for m in [
        "[dependencies]\nureq = \"3\"",
        "[dependencies.ureq]\nversion = \"3\"",
        "[dev-dependencies]\nx = { package = \"ureq\", version = \"3\" }",
        "[dependencies]\nx = { version = \"3\",\n  package = \"ureq\",\n}",
        "[dependencies.x]\npackage = \"ureq\"\nversion = \"3\"",
        "[target.'cfg(unix)'.dependencies]\nureq.workspace = true",
        "[build-dependencies]\nureq = \"3\"",
        "[workspace.dependencies]\nureq = \"3\"",
        "[target.x86_64-unknown-linux-gnu.dev-dependencies]\nureq = \"3\"",
    ] {
        assert!(dependency_names(m).contains(&"ureq".to_owned()), "{m}");
    }
}

#[test]
fn banned_source_patterns_are_found_in_any_layout() {
    for s in [
        "let s = TcpStream::connect(a);",
        "use rustix::net::connect;",
        "use rustix :: \n net::connect;",
        "use rustix::{fd::AsFd, net::connect};",
        "use rustix::{\n    net,\n};",
        "use nix::sys::socket::socket;",
        "use nix::{sys::socket};",
        "let a: SocketAddr;",
        "std::process::Command::new(\"curl\")",
        "Command\n::new(x)",
        "use rustix::net::AddressFamily;",
        "let f = libc::AF_INET;",
        "UdpSocket::bind(a)",
        "UnixStream::connect(p)",
    ] {
        assert!(!banned_in(s).is_empty(), "missed: {s}");
    }
    assert!(banned_in("use rustix::fs::flock; use rustix::process::geteuid;").is_empty());
}

#[test]
fn scanner_flags_files_hidden_in_lookalike_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let put = |rel: &str, text: &str| {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    };
    // Exempt: the real egress crate, the workspace target and .git.
    put("crates/ow-egress/src/lib.rs", "use std::net::TcpStream;");
    put(
        "crates/ow-egress/Cargo.toml",
        "[dependencies]\nureq = \"3\"",
    );
    put("target/debug/x.rs", "use std::net::TcpStream;");
    put(".git/x.rs", "use std::net::TcpStream;");
    assert!(sources_with_banned_use(&root).is_empty());
    assert!(manifests_with_banned_crates(&root).is_empty());
    // Not exempt: same names elsewhere.
    put("crates/evil/src/target/net.rs", "use std::net::TcpStream;");
    put(
        "crates/evil/ow-egress/Cargo.toml",
        "[dependencies]\nureq = \"3\"",
    );
    put(
        "crates/evil/ow-egress/src/lib.rs",
        "use std::net::UdpSocket;",
    );
    put("crates/evil/.git/x.rs", "use std::net::TcpListener;");
    let src = sources_with_banned_use(&root);
    assert_eq!(src.len(), 3, "{src:?}");
    assert!(
        src.iter()
            .any(|s| s.contains("crates/evil/src/target/net.rs"))
    );
    assert!(
        src.iter()
            .any(|s| s.contains("crates/evil/ow-egress/src/lib.rs"))
    );
    assert_eq!(manifests_with_banned_crates(&root).len(), 1);
}

#[test]
fn deny_toml_bans_exactly_the_crates_this_scanner_bans() {
    let deny: toml::Table = fs::read_to_string(root().join("deny.toml"))
        .unwrap()
        .parse()
        .unwrap();
    let mut in_deny: Vec<&str> = deny["bans"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["crate"].as_str().unwrap())
        .collect();
    let mut in_scan = BANNED_CRATES.to_vec();
    in_deny.sort_unstable();
    in_scan.sort_unstable();
    assert_eq!(in_deny, in_scan);
}
