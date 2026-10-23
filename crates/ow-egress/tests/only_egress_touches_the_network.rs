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
];

/// Source patterns for internet sockets. Unix-domain sockets stay allowed (the daemon's own
/// socket uses them).
const BANNED_SOURCE: &[&str] = &[
    "TcpStream",
    "TcpListener",
    "UdpSocket",
    "ToSocketAddrs",
    "SocketAddrV4",
    "SocketAddrV6",
    "AddressFamily::INET",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.map(|e| e.unwrap().path()) {
        if e.is_dir() {
            if !matches!(
                e.file_name().and_then(|n| n.to_str()),
                Some("target" | ".git")
            ) {
                files(&e, ext, out);
            }
        } else if e.extension().and_then(|x| x.to_str()) == Some(ext) {
            out.push(e);
        }
    }
}

/// Crate names a manifest depends on (any dependency table, including renames).
fn dependency_names(manifest: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_deps = false;
    for line in manifest.lines().map(str::trim) {
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_deps = header.ends_with("dependencies");
            if let Some((_, name)) = header.rsplit_once("dependencies.") {
                names.push(name.trim_matches('"').to_owned());
            }
        } else if in_deps && !line.starts_with('#') {
            if let Some((key, rest)) = line.split_once('=') {
                names.push(
                    key.trim()
                        .split('.')
                        .next()
                        .unwrap()
                        .trim_matches('"')
                        .to_owned(),
                );
                if let Some(pkg) = rest
                    .split("package")
                    .nth(1)
                    .and_then(|p| p.split('"').nth(1))
                {
                    names.push(pkg.to_owned());
                }
            }
        } else if line.starts_with("package") {
            if let Some(pkg) = line.split('"').nth(1) {
                names.push(pkg.to_owned());
            }
        }
    }
    names
}

fn other_manifests() -> Vec<PathBuf> {
    let mut all = Vec::new();
    files(&root(), "toml", &mut all);
    all.retain(|p| p.file_name().unwrap() == "Cargo.toml");
    all.retain(|p| !p.components().any(|c| c.as_os_str() == "ow-egress"));
    all
}

#[test]
fn the_scan_sees_the_workspace() {
    let m = other_manifests();
    assert!(m.len() >= 7, "found only {m:?}");
    assert!(dependency_names("[dependencies]\nureq = \"3\"").contains(&"ureq".to_owned()));
    assert!(dependency_names("[dependencies.ureq]\nversion = \"3\"").contains(&"ureq".to_owned()));
    assert!(
        dependency_names("[dev-dependencies]\nx = { package = \"ureq\", version = \"3\" }")
            .contains(&"ureq".to_owned())
    );
    assert!(
        dependency_names("[target.'cfg(unix)'.dependencies]\nrustls.workspace = true")
            .contains(&"rustls".to_owned())
    );
}

#[test]
fn no_other_crate_depends_on_an_http_tls_or_socket_client_crate() {
    for path in other_manifests() {
        let text = fs::read_to_string(&path).unwrap();
        for dep in dependency_names(&text) {
            assert!(
                !BANNED_CRATES.contains(&dep.as_str()),
                "{} depends on `{dep}`: network crates belong to ow-egress only",
                path.display()
            );
        }
    }
}

#[test]
fn no_other_crate_opens_an_internet_socket() {
    let mut sources = Vec::new();
    files(&root().join("crates"), "rs", &mut sources);
    files(&root().join("fuzz"), "rs", &mut sources);
    sources.retain(|p| !p.components().any(|c| c.as_os_str() == "ow-egress"));
    assert!(sources.len() >= 10);
    for path in sources {
        let text = fs::read_to_string(&path).unwrap();
        for bad in BANNED_SOURCE {
            assert!(!text.contains(bad), "{} uses `{bad}`", path.display());
        }
    }
}
