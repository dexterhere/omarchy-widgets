//! Shared by the leak tests: the spellings a secret can take and the places to look.
#![allow(dead_code, clippy::unwrap_used)]
use std::io::Read;
use std::sync::{Arc, Mutex};

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn pct(b: &[u8]) -> String {
    b.iter().map(|x| format!("%{x:02X}")).collect()
}

pub fn b64(b: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in b.chunks(3) {
        let n = c.iter().fold(0u32, |n, &x| n << 8 | u32::from(x)) << (8 * (3 - c.len()));
        for i in 0..=c.len() {
            out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

/// Every spelling worth searching for. Base64 is taken at the three alignments a substring can
/// have inside a larger encoded stream, trimming the characters that depend on neighbours.
pub fn spellings(needle: &str) -> Vec<(String, Vec<u8>)> {
    let b = needle.as_bytes();
    let mut v = vec![
        ("raw".to_owned(), b.to_vec()),
        ("hex".into(), hex(b).into_bytes()),
        ("HEX".into(), hex(b).to_uppercase().into_bytes()),
        ("percent".into(), pct(b).into_bytes()),
    ];
    for shift in 0..3usize {
        let mut padded = vec![0u8; shift];
        padded.extend_from_slice(b);
        let e = b64(&padded);
        let start = [0, 2, 3][shift];
        let body = &e[start..e.len().saturating_sub(2)];
        v.push((format!("base64+{shift}"), body.as_bytes().to_vec()));
        v.push((
            format!("base64url+{shift}"),
            body.replace('+', "-").replace('/', "_").into_bytes(),
        ));
    }
    v
}

pub fn drain(mut r: impl Read + Send + 'static) -> Arc<Mutex<Vec<u8>>> {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let b2 = Arc::clone(&buf);
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = r.read(&mut chunk) {
            if n == 0 {
                break;
            }
            b2.lock().unwrap().extend_from_slice(&chunk[..n]);
        }
    });
    buf
}

pub fn files(p: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(p).unwrap() {
        let path = e.unwrap().path();
        let md = std::fs::symlink_metadata(&path).unwrap();
        if md.is_dir() {
            files(&path, out);
        } else if md.is_file() {
            out.push(path);
        }
    }
}
