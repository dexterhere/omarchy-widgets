//! `$XDG_CONFIG_HOME/owd/config.toml`: the provider-to-secret binding (docs/AUTH_PLAN.md 5.2
//! item 3, DECISIONS GITHUB-PROVIDER).
//!
//! ```toml
//! [providers.github]
//! secret = "github-main"      # vault secret name; must match the provider's pattern
//! interval_secs = 120         # optional; below 60 is raised to 60, default 120
//! ```
//!
//! This file is the ONLY way a provider learns which vault secret to use. A name pattern per
//! provider is code (below), not configuration, so editing the file cannot point a provider at
//! a secret meant for something else (`gmail-app-pw` is refused for `github`). Loaded like
//! `grants.toml`: mode 0600 and ours, no symlink, size capped, unknown keys rejected, any error
//! is an error (the daemon does not start), a missing file means no providers are configured.
//! Errors say which rule failed, never what the file contained.
use crate::private_file::{ReadError, read_private};
use serde::Deserialize;
use std::path::Path;
use std::time::Duration;

/// Largest config file read.
pub const MAX_CONFIG_FILE: u64 = 64 * 1024;
/// Polling floor, whatever the file says (GitHub search limits, docs/AUTH_PLAN.md 3.6).
pub const MIN_INTERVAL_SECS: u64 = 60;
pub const DEFAULT_INTERVAL_SECS: u64 = 120;
/// Ceiling so a huge value cannot overflow the hub's backoff arithmetic.
pub const MAX_INTERVAL_SECS: u64 = 3_600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("config file is accessible by group/others or not owned by us")]
    Insecure,
    #[error("config file is invalid")]
    Invalid,
    #[error("config file is larger than the 64 KiB limit")]
    TooLarge,
    #[error("cannot read config file")]
    Io,
    #[error("secret name is not allowed for this provider")]
    SecretName,
}

/// Is `name` a vault secret name this provider may be bound to? Patterns are code. Unknown
/// providers have none, so nothing can be bound to them.
pub fn secret_name_allowed(provider: &str, name: &str) -> bool {
    match provider {
        // ^github-[a-z0-9-]{1,40}$
        "github" => name.strip_prefix("github-").is_some_and(|rest| {
            (1..=40).contains(&rest.len())
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        }),
        _ => false,
    }
}

/// A validated binding. Only this module can build one, so a provider constructed from it
/// cannot hold a secret name outside its pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderBinding {
    secret: String,
    interval: Duration,
}

impl ProviderBinding {
    pub fn secret(&self) -> &str {
        &self.secret
    }

    /// Already clamped to `MIN_INTERVAL_SECS..=MAX_INTERVAL_SECS`.
    pub fn interval(&self) -> Duration {
        self.interval
    }

