//! Append-only JSONL audit log. Entries are typed so a payload value cannot be logged:
//! there is no field that could carry one.
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const ROTATE_AT: u64 = 1024 * 1024;
const MAX_ID_CHARS: usize = 128;

#[derive(Serialize)]
struct Entry<'a> {
    ts: u64,
    widget: &'a str,
    topic: &'a str,
    op: &'a str,
    decision: &'a str,
    bytes: usize,
}

pub struct AuditLog {
    path: PathBuf,
    file: File,
}

fn open(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
}

/// Client-supplied ids are untrusted: bound their size before they reach the log.
fn clip(s: &str) -> &str {
    match s.char_indices().nth(MAX_ID_CHARS) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

impl AuditLog {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        Ok(Self {
            path: path.to_owned(),
            file: open(path)?,
        })
    }

    pub fn record(&mut self, widget: &str, topic: &str, op: &str, allow: bool, bytes: usize) {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let e = Entry {
            ts,
            widget: clip(widget),
            topic: clip(topic),
            op,
            decision: if allow { "allow" } else { "deny" },
            bytes,
        };
        let Ok(mut line) = serde_json::to_string(&e) else {
            return;
        };
        line.push('\n');
        if self.file.metadata().is_ok_and(|m| m.len() >= ROTATE_AT) {
            let _ = std::fs::rename(&self.path, self.path.with_extension("jsonl.1"));
            if let Ok(f) = open(&self.path) {
                self.file = f;
            }
        }
        // Audit failure must not take the daemon down; the request outcome is unaffected.
        let _ = self.file.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn writes_private_jsonl_without_payload() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("audit.jsonl");
        let mut a = AuditLog::open(&p).unwrap();
        a.record("w", "github.prs", "snapshot", false, 0);
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains(r#""decision":"deny""#) && text.ends_with('\n'));
    }

    #[test]
    fn hostile_ids_are_clipped_and_escaped() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("audit.jsonl");
        let mut a = AuditLog::open(&p).unwrap();
        a.record(
            &format!("{}\n{{\"x\":1}}", "a".repeat(500)),
            "t",
            "snapshot",
            true,
            1,
        );
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(text.lines().count(), 1, "log injection via newline");
        assert!(text.len() < 400);
    }

    #[test]
    fn rotates_when_large() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("audit.jsonl");
        std::fs::write(&p, vec![b'x'; ROTATE_AT as usize]).unwrap();
        let mut a = AuditLog::open(&p).unwrap();
        a.record("w", "t", "snapshot", true, 1);
        assert!(d.path().join("audit.jsonl.1").exists());
        assert!(std::fs::metadata(&p).unwrap().len() < 200);
    }
}
