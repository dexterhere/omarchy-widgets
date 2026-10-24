//! Disk side of install and remove: the lock, staged writes, the swap with rollback, the
//! install record, and atomic replacement of the grants file. Every path is derived from the
//! widgets directory and an id that passed `text::store_id`; nothing fetched is used as a path.
use super::grants_edit::Entry;
use super::json;
use super::text::store_id;
use ow_core::Paths;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

pub const RECORD: &str = "installed.json";
const MAX_RECORD: usize = 64 * 1024;

fn io(what: &str, e: std::io::Error) -> String {
    format!("{what}: {e}")
}

/// What `ow install` remembers about one widget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub id: String,
    pub repo: String,
    pub commit: String,
    pub version: String,
    pub status: String,
    pub tier: String,
    pub installed_at: String,
    pub files: BTreeMap<String, String>,
    pub granted: Vec<Entry>,
}

impl Installed {
    pub fn to_json(&self) -> Vec<u8> {
        let granted: Vec<Value> = self
            .granted
            .iter()
            .map(|g| json!({"topics": g.topics, "fields": g.fields}))
            .collect();
        let v = json!({
            "id": self.id, "repo": self.repo, "commit": self.commit, "version": self.version,
            "status": self.status, "tier": self.tier, "installedAt": self.installed_at,
            "files": self.files, "granted": granted,
        });
        let mut b = serde_json::to_vec_pretty(&v).unwrap_or_default();
        b.push(b'\n');
        b
    }

    pub fn parse(bytes: &[u8]) -> Result<Installed, String> {
        if bytes.len() > MAX_RECORD {
            return Err("install record is too large".into());
        }
        let v = json::strict(bytes, json::MAX_DEPTH)?;
        let s = |k: &str| {
            v[k].as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("install record: {k} missing"))
        };
        let strs = |x: &Value| -> Vec<String> {
            x.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|y| y.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        };
        let files = v["files"]
            .as_object()
            .ok_or("install record: files missing")?
            .iter()
            .filter_map(|(k, h)| h.as_str().map(|h| (k.clone(), h.to_owned())))
            .collect();
        let granted = v["granted"]
            .as_array()
            .ok_or("install record: granted missing")?
            .iter()
            .map(|g| Entry {
                topics: strs(&g["topics"]),
                fields: strs(&g["fields"]),
            })
            .collect();
        let commit = s("commit")?;
        if !super::text::full_sha(&commit) {
            return Err("install record: commit is not a 40-character SHA".into());
        }
        Ok(Installed {
            id: s("id")?,
            repo: s("repo")?,
            commit,
            version: s("version")?,
            status: s("status")?,
            tier: s("tier")?,
            installed_at: s("installedAt")?,
            files,
            granted,
        })
    }
}

/// The directory of one widget. `None` if the id is not a valid store id: callers must refuse.
pub fn widget_dir(paths: &Paths, id: &str) -> Option<PathBuf> {
    store_id(id).then(|| paths.widgets().join(id))
}

/// Read the record of an installed widget. `Ok(None)` = nothing installed there.
pub fn read_installed(paths: &Paths, id: &str) -> Result<Option<Installed>, String> {
    let dir = widget_dir(paths, id).ok_or("not a valid widget id")?;
    let file = dir.join(RECORD);
    match read_capped(&file, MAX_RECORD) {
        Ok(b) => Installed::parse(&b).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io("cannot read the install record", e)),
    }
}

/// Read a regular file without following a final symlink, at most `cap` bytes (more is an error).
pub fn read_capped(path: &Path, cap: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let f = fs::OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(path)?;
    if !f.metadata()?.is_file() {
        return Err(std::io::Error::other("not a regular file"));
    }
    let mut b = Vec::new();
    f.take(cap as u64 + 1).read_to_end(&mut b)?;
    if b.len() > cap {
        return Err(std::io::Error::other("file too large"));
    }
    Ok(b)
}

/// One install or remove at a time. Holds an exclusive flock on `widgets/.lock` until dropped,
/// and has removed leftovers (`.stage-*`, `.old-*`) of crashed runs.
pub struct Lock {
    _file: fs::File,
    pub widgets: PathBuf,
}

impl Lock {
    pub fn acquire(paths: &Paths) -> Result<Lock, String> {
        let widgets = paths.widgets();
        if let Some(parent) = widgets.parent() {
            ow_core::private_dir(parent).map_err(|e| io("data directory", e))?;
        }
        ow_core::private_dir(&widgets).map_err(|e| io("widgets directory", e))?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(widgets.join(".lock"))
            .map_err(|e| io("lock file", e))?;
        rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive)
            .map_err(|_| "another ow install or remove is running".to_string())?;
        let l = Lock {
            _file: file,
            widgets,
        };
        l.sweep();
        Ok(l)
    }

    /// Remove `.stage-*` and `.old-*` directories (never following symlinks, never anything else).
    fn sweep(&self) {
        let Ok(rd) = fs::read_dir(&self.widgets) else {
            return;
        };
        for e in rd.flatten() {
            let n = e.file_name();
            let n = n.to_string_lossy();
            if n.starts_with(".stage-") || n.starts_with(".old-") {
                remove_tree(&e.path());
            }
        }
    }
}

/// Delete a directory tree, or a single symlink/file, without following symlinks.
/// `std::fs::remove_dir_all` does not follow symlinks inside the tree either.
fn remove_tree(p: &Path) {
    match fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => {
            let _ = fs::remove_dir_all(p);
        }
        Ok(_) => {
            let _ = fs::remove_file(p);
        }
        Err(_) => {}
    }
}

