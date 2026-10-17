//! Per-widget topic grants. Deny by default; see docs/DAEMON_SPEC.md section 6.
//!
//! The widget id is claimed by the client and is not authenticated: for code widgets
//! this is a guardrail for honest code, not a barrier.
use serde::Deserialize;
use serde_json::{Map, Value};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub widget: String,
    pub topics: Vec<String>,
    /// Top-level keys only: an allowed key returns its whole subtree, so providers must
    /// not nest data of mixed sensitivity under one key.
    /// `None` = every field of the topic; `Some([])` = no fields.
    pub fields: Option<Vec<String>>,
    pub approved_at: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    grant: Vec<Grant>,
}

#[derive(Debug, Default, Clone)]
pub struct Grants(Vec<Grant>);

#[derive(Debug, thiserror::Error)]
pub enum GrantsError {
    #[error("grants file is accessible by group/others or not owned by us")]
    Insecure,
    #[error("grants file is invalid")]
    Invalid,
    #[error("cannot read grants file")]
    Io,
    #[error("the same widget and topic are granted twice")]
    Duplicate,
}

impl Grants {
    /// Fails closed: an unreadable or malformed file is an error, never "allow all".
    pub fn parse(text: &str) -> Result<Self, GrantsError> {
        let f: File = toml::from_str(text).map_err(|_| GrantsError::Invalid)?;
        // Ambiguous duplicates are rejected rather than resolved: first-match-wins could
        // silently narrow or widen a grant.
        let mut seen = std::collections::HashSet::new();
        for g in &f.grant {
            for t in &g.topics {
                if !seen.insert((g.widget.as_str(), t.as_str())) {
                    return Err(GrantsError::Duplicate);
                }
            }
        }
        Ok(Grants(f.grant))
    }

    /// Missing file = no grants. The file must be a regular file we own with no
    /// group/other access. Opened with O_NOFOLLOW|O_NONBLOCK and checked on the open
    /// descriptor, so a swapped path or a FIFO cannot bypass the check or hang startup.
    pub fn load(path: &Path) -> Result<Self, GrantsError> {
        use std::io::Read;
        use std::os::unix::fs::OpenOptionsExt;
        let flags = rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK;
        let f = match std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(flags.bits() as i32)
            .open(path)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(_) => return Err(GrantsError::Io),
        };
        let md = f.metadata().map_err(|_| GrantsError::Io)?;
        if !md.is_file()
            || md.mode() & 0o077 != 0
            || md.uid() != rustix::process::geteuid().as_raw()
        {
            return Err(GrantsError::Insecure);
        }
        let mut text = String::new();
        f.take(1024 * 1024)
            .read_to_string(&mut text)
            .map_err(|_| GrantsError::Io)?;
        Self::parse(&text)
    }

    /// The grant covering `(widget, topic)`, if any.
    pub fn find(&self, widget: &str, topic: &str) -> Option<&Grant> {
        self.0
            .iter()
            .find(|g| g.widget == widget && g.topics.iter().any(|t| t == topic))
    }
}

impl Grant {
    /// Drop every top-level field the grant does not cover.
    pub fn filter(&self, mut data: Map<String, Value>) -> Map<String, Value> {
        if let Some(allowed) = &self.fields {
            data.retain(|k, _| allowed.iter().any(|f| f == k));
        }
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;

    const T: &str = r#"
[[grant]]
widget = "w1"
topics = ["github.prs"]
fields = ["open"]
[[grant]]
widget = "w2"
topics = ["system.cpu"]
"#;

    #[test]
    fn deny_by_default_and_exact_match() {
        let g = Grants::parse(T).unwrap();
        assert!(g.find("w1", "github.prs").is_some());
        assert!(g.find("w1", "system.cpu").is_none());
        assert!(g.find("w3", "github.prs").is_none());
        assert!(Grants::default().find("w1", "github.prs").is_none());
    }

    #[test]
    fn duplicate_grants_are_rejected() {
        let t =
            "[[grant]]\nwidget=\"w\"\ntopics=[\"t\"]\n[[grant]]\nwidget=\"w\"\ntopics=[\"t\"]\n";
        assert!(matches!(Grants::parse(t), Err(GrantsError::Duplicate)));
    }

    #[test]
    fn symlink_and_fifo_are_refused() {
        let d = tempfile::tempdir().unwrap();
        let real = d.path().join("real.toml");
        std::fs::write(&real, T).unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = d.path().join("grants.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(Grants::load(&link).is_err());
        let fifo = d.path().join("fifo.toml");
        rustix::fs::mknodat(
            rustix::fs::CWD,
            &fifo,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(0o600),
            0,
        )
        .unwrap();
        assert!(Grants::load(&fifo).is_err());
    }

    #[test]
    fn field_filtering() {
        let g = Grants::parse(T).unwrap();
        let data = json!({"open":3,"review":1}).as_object().cloned().unwrap();
        let f = g.find("w1", "github.prs").unwrap().filter(data.clone());
        assert_eq!(Value::Object(f), json!({"open":3}));
        let all = g.find("w2", "system.cpu").unwrap().filter(data.clone());
        assert_eq!(all, data);
    }

    #[test]
    fn empty_fields_list_grants_nothing() {
        let g = Grants::parse("[[grant]]\nwidget=\"w\"\ntopics=[\"t\"]\nfields=[]\n").unwrap();
        let out = g
            .find("w", "t")
            .unwrap()
            .filter(json!({"a":1}).as_object().cloned().unwrap());
        assert!(out.is_empty());
    }

    #[test]
    fn malformed_or_typo_fails_closed() {
        assert!(Grants::parse("not toml [").is_err());
        assert!(Grants::parse("[[grant]]\nwidget=\"w\"\ntopcs=[\"t\"]\n").is_err());
        assert!(Grants::parse("[[grnt]]\nwidget=\"w\"\n").is_err());
    }

    #[test]
    fn load_checks_mode_and_missing_is_empty() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("grants.toml");
        assert!(Grants::load(&p).unwrap().find("w1", "x").is_none());
        std::fs::write(&p, T).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(Grants::load(&p), Err(GrantsError::Insecure)));
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(Grants::load(&p).unwrap().find("w1", "github.prs").is_some());
    }
}
