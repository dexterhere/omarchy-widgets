//! The registry index (`registry.json`): parsing, selecting one listing, and the status and tier
//! gate. Pure functions over bytes. The index is trusted to say which repo and commit an id
//! means (docs/INSTALL_DESIGN.md section 2); everything it says is still checked for shape.
use super::json;
use super::text::{full_sha, sha256_hex, show_value, store_id};
use serde_json::Value;
use std::collections::BTreeMap;

pub const FILES: [&str; 3] = ["manifest.json", "widget.json", "layout.json"];
const MAX_ENTRIES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Planned,
    Alpha,
    Stable,
    Removed,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Status::Planned => "planned",
            Status::Alpha => "alpha",
            Status::Stable => "stable",
            Status::Removed => "removed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Concept,
    Community,
    Spec,
    Verified,
    Core,
}

impl Tier {
    pub fn name(self) -> &'static str {
        match self {
            Tier::Concept => "concept",
            Tier::Community => "community",
            Tier::Spec => "spec",
            Tier::Verified => "verified",
            Tier::Core => "core",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub owner: String,
    pub name: String,
}

impl Repo {
    pub fn url(&self) -> String {
        format!("https://github.com/{}/{}", self.owner, self.name)
    }
}

#[derive(Debug, Clone)]
pub struct Listing {
    pub id: String,
    pub repo: Repo,
    pub commit: String,
    pub status: Status,
    pub tier: Tier,
    pub name: Option<String>,
    pub version: Option<String>,
    /// File name -> sha256 hex, when the index carries hashes.
    pub sha256: Option<BTreeMap<String, String>>,
    /// The index's own copy of what the website showed: compared with the fetched files.
    pub providers: Option<Value>,
    pub capabilities: Option<Value>,
    pub layout: Option<Value>,
}

/// `https://github.com/<owner>/<repo>` only. Owner: GitHub login characters; repo: letters,
/// digits, `.`, `_`, `-`, not `.`/`..`, no `.git` suffix (the raw host does not use it).
pub fn parse_repo(s: &str) -> Result<Repo, String> {
    let rest = s
        .strip_prefix("https://github.com/")
        .ok_or("repo must be https://github.com/<owner>/<repo>")?;
    let (owner, name) = rest
        .split_once('/')
        .ok_or("repo must name an owner and a repository")?;
    let owner_ok = !owner.is_empty()
        && owner.len() <= 39
        && !owner.starts_with('-')
        && owner
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-');
    let name_ok = !name.is_empty()
        && name.len() <= 100
        && name != "."
        && name != ".."
        && !name.ends_with(".git")
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'));
    if owner_ok && name_ok {
        Ok(Repo {
            owner: owner.into(),
            name: name.into(),
        })
    } else {
        Err("repo owner or name has characters GitHub does not allow".into())
    }
}

/// Parse the index and return the one entry for `id`. `id` must already satisfy `store_id`.
pub fn select_listing(bytes: &[u8], id: &str) -> Result<Listing, String> {
    if !store_id(id) {
        return Err("not a valid widget id".into());
    }
    let index = json::strict(bytes, json::MAX_DEPTH_INDEX)?;
    if index["schemaVersion"] != 1 {
        return Err("the registry index has a schemaVersion this ow does not understand".into());
    }
    let widgets = index["widgets"]
        .as_array()
        .ok_or("the registry index has no widget list")?;
    if widgets.len() > MAX_ENTRIES {
        return Err("the registry index is implausibly large".into());
    }
    let mut hits = widgets.iter().filter(|w| w["id"].as_str() == Some(id));
    let entry = hits.next().ok_or("this id is not in the registry")?;
    if hits.next().is_some() {
        return Err("the registry lists this id more than once; refusing".into());
    }
    listing(entry, id)
}

fn opt_str(e: &Value, k: &str) -> Result<Option<String>, String> {
    match e.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("registry entry: {k} must be a string")),
    }
}