    fn new(provider: &str, raw: RawProvider) -> Result<Self, ConfigError> {
        if !secret_name_allowed(provider, &raw.secret) {
            return Err(ConfigError::SecretName);
        }
        let secs = raw
            .interval_secs
            .unwrap_or(DEFAULT_INTERVAL_SECS)
            .clamp(MIN_INTERVAL_SECS, MAX_INTERVAL_SECS);
        Ok(Self {
            secret: raw.secret,
            interval: Duration::from_secs(secs),
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvider {
    secret: String,
    interval_secs: Option<u64>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProviders {
    github: Option<RawProvider>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    #[serde(default)]
    providers: RawProviders,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Config {
    github: Option<ProviderBinding>,
}

impl Config {
    /// A duplicate `[providers.github]` section or key is a TOML error, hence `Invalid`.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let raw: RawFile = toml::from_str(text).map_err(|_| ConfigError::Invalid)?;
        let github = raw
            .providers
            .github
            .map(|r| ProviderBinding::new("github", r))
            .transpose()?;
        Ok(Config { github })
    }

    /// Missing file = no providers configured.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match read_private(path, MAX_CONFIG_FILE) {
            Ok(Some(text)) => Self::parse(&text),
            Ok(None) => Ok(Self::default()),
            Err(ReadError::Insecure) => Err(ConfigError::Insecure),
            Err(ReadError::TooLarge) => Err(ConfigError::TooLarge),
            Err(ReadError::Io) => Err(ConfigError::Io),
        }
    }

    pub fn github(&self) -> Option<&ProviderBinding> {
        self.github.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const OK: &str = "[providers.github]\nsecret = \"github-main\"\n";

    fn write(dir: &Path, text: &str, mode: u32) -> std::path::PathBuf {
        let p = dir.join("config.toml");
        std::fs::write(&p, text).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    #[test]
    fn minimal_file_gets_defaults() {
        let g = Config::parse(OK).unwrap();
        let g = g.github().unwrap();
        assert_eq!(g.secret(), "github-main");
        assert_eq!(g.interval(), Duration::from_secs(120));
    }

    #[test]
    fn empty_file_and_missing_file_configure_nothing() {
        assert!(Config::parse("").unwrap().github().is_none());
        let d = tempfile::tempdir().unwrap();
        let none = Config::load(&d.path().join("config.toml")).unwrap();
        assert!(none.github().is_none());
    }

    #[test]
    fn interval_is_floored_defaulted_and_capped() {
        let iv = |s: &str| {
            let t = format!("{OK}interval_secs = {s}\n");
            Config::parse(&t).map(|c| c.github().unwrap().interval().as_secs())
        };
        assert_eq!(iv("1"), Ok(60));
        assert_eq!(iv("0"), Ok(60));
        assert_eq!(iv("59"), Ok(60));
        assert_eq!(iv("60"), Ok(60));
        assert_eq!(iv("300"), Ok(300));
        assert_eq!(iv("18446744073709551615"), Ok(MAX_INTERVAL_SECS));
        // Not a non-negative integer: refused, not coerced.
        for bad in [
            "-5",
            "1.5",
            "\"120\"",
            "true",
            "[1]",
            "18446744073709551616",
        ] {
            assert_eq!(iv(bad), Err(ConfigError::Invalid), "{bad}");
        }
    }

    #[test]
    fn secret_names_must_match_the_providers_pattern() {
        let name = |n: &str| Config::parse(&format!("[providers.github]\nsecret = \"{n}\"\n"));
        for good in [
            "github-main",
            "github-a",
            "github-0",
            "github--",
            &format!("github-{}", "a".repeat(40)),
        ] {
            assert!(name(good).is_ok(), "{good}");
        }
        for bad in [
            "gmail-app-pw", // a secret meant for something else
            "github-",
            "github",
            "Github-main",
            "github-Main",
            "github-main ",
            "github-ma_in",
            "github-ma.in",
            "github-mäin",
            "xgithub-main",
            "",
            &format!("github-{}", "a".repeat(41)),
        ] {
            assert_eq!(name(bad).err(), Some(ConfigError::SecretName), "{bad:?}");
        }
        // Escapes cannot smuggle a newline or traversal past the pattern either.
        assert!(name("github-a\\u000Ab").is_err());
        assert!(!secret_name_allowed("github", "github-main\n"));
        assert!(!secret_name_allowed("gmail", "gmail-main"));
        assert!(!secret_name_allowed("", "github-main"));
    }

    #[test]
    fn unknown_keys_sections_and_providers_are_rejected() {
        for bad in [
            "[providers.github]\nsecret = \"github-a\"\nsecrt = 1\n",
            "[providers.github]\nsecret = \"github-a\"\ntoken = \"x\"\n",
            "[providers.gmail]\nsecret = \"gmail-a\"\n",
            "[provider.github]\nsecret = \"github-a\"\n",
            "[extra]\nx = 1\n",
            "top = 1\n",
            "[providers]\nx = 1\n",
            "[providers.github]\n",
            "[providers.github]\nsecret = 5\n",
            "[providers.github]\nsecret = [\"github-a\"]\n",
            "not toml [",
        ] {
            assert_eq!(
                Config::parse(bad).err(),
                Some(ConfigError::Invalid),
                "{bad}"
            );
        }
    }

    #[test]
    fn duplicate_sections_and_keys_are_rejected() {
        let dup_section = format!("{OK}[providers.github]\nsecret = \"github-other\"\n");
        let dup_key = "[providers.github]\nsecret = \"github-a\"\nsecret = \"github-b\"\n";
        let dup_dotted = format!("{OK}providers.github.secret = \"github-x\"\n");
        for bad in [dup_section.as_str(), dup_key, dup_dotted.as_str()] {
            assert!(Config::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn load_requires_a_private_regular_file_we_own() {
        let d = tempfile::tempdir().unwrap();
        for mode in [0o644, 0o640, 0o604, 0o666, 0o660] {
            let p = write(d.path(), OK, mode);
            assert_eq!(
                Config::load(&p).err(),
                Some(ConfigError::Insecure),
                "{mode:o}"
            );
        }
        let p = write(d.path(), OK, 0o600);
        assert!(Config::load(&p).unwrap().github().is_some());
        let p = write(d.path(), OK, 0o400);
        assert!(Config::load(&p).unwrap().github().is_some());
    }

    #[test]
    fn world_writable_file_is_refused_even_if_valid() {
        let d = tempfile::tempdir().unwrap();
        let p = write(d.path(), OK, 0o666);
        assert_eq!(Config::load(&p).err(), Some(ConfigError::Insecure));
    }

    #[test]
    fn symlink_fifo_and_directory_are_refused() {
        let d = tempfile::tempdir().unwrap();
        let real = write(d.path(), OK, 0o600);
        let link = d.path().join("link.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(Config::load(&link).is_err());
        let fifo = d.path().join("fifo.toml");
        rustix::fs::mknodat(
            rustix::fs::CWD,
            &fifo,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(0o600),
            0,
        )
        .unwrap();
        assert!(Config::load(&fifo).is_err());
        assert!(Config::load(d.path()).is_err());
    }

    #[test]
    fn oversize_and_non_utf8_files_are_refused() {
        let d = tempfile::tempdir().unwrap();
        let big = format!("{OK}#{}\n", "x".repeat(MAX_CONFIG_FILE as usize));
        let p = write(d.path(), &big, 0o600);
        assert_eq!(Config::load(&p).err(), Some(ConfigError::TooLarge));
        // Exactly at the cap is fine.
        let pad = MAX_CONFIG_FILE as usize - OK.len() - 2;
        let at_cap = format!("{OK}#{}\n", "x".repeat(pad));
        assert_eq!(at_cap.len() as u64, MAX_CONFIG_FILE);
        let p = write(d.path(), &at_cap, 0o600);
        assert!(Config::load(&p).is_ok());
        let p = d.path().join("config.toml");
        std::fs::write(&p, b"[providers.github]\nsecret = \"github-\xff\"\n").unwrap();
        assert!(Config::load(&p).is_err());
    }

    /// Error text names the rule, never the content (a mistyped secret is still a secret).
    #[test]
    fn errors_do_not_echo_the_file() {
        let t = "[providers.github]\nsecret = \"FAKE-pat-0123456789abcdef\"\n";
        let e = Config::parse(t).unwrap_err();
        assert!(!format!("{e} {e:?}").contains("FAKE"));
        let t = "[providers.github]\nsecret = \"github-a\"\nFAKE-pat-0123456789abcdef = 1\n";
        let e = Config::parse(t).unwrap_err();
        assert!(!format!("{e} {e:?}").contains("FAKE"));
    }
}
