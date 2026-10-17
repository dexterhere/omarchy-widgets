//! Append-only JSONL audit log. Entries are typed so a payload value cannot be logged:
//! there is no field that could carry one.
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
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

const KEEP: u32 = 5;

fn open(path: &Path) -> std::io::Result<File> {
    let f = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?;
    // `mode` only applies on creation; tighten a pre-existing file too.
    f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    Ok(f)
}

fn numbered(path: &Path, n: u32) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(format!(".{n}"));
    PathBuf::from(s)
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

    /// Returns whether the entry reached the file. Callers that release data must
    /// treat `false` as a failure: no audit line, no data.
    pub fn record(
        &mut self,
        widget: &str,
        topic: &str,
        op: &str,
        allow: bool,
        bytes: usize,
    ) -> bool {
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
            return false;
        };
        line.push('\n');
        if !self.reopen_if_needed() {
            return false;
        }
        self.file.write_all(line.as_bytes()).is_ok()
    }

    /// Reopen when the path was deleted/replaced under us; rotate when large.
    fn reopen_if_needed(&mut self) -> bool {
        let on_disk = std::fs::metadata(&self.path).ok();
        let ours = self.file.metadata().ok();
        let replaced = match (&on_disk, &ours) {
            (Some(a), Some(b)) => a.ino() != b.ino() || a.dev() != b.dev(),
            _ => true,
        };
        let big = on_disk.as_ref().is_some_and(|m| m.len() >= ROTATE_AT);
        if big && !replaced {
            for n in (1..KEEP).rev() {
                let _ = std::fs::rename(numbered(&self.path, n), numbered(&self.path, n + 1));
            }
            let _ = std::fs::rename(&self.path, numbered(&self.path, 1));
        }
        if replaced || big {
            match open(&self.path) {
                Ok(f) => self.file = f,
                Err(_) => return false,
            }
        }
        true
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
        assert!(a.record("w", "github.prs", "snapshot", false, 0));
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
    fn deleted_file_is_recreated_and_old_mode_is_tightened() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("audit.jsonl");
        std::fs::write(&p, b"").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        let mut a = AuditLog::open(&p).unwrap();
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::remove_file(&p).unwrap();
        assert!(a.record("w", "t", "snapshot", true, 1));
        assert!(
            std::fs::read_to_string(&p)
                .unwrap()
                .contains("\"widget\":\"w\"")
        );
    }

    #[test]
    fn rotation_keeps_numbered_history() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("audit.jsonl");
        let mut a = AuditLog::open(&p).unwrap();
        for _ in 0..3 {
            std::fs::write(&p, vec![b'x'; ROTATE_AT as usize]).unwrap();
            assert!(a.record("w", "t", "snapshot", true, 1));
        }
        assert!(d.path().join("audit.jsonl.1").exists());
        assert!(d.path().join("audit.jsonl.2").exists());
    }

    #[test]
    fn rotates_when_large() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("audit.jsonl");
        std::fs::write(&p, vec![b'x'; ROTATE_AT as usize]).unwrap();
        let mut a = AuditLog::open(&p).unwrap();
        assert!(a.record("w", "t", "snapshot", true, 1));
        assert!(d.path().join("audit.jsonl.1").exists());
        assert!(std::fs::metadata(&p).unwrap().len() < 200);
    }
}