fn listing(e: &Value, id: &str) -> Result<Listing, String> {
    let repo = parse_repo(e["repo"].as_str().ok_or("registry entry has no repo")?)?;
    // io.github.<owner>.<name> must come from a repo owned by <owner> (same rule as the build).
    if let Some(rest) = id.strip_prefix("io.github.") {
        let ns = rest.split('.').next().unwrap_or("");
        if !ns.eq_ignore_ascii_case(&repo.owner) {
            return Err("the id's GitHub namespace does not match the repo owner".into());
        }
    }
    let commit = e["commit"].as_str().ok_or("registry entry has no commit")?;
    if commit.bytes().all(|c| c == b'0') && commit.len() == 40 {
        return Err(
            "this entry has the all-zero placeholder commit (a concept entry): there are no files to fetch yet"
                .into(),
        );
    }
    if !full_sha(commit) {
        return Err(
            "commit must be the full 40-character lowercase SHA (short hashes are refused)".into(),
        );
    }
    let status = match e["status"].as_str() {
        Some("planned") => Status::Planned,
        Some("alpha") => Status::Alpha,
        Some("stable") => Status::Stable,
        Some("removed") => Status::Removed,
        other => {
            return Err(format!(
                "unknown status {}: refusing",
                show_value(other.unwrap_or("(missing)"))
            ));
        }
    };
    let tier = match e["tier"].as_str() {
        Some("concept") => Tier::Concept,
        Some("community") => Tier::Community,
        Some("spec") => Tier::Spec,
        Some("verified") => Tier::Verified,
        Some("core") => Tier::Core,
        other => {
            return Err(format!(
                "unknown tier {}: refusing",
                show_value(other.unwrap_or("(missing)"))
            ));
        }
    };
    let sha256 = match e.get("sha256") {
        None | Some(Value::Null) => None,
        Some(Value::Object(m)) => {
            let mut out = BTreeMap::new();
            for (k, v) in m {
                if !FILES.contains(&k.as_str()) {
                    return Err("registry entry: sha256 names an unknown file".into());
                }
                match v.as_str() {
                    Some(h) if sha256_hex(h) => out.insert(k.clone(), h.to_owned()),
                    _ => {
                        return Err(
                            "registry entry: sha256 values must be 64 lowercase hex digits".into(),
                        );
                    }
                };
            }
            Some(out)
        }
        Some(_) => return Err("registry entry: sha256 must be an object".into()),
    };
    Ok(Listing {
        id: id.to_owned(),
        repo,
        commit: commit.to_owned(),
        status,
        tier,
        name: opt_str(e, "name")?,
        version: opt_str(e, "version")?,
        sha256,
        providers: e.get("providers").cloned(),
        capabilities: e.get("capabilities").cloned(),
        layout: e.get("layout").cloned(),
    })
}

/// What the status allows. `Ok(true)` = installable only because `--allow-planned` was given,
/// so the caller must say the widget will not show data.
pub fn gate(l: &Listing, allow_planned: bool) -> Result<bool, String> {
    match l.status {
        Status::Removed => Err("this widget was removed (taken down); refusing even with --allow-planned".into()),
        Status::Planned if !allow_planned => Err(
            "this widget is only planned (a design concept) and will not show data; pass --allow-planned to install it anyway"
                .into(),
        ),
        Status::Planned => Ok(true),
        Status::Alpha | Status::Stable => Ok(false),
    }
}

