use std::path::{Path, PathBuf};

/// Every path the daemon uses, derived from XDG variables so tests and dev runs can
/// point it at a throwaway directory.
#[derive(Debug, Clone)]
pub struct Paths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
    pub runtime: PathBuf,
}

impl Paths {
    /// Everything under one directory (`--config-dir`, tests). Never touches real XDG dirs.
    pub fn under(root: &Path) -> Self {
        Paths {
            config: root.join("config/owd"),
            data: root.join("data/owd"),
            state: root.join("state/owd"),
            runtime: root.join("run/owd"),
        }
    }

    /// From the environment. Relative values are ignored per the XDG spec.
    /// `XDG_RUNTIME_DIR` is required: there is no insecure fallback such as /tmp.
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|k| std::env::var_os(k).map(PathBuf::from))
    }

    fn from_lookup(get: impl Fn(&str) -> Option<PathBuf>) -> Result<Self, String> {
        let abs = |k: &str| get(k).filter(|p| p.is_absolute());
        let home = abs("HOME");
        let pick = |k: &str, rel: &str| {
            abs(k)
                .or_else(|| home.as_ref().map(|h| h.join(rel)))
                .ok_or_else(|| format!("{k} and HOME are unset or not absolute"))
        };
        let runtime = abs("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR is unset or not absolute")?;
        Ok(Paths {
            config: pick("XDG_CONFIG_HOME", ".config")?.join("owd"),
            data: pick("XDG_DATA_HOME", ".local/share")?.join("owd"),
            state: pick("XDG_STATE_HOME", ".local/state")?.join("owd"),
            runtime: runtime.join("owd"),
        })
    }

    pub fn grants(&self) -> PathBuf {
        self.config.join("grants.toml")
    }

    pub fn vault(&self) -> PathBuf {
        self.data.join("vault.owv")
    }

    pub fn audit(&self) -> PathBuf {
        self.state.join("audit.jsonl")
    }

    pub fn socket(&self) -> PathBuf {
        self.runtime.join("owd.sock")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_dir_is_required_and_relative_is_ignored() {
        let none = |_: &str| None;
        assert!(Paths::from_lookup(none).is_err());
        let rel = |k: &str| (k == "XDG_RUNTIME_DIR").then(|| PathBuf::from("run"));
        assert!(Paths::from_lookup(rel).is_err());
    }

    #[test]
    fn defaults_from_home() {
        let env = |k: &str| match k {
            "HOME" => Some(PathBuf::from("/h")),
            "XDG_RUNTIME_DIR" => Some(PathBuf::from("/run/user/1")),
            _ => None,
        };
        let p = Paths::from_lookup(env).unwrap();
        assert_eq!(p.data, Path::new("/h/.local/share/owd"));
        assert_eq!(p.socket(), Path::new("/run/user/1/owd/owd.sock"));
    }
}