fn unique_suffix() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    format!(
        "{}-{t:x}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    )
}

/// Write a new file, mode 0600, failing if it exists, and flush it to disk.
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| io("cannot create a file", e))?;
    f.write_all(bytes)
        .map_err(|e| io("cannot write a file", e))?;
    f.sync_all().map_err(|e| io("cannot flush a file", e))
}

fn sync_dir(p: &Path) {
    if let Ok(d) = fs::File::open(p) {
        let _ = d.sync_all();
    }
}

/// A widget directory written next to its final place and swapped in. Dropping it without
/// `finish` leaves nothing behind: the stage is removed and an earlier install is put back.
#[must_use = "call finish() or let it roll back"]
pub struct Swap {
    widgets: PathBuf,
    target: PathBuf,
    stage: PathBuf,
    old: Option<PathBuf>,
    swapped: bool,
    done: bool,
}

/// Write `files` plus the record into a stage directory and swap it into `widgets/<id>`. An
/// existing install moves aside and comes back on rollback.
pub fn stage_and_swap(
    lock: &Lock,
    id: &str,
    files: &[(&str, &[u8])],
    record: &Installed,
) -> Result<Swap, String> {
    if !store_id(id) || id != record.id {
        return Err("internal: bad id for storage".into());
    }
    let suffix = unique_suffix();
    let stage = lock.widgets.join(format!(".stage-{suffix}"));
    let target = lock.widgets.join(id);
    let mut sw = Swap {
        widgets: lock.widgets.clone(),
        target,
        stage,
        old: None,
        swapped: false,
        done: false,
    };
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&sw.stage)
        .map_err(|e| io("cannot create the staging directory", e))?;
    for (name, bytes) in files {
        if !matches!(*name, "manifest.json" | "widget.json" | "layout.json") {
            return Err("internal: unexpected file name".into());
        }
        write_new(&sw.stage.join(name), bytes)?;
    }
    write_new(&sw.stage.join(RECORD), &record.to_json())?;
    sync_dir(&sw.stage);
    match fs::symlink_metadata(&sw.target) {
        Ok(_) => {
            let old = lock.widgets.join(format!(".old-{suffix}"));
            fs::rename(&sw.target, &old)
                .map_err(|e| io("cannot move the existing install aside", e))?;
            sw.old = Some(old);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io("cannot inspect the install location", e)),
    }
    fs::rename(&sw.stage, &sw.target).map_err(|e| io("cannot move the new files into place", e))?;
    sw.swapped = true;
    sync_dir(&sw.widgets);
    Ok(sw)
}

impl Swap {
    /// Keep the new install and delete the old one.
    pub fn finish(mut self) {
        if let Some(old) = self.old.take() {
            remove_tree(&old);
        }
        self.done = true;
    }
}

impl Drop for Swap {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        if self.swapped {
            remove_tree(&self.target);
        }
        remove_tree(&self.stage);
        if let Some(old) = self.old.take() {
            let _ = fs::rename(&old, &self.target);
        }
    }
}

/// Remove `widgets/<id>` (the directory, or a symlink in its place, never what the link points
/// to). Returns whether anything was there.
pub fn remove_widget_dir(lock: &Lock, id: &str) -> Result<bool, String> {
    if !store_id(id) {
        return Err("not a valid widget id".into());
    }
    let target = lock.widgets.join(id);
    match fs::symlink_metadata(&target) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(io("cannot inspect the widget directory", e)),
        Ok(_) => {}
    }
    // Rename first so a failure part-way never leaves a half-deleted live install.
    let gone = lock.widgets.join(format!(".old-{}", unique_suffix()));
    fs::rename(&target, &gone).map_err(|e| io("cannot remove the widget directory", e))?;
    remove_tree(&gone);
    sync_dir(&lock.widgets);
    Ok(true)
}

/// Replace the grants file atomically: temp file (0600) in the same directory, flush, rename,
/// flush the directory.
pub fn write_grants(path: &Path, text: &str) -> Result<(), String> {
    write_private_atomic(path, text, ow_core::MAX_GRANTS_FILE, "grants file")
}

/// Replace a small private text file (`grants.toml`, `config.toml`): the directory is made or
/// checked private (0700, ours), the text goes to a new temp file in it (`O_EXCL`, 0600, so a
/// pre-placed link is never followed), is flushed, and is renamed over `path` (a symlink at
/// `path` is replaced, not followed); then the directory is flushed. `what` names the file in
/// error text.
pub fn write_private_atomic(path: &Path, text: &str, cap: u64, what: &str) -> Result<(), String> {
    if text.len() as u64 > cap {
        return Err(format!("the {what} would exceed the size the daemon reads"));
    }
    let dir = path.parent().ok_or("bad path")?;
    ow_core::private_dir(dir).map_err(|e| io("config directory", e))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("bad path")?;
    let tmp = dir.join(format!("{name}.{}.tmp", unique_suffix()));
    let r = write_new(&tmp, text.as_bytes()).and_then(|()| {
        fs::rename(&tmp, path).map_err(|e| io(&format!("cannot replace the {what}"), e))
    });
    if r.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    r?;
    sync_dir(dir);
    Ok(())
}

/// Ids with an install record, sorted. Entries that are not valid ids or not directories are
/// skipped (a stray file cannot make `list` fail or misreport).
pub fn installed_ids(paths: &Paths) -> Vec<String> {
    let Ok(rd) = fs::read_dir(paths.widgets()) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| store_id(n))
        .collect();
    ids.sort();
    ids
}