/// The raw-host path of one file at the pinned commit. Built only from validated parts.
pub fn file_path(l: &Listing, file: &str) -> Result<String, String> {
    if !FILES.contains(&file) || !full_sha(&l.commit) {
        return Err("internal: refusing to build a path".into());
    }
    Ok(format!(
        "/{}/{}/{}/{}",
        l.repo.owner, l.repo.name, l.commit, file
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const C: &str = "0123456789abcdef0123456789abcdef01234567";

    fn entry(over: Value) -> Value {
        let mut e = json!({
            "id": "io.github.dexterhere.clock", "repo": "https://github.com/dexterhere/omarchy-widget-clock",
            "commit": C, "status": "alpha", "tier": "spec", "name": "Clock", "version": "0.1.0"
        });
        for (k, v) in over.as_object().cloned().unwrap_or_default() {
            if v.is_null() {
                e.as_object_mut().unwrap().remove(&k);
            } else {
                e[k] = v;
            }
        }
        e
    }

    fn index(entries: Vec<Value>) -> Vec<u8> {
        serde_json::to_vec(&json!({"schemaVersion": 1, "widgets": entries})).unwrap()
    }

    fn pick(over: Value) -> Result<Listing, String> {
        select_listing(&index(vec![entry(over)]), "io.github.dexterhere.clock")
    }

    #[test]
    fn selects_the_one_matching_entry() {
        let other = entry(json!({"id": "io.github.dexterhere.other"}));
        let l = select_listing(
            &index(vec![other, entry(json!({}))]),
            "io.github.dexterhere.clock",
        )
        .unwrap();
        assert_eq!(
            (l.commit.as_str(), l.status, l.tier),
            (C, Status::Alpha, Tier::Spec)
        );
        assert_eq!(
            l.repo.url(),
            "https://github.com/dexterhere/omarchy-widget-clock"
        );
        assert_eq!(
            file_path(&l, "widget.json").unwrap(),
            format!("/dexterhere/omarchy-widget-clock/{C}/widget.json")
        );
    }

    #[test]
    fn missing_duplicate_and_bad_requested_ids() {
        let e = entry(json!({}));
        assert!(
            select_listing(&index(vec![]), "io.github.dexterhere.clock")
                .unwrap_err()
                .contains("not in the registry")
        );
        assert!(
            select_listing(&index(vec![e.clone(), e]), "io.github.dexterhere.clock")
                .unwrap_err()
                .contains("more than once")
        );
        for bad in ["../x", "a/b", "", ".hidden", "A.b", "a..b"] {
            assert!(
                select_listing(&index(vec![]), bad)
                    .unwrap_err()
                    .contains("valid widget id"),
                "{bad}"
            );
        }
    }

    #[test]
    fn commit_must_be_the_full_lowercase_sha() {
        for bad in [
            "0123456",
            &C[..39],
            &C.to_uppercase(),
            &format!("{C}0"),
            "HEAD",
            "main",
            "",
            &C.replace('0', "g"),
        ] {
            let e = pick(json!({"commit": bad})).unwrap_err();
            assert!(e.contains("40-character"), "{bad}: {e}");
        }
        assert!(
            pick(json!({"commit": null}))
                .unwrap_err()
                .contains("no commit")
        );
        assert!(
            pick(json!({"commit": "0".repeat(40)}))
                .unwrap_err()
                .contains("placeholder")
        );
        assert!(pick(json!({"commit": 5})).is_err());
    }

    #[test]
    fn a_changed_commit_changes_every_fetch_path() {
        let a = pick(json!({})).unwrap();
        let b = pick(json!({"commit": "f".repeat(40)})).unwrap();
        for f in FILES {
            let (pa, pb) = (file_path(&a, f).unwrap(), file_path(&b, f).unwrap());
            assert_ne!(pa, pb);
            assert!(pa.contains(C) && pb.contains(&"f".repeat(40)));
        }
    }

    #[test]
    fn unknown_status_and_tier_are_refused() {
        for s in ["Alpha", "beta", "", "alpha ", "ALPHA", "planned\n"] {
            assert!(
                pick(json!({"status": s}))
                    .unwrap_err()
                    .contains("unknown status"),
                "{s:?}"
            );
        }
        assert!(
            pick(json!({"status": null}))
                .unwrap_err()
                .contains("unknown status")
        );
        assert!(
            pick(json!({"status": 3}))
                .unwrap_err()
                .contains("unknown status")
        );
        for t in ["code", "Spec", "official", "", "core "] {
            assert!(
                pick(json!({"tier": t}))
                    .unwrap_err()
                    .contains("unknown tier"),
                "{t:?}"
            );
        }
        assert!(
            pick(json!({"tier": null}))
                .unwrap_err()
                .contains("unknown tier")
        );
        // The refusal text never echoes more than a short, sanitized value.
        let e = pick(json!({"status": "x\u{1b}[31m".repeat(50)})).unwrap_err();
        assert!(!e.contains('\u{1b}') && e.len() < 120, "{e}");
        for t in ["concept", "community", "spec", "verified", "core"] {
            assert!(pick(json!({"tier": t})).is_ok());
        }
    }

    #[test]
    fn status_gate() {
        let l = |s| pick(json!({"status": s})).unwrap();
        assert_eq!(gate(&l("alpha"), false), Ok(false));
        assert_eq!(gate(&l("stable"), false), Ok(false));
        assert!(
            gate(&l("planned"), false)
                .unwrap_err()
                .contains("--allow-planned")
        );
        assert_eq!(gate(&l("planned"), true), Ok(true));
        // A takedown is not lifted by the flag.
        assert!(gate(&l("removed"), false).is_err());
        assert!(gate(&l("removed"), true).unwrap_err().contains("removed"));
    }

    #[test]
    fn repo_must_be_a_plain_github_repo() {
        for bad in [
            "http://github.com/a/b",
            "https://github.com/a",
            "https://github.com/a/b/c",
            "https://github.com/a/b?x=1",
            "https://github.com/a/b#f",
            "https://github.com/a/..",
            "https://github.com/../b",
            "https://github.com/a/b.git",
            "https://evil.com/a/b",
            "https://github.com.evil.com/a/b",
            "https://user@github.com/a/b",
            "https://github.com:443/a/b",
            "https://github.com/-a/b",
            "https://github.com/a b/c",
            "https://github.com//b",
            "https://github.com/a/b%2f",
            "https://github.com/a/b\n",
            "git@github.com:a/b",
            "https://GITHUB.com/a/b",
            "",
        ] {
            assert!(parse_repo(bad).is_err(), "{bad:?}");
        }
        assert!(parse_repo("https://github.com/Dexter-Here/omarchy_widget.clock").is_ok());
        assert!(pick(json!({"repo": 3})).is_err());
    }

    #[test]
    fn github_namespace_must_match_the_repo_owner() {
        let e = pick(json!({"repo": "https://github.com/mallory/clock"})).unwrap_err();
        assert!(e.contains("namespace"), "{e}");
        assert!(pick(json!({"repo": "https://github.com/DexterHere/clock"})).is_ok());
        // Other namespaces are the registry's decision (namespaces.json).
        let e =
            entry(json!({"id": "org.example.clock", "repo": "https://github.com/mallory/clock"}));
        assert!(select_listing(&index(vec![e]), "org.example.clock").is_ok());
    }

    #[test]
    fn hashes_are_optional_but_strict_when_present() {
        assert!(pick(json!({})).unwrap().sha256.is_none());
        let h = "ab".repeat(32);
        let ok = pick(json!({"sha256": {"manifest.json": h, "widget.json": h, "layout.json": h}}))
            .unwrap();
        assert_eq!(ok.sha256.unwrap().len(), 3);
        for bad in [
            json!({"manifest.json": "AB".repeat(32)}),
            json!({"manifest.json": "ab"}),
            json!({"../etc/passwd": h}),
            json!({"README.md": h}),
            json!("x"),
            json!([h]),
            json!({"manifest.json": 1}),
        ] {
            assert!(pick(json!({"sha256": bad})).is_err());
        }
    }

    #[test]
    fn hostile_indexes() {
        let id = "io.github.dexterhere.clock";
        for bytes in [
            b"{}".to_vec(),
            b"[]".to_vec(),
            b"".to_vec(),
            br#"{"schemaVersion":2,"widgets":[]}"#.to_vec(),
            br#"{"schemaVersion":1}"#.to_vec(),
            br#"{"schemaVersion":1,"widgets":{}}"#.to_vec(),
            br#"{"schemaVersion":1,"schemaVersion":1,"widgets":[]}"#.to_vec(),
            b"\xef\xbb\xbf{\"schemaVersion\":1,\"widgets\":[]}".to_vec(),
        ] {
            assert!(select_listing(&bytes, id).is_err());
        }
        // Duplicate key inside an entry the caller did not ask for still poisons the index.
        let dup = format!(
            r#"{{"schemaVersion":1,"widgets":[{{"id":"a.b","id":"c.d"}},{}]}}"#,
            entry(json!({}))
        );
        assert!(
            select_listing(dup.as_bytes(), id)
                .unwrap_err()
                .contains("duplicate")
        );
        // Deep nesting.
        let deep = format!(
            r#"{{"schemaVersion":1,"widgets":[{}1{}]}}"#,
            "[".repeat(200),
            "]".repeat(200)
        );
        assert!(select_listing(deep.as_bytes(), id).is_err());
    }
}
