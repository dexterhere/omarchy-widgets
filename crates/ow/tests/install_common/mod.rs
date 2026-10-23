//! Shared helpers for the install tests: a fake byte source over the real widget files and the
//! real registry index, a scripted terminal, and tree snapshots. Nothing here touches the network
//! or the real home directory.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]
use ow::install::index::{Listing, file_path};
use ow::install::{Source, Ui};
use ow_core::Paths;
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const SHA_A: &str = "0123456789abcdef0123456789abcdef01234567";
pub const SHA_B: &str = "fedcba9876543210fedcba9876543210fedcba98";

pub fn fixture(widget: &str, file: &str) -> Vec<u8> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(widget)
        .join(file);
    std::fs::read(p).unwrap()
}

pub fn registry_fixture() -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/registry.json");
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}

pub fn id_of(widget: &str) -> String {
    format!("io.github.dexterhere.{widget}")
}

#[derive(Default)]
pub struct Inner {
    pub index: Vec<u8>,
    pub files: BTreeMap<String, Vec<u8>>,
    pub log: Mutex<Vec<String>>,
}

#[derive(Clone)]
pub struct Fake(pub Arc<Inner>);

impl Fake {
    pub fn log(&self) -> Vec<String> {
        self.0.log.lock().unwrap().clone()
    }
}

impl Source for Fake {
    fn index(&self) -> Result<Vec<u8>, String> {
        self.0.log.lock().unwrap().push("index".into());
        Ok(self.0.index.clone())
    }
    fn file(&self, l: &Listing, name: &str) -> Result<Vec<u8>, String> {
        let path = file_path(l, name)?;
        self.0.log.lock().unwrap().push(path.clone());
        self.0
            .files
            .get(&path)
            .cloned()
            .ok_or_else(|| "not found (HTTP 404)".to_owned())
    }
}

/// The real index and the real files of `widget`, with the entry set to `commit`, `status`
/// and `tier` (the shipped index has placeholder commits). `edit` may change the entry.
pub fn world(
    widget: &str,
    commit: &str,
    status: &str,
    tier: &str,
    edit: impl FnOnce(&mut Value),
) -> Fake {
    let mut idx = registry_fixture();
    let id = id_of(widget);
    let e = idx["widgets"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|w| w["id"] == id)
        .unwrap();
    e["commit"] = json!(commit);
    e["status"] = json!(status);
    e["tier"] = json!(tier);
    edit(e);
    let repo = e["repo"]
        .as_str()
        .unwrap()
        .strip_prefix("https://github.com/")
        .unwrap()
        .to_owned();
    let mut files = BTreeMap::new();
    for f in ["manifest.json", "widget.json", "layout.json"] {
        files.insert(format!("/{repo}/{commit}/{f}"), fixture(widget, f));
    }
    Fake(Arc::new(Inner {
        index: serde_json::to_vec(&idx).unwrap(),
        files,
        log: Mutex::default(),
    }))
}

/// Replace what the source serves for one file of `widget` at `commit`.
pub fn with_file(f: &Fake, widget: &str, commit: &str, name: &str, bytes: Vec<u8>) -> Fake {
    let idx: Value = serde_json::from_slice(&f.0.index).unwrap();
    let id = id_of(widget);
    let repo = idx["widgets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == id)
        .unwrap()["repo"]
        .as_str()
        .unwrap()
        .strip_prefix("https://github.com/")
        .unwrap()
        .to_owned();
    let mut files = f.0.files.clone();
    files.insert(format!("/{repo}/{commit}/{name}"), bytes);
    Fake(Arc::new(Inner {
        index: f.0.index.clone(),
        files,
        log: Mutex::default(),
    }))
}

pub struct Script {
    pub interactive: bool,
    pub answers: VecDeque<String>,
    pub asked: Vec<String>,
}

impl Script {
    pub fn says(a: &[&str]) -> Script {
        Script {
            interactive: true,
            answers: a.iter().map(|s| (*s).to_owned()).collect(),
            asked: vec![],
        }
    }
    pub fn no_terminal() -> Script {
        Script {
            interactive: false,
            answers: VecDeque::new(),
            asked: vec![],
        }
    }
}

impl Ui for Script {
    fn interactive(&self) -> bool {
        self.interactive
    }
    fn ask(&mut self, prompt: &str) -> Result<String, String> {
        self.asked.push(prompt.to_owned());
        self.answers
            .pop_front()
            .ok_or_else(|| "no answer".to_owned())
    }
}

pub fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| (*s).to_owned()).collect()
}

pub struct Run {
    pub result: Result<(), String>,
    pub out: String,
}

pub fn install(paths: &Paths, src: &Fake, a: &[&str], ui: &mut Script) -> Run {
    let mut out = Vec::new();
    let s = src.clone();
    let result = ow::install::install(paths, &args(a), &mut out, &move |_| Box::new(s.clone()), ui);
    Run {
        result,
        out: String::from_utf8(out).unwrap(),
    }
}

pub fn remove(paths: &Paths, a: &[&str]) -> Run {
    let mut out = Vec::new();
    let result = ow::install::remove(paths, &args(a), &mut out);
    Run {
        result,
        out: String::from_utf8(out).unwrap(),
    }
}

pub fn list(paths: &Paths) -> String {
    let mut out = Vec::new();
    ow::install::list(paths, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

/// A private throwaway tree with the config dir ready (as the daemon would have it).
pub fn tree() -> (tempfile::TempDir, Paths) {
    let d = tempfile::tempdir().unwrap();
    let paths = Paths::under(d.path());
    (d, paths)
}

/// Every file and directory below `root` with mode and content hash, for before/after equality.
pub fn snapshot(root: &Path) -> BTreeMap<PathBuf, (u32, String)> {
    use std::os::unix::fs::PermissionsExt;
    let mut out = BTreeMap::new();
    fn go(p: &Path, root: &Path, out: &mut BTreeMap<PathBuf, (u32, String)>) {
        for e in std::fs::read_dir(p).unwrap() {
            let e = e.unwrap().path();
            let md = std::fs::symlink_metadata(&e).unwrap();
            let rel = e.strip_prefix(root).unwrap().to_owned();
            let mode = md.permissions().mode() & 0o7777;
            if md.is_dir() {
                out.insert(rel, (mode, "dir".into()));
                go(&e, root, out);
            } else if md.is_file() {
                out.insert(
                    rel,
                    (mode, ow::install::hash::sha256(&std::fs::read(&e).unwrap())),
                );
            } else {
                out.insert(rel, (mode, "link".into()));
            }
        }
    }
    go(root, root, &mut out);
    out
}

pub fn write_grants(paths: &Paths, text: &str, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(&paths.config).unwrap();
    std::fs::set_permissions(&paths.config, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(paths.grants(), text).unwrap();
    std::fs::set_permissions(paths.grants(), std::fs::Permissions::from_mode(mode)).unwrap();
}
