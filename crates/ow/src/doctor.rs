//! `ow doctor`: read-only diagnostics. It looks at files and asks the running daemon two
//! questions (`status`, `vault.status`) through the same trusted client as every other verb. It
//! writes nothing, never runs `systemctl` or any program, never touches the network, and never
//! reads the vault file or the grants' secrets (there are none). Every path is injected through
//! `Env`, so tests run against a temp directory.
use crate::daemon;
use crate::install::{self, catalog, hash, index, store, text::show, validate};
use ow_core::{Grants, GrantsError, Paths};
use serde_json::{Value, json};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

/// Folder name the bridge plugin gets under Omarchy's plugin directory (the manifest id).
pub const PLUGIN_ID: &str = "io.github.dexterhere.widget-shell";
const UNIT_NAME: &str = "owd.service";
/// A unit file bigger than this is not ours.
const UNIT_CAP: usize = 64 * 1024;
/// At most this many widget directories are examined (a hostile directory cannot stall us).
const MAX_WIDGETS: usize = 500;
/// A mute daemon must not stall the report: each of the two questions waits this long at most.
pub const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Fail,
    /// Not checked, with the reason in the message. Never makes the exit code non-zero.
    Skip,
}

impl Level {
    fn label(self) -> &'static str {
        match self {
            Level::Ok => "OK",
            Level::Warn => "WARN",
            Level::Fail => "FAIL",
            Level::Skip => "SKIP",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Check {
    pub id: String,
    pub level: Level,
    pub msg: String,
    pub fix: Option<String>,
}

fn c(id: &str, level: Level, msg: impl Into<String>, fix: Option<String>) -> Check {
    Check {
        id: id.into(),
        level,
        msg: msg.into(),
        fix,
    }
}

/// Everything `run` looks at. Tests build this by hand.
pub struct Env {
    pub paths: Paths,
    /// The user id that must own everything.
    pub uid: u32,
    /// Directories searched for `owd.service`, most specific first. The first one is the user's
    /// own (`~/.config/systemd/user`); the others are where a package or an admin puts it.
    pub unit_dirs: Vec<PathBuf>,
    /// Omarchy's plugin directory, or `None` when it cannot be justified (reported as SKIP).
    pub plugin_dir: Option<PathBuf>,
    pub ow_version: String,
    /// `$HOME`, used to expand `%h` in the unit.
    pub home: Option<PathBuf>,
    /// Print vault secret names (`--names`). Off by default: reports get pasted.
    pub names: bool,
    /// Areas ("data", "config", "state", "runtime") whose location could not be determined, with
    /// the reason. Their checks are reported as SKIP.
    pub unknown: Vec<(&'static str, String)>,
}

impl Env {
    fn known(&self, area: &str) -> bool {
        !self.unknown.iter().any(|(a, _)| *a == area)
    }

    /// The process environment, tolerating missing or relative variables: what cannot be located
    /// is skipped with the reason instead of ending the run.
    pub fn from_lookup(get: impl Fn(&str) -> Option<PathBuf>) -> Env {
        let abs = |k: &str| get(k).filter(|p| p.is_absolute());
        let home = abs("HOME");
        let mut unknown = Vec::new();
        let mut area = |name: &'static str, var: &str, rel: &str| {
            let dir = abs(var).or_else(|| home.as_ref().map(|h| h.join(rel)));
            if dir.is_none() {
                unknown.push((name, format!("{var} and HOME are unset or not absolute")));
            }
            dir
        };
        let config = area("config", "XDG_CONFIG_HOME", ".config");
        let data = area("data", "XDG_DATA_HOME", ".local/share");
        let state = area("state", "XDG_STATE_HOME", ".local/state");
        let runtime = abs("XDG_RUNTIME_DIR");
        if runtime.is_none() {
            unknown.push(("runtime", "XDG_RUNTIME_DIR is unset or not absolute".into()));
        }
        let sub = |d: &Option<PathBuf>| d.as_ref().map_or_else(PathBuf::new, |d| d.join("owd"));
        let paths = Paths {
            config: sub(&config),
            data: sub(&data),
            state: sub(&state),
            runtime: sub(&runtime),
        };
        let mut e = Env::new(paths, false, home);
        e.unit_dirs = config
            .map(|c| c.join("systemd/user"))
            .into_iter()
            .chain(["/etc/systemd/user".into(), "/usr/lib/systemd/user".into()])
            .collect();
        e.unknown = unknown;
        e
    }

    /// The real environment. `--config-dir` (a sandbox) looks only inside the sandbox.
    pub fn new(paths: Paths, sandbox: bool, home: Option<PathBuf>) -> Env {
        let config_home = paths
            .config
            .parent()
            .map_or_else(|| paths.config.clone(), Path::to_path_buf);
        let mut unit_dirs = vec![config_home.join("systemd/user")];
        if !sandbox {
            unit_dirs.push("/etc/systemd/user".into());
            unit_dirs.push("/usr/lib/systemd/user".into());
        }
        Env {
            paths,
            uid: rustix::process::geteuid().as_raw(),
            unit_dirs,
            // docs/SPIKES.md S5: Omarchy 4.0.6 watches `~/.config/omarchy/plugins` only. Whether it
            // follows XDG_CONFIG_HOME is not verified, so the path is built from HOME alone.
            plugin_dir: if sandbox {
                None
            } else {
                home.as_ref().map(|h| h.join(".config/omarchy/plugins"))
            },
            ow_version: env!("CARGO_PKG_VERSION").into(),
            home,
            names: false,
            unknown: Vec::new(),
        }
    }
}

/// `cmd 'path'` for a person to paste: the path is POSIX single-quoted. A path with control
/// characters or invalid UTF-8 gets no command at all (nothing risky is printed).
pub fn cmd_path(cmd: &str, path: &Path) -> Option<String> {
    let t = path.to_str()?;
    if t.chars().any(char::is_control) {
        return None;
    }
    Some(format!("{cmd} '{}'", t.replace('\'', "'\\''")))
}

fn p(path: &Path) -> String {
    show(&path.display().to_string())
}

// ---- directories and files ----------------------------------------------------------------

pub enum Node {
    Missing,
    /// A symlink, or anything that is not what was expected.
    Wrong(&'static str),
    Found {
        mode: u32,
        uid: u32,
    },
}

/// Look at `path` without following a final symlink. `want_dir` selects directory or regular file.
pub fn inspect(path: &Path, want_dir: bool) -> Node {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Node::Missing,
        Err(_) => Node::Wrong("cannot be read"),
        Ok(m) if m.file_type().is_symlink() => Node::Wrong("is a symbolic link"),
        Ok(m) if want_dir && !m.is_dir() => Node::Wrong("is not a directory"),
        Ok(m) if !want_dir && !m.is_file() => Node::Wrong("is not a regular file"),
        Ok(m) => Node::Found {
            mode: m.mode() & 0o7777,
            uid: m.uid(),
        },
    }
}

/// A directory that must be 0700 and ours. `missing` is the verdict when it does not exist yet.
pub fn check_private_dir(
    id: &str,
    what: &str,
    path: &Path,
    uid: u32,
    missing: (Level, &str),
) -> Check {
    let mk = cmd_path("install -d -m 0700", path);
    match inspect(path, true) {
        Node::Missing => c(
            id,
            missing.0,
            format!("{what} {} does not exist yet. {}", p(path), missing.1),
            mk,
        ),
        Node::Wrong(why) => c(
            id,
            Level::Fail,
            format!("{what} {} {why}; it must be a real directory.", p(path)),
            Some("move it away, then create it again".into()),
        ),
        Node::Found { uid: u, .. } if u != uid => c(
            id,
            Level::Fail,
            format!("{what} {} is owned by another user.", p(path)),
            Some("it must belong to you; do not use this directory".into()),
        ),
        Node::Found { mode, .. } if mode != 0o700 => c(
            id,
            Level::Fail,
            format!("{what} {} has mode {mode:04o}; it must be 0700.", p(path)),
            cmd_path("chmod 700", path),
        ),
        Node::Found { .. } => c(
            id,
            Level::Ok,
            format!("{what} {} is private (0700) and yours.", p(path)),
            None,
        ),
    }
}

/// The vault file: present, regular, yours, no group/other access. Its content is never read.
pub fn check_vault_file(path: &Path, uid: u32) -> Check {
    match inspect(path, false) {
        Node::Missing => c(
            "vault-file",
            Level::Warn,
            format!(
                "No vault yet at {}. Widgets without secrets still work.",
                p(path)
            ),
            Some("ow vault init".into()),
        ),
        Node::Wrong(why) => c(
            "vault-file",
            Level::Fail,
            format!("Vault {} {why}.", p(path)),
            Some("move it away and restore a backup, or run: ow vault init".into()),
        ),
        Node::Found { uid: u, .. } if u != uid => c(
            "vault-file",
            Level::Fail,
            format!("Vault {} is owned by another user.", p(path)),
            Some("do not use it".into()),
        ),
        Node::Found { mode, .. } if mode & 0o077 != 0 => c(
            "vault-file",
            Level::Fail,
            format!(
                "Vault {} has mode {mode:04o}; group or others can access it.",
                p(path)
            ),
            cmd_path("chmod 600", path),
        ),
        Node::Found { mode, .. } if mode != 0o600 => c(
            "vault-file",
            Level::Warn,
            format!("Vault {} has mode {mode:04o}; 0600 is expected.", p(path)),
            cmd_path("chmod 600", path),
        ),
        Node::Found { .. } => c(
            "vault-file",
            Level::Ok,
            format!("Vault file {} exists with mode 0600.", p(path)),
            None,
        ),
    }
}

/// `grants.toml` through the daemon's own loader: same mode, owner, size and parse rules.
pub fn check_grants(path: &Path, widgets: usize) -> Check {
    let fixmode = cmd_path("chmod 600", path);
    match Grants::load(path) {
        Ok(_) if matches!(inspect(path, false), Node::Missing) => {
            if widgets > 0 {
                c(
                    "grants",
                    Level::Warn,
                    format!(
                        "No grants file but {widgets} widget(s) are installed, so each is denied all data."
                    ),
                    Some("reinstall a widget with: ow install <id>".into()),
                )
            } else {
                c(
                    "grants",
                    Level::Ok,
                    "No grants file yet; every widget is denied until one is installed.",
                    None,
                )
            }
        }
        Ok(g) => c(
            "grants",
            Level::Ok,
            format!(
                "grants.toml is valid (0600) with {} grant(s).",
                g.entries().len()
            ),
            None,
        ),
        Err(GrantsError::Insecure) => c(
            "grants",
            Level::Fail,
            format!(
                "{} is not a plain file of yours with mode 0600; the daemon will refuse it.",
                p(path)
            ),
            fixmode,
        ),
        Err(GrantsError::Io) => c(
            "grants",
            Level::Fail,
            format!("{} cannot be read.", p(path)),
            Some("check that it is a regular file you can read".into()),
        ),
        Err(e) => c(
            "grants",
            Level::Fail,
            format!(
                "{} is not valid ({e}); the daemon would deny every widget.",
                p(path)
            ),
            Some("fix or remove the file, then: ow install <id> again for widgets you want".into()),
        ),
    }
}

// ---- widgets ------------------------------------------------------------------------------

/// Validate one installed widget with the install validators. Returns the problem, if any.
fn widget_problem(paths: &Paths, id: &str) -> Result<(), String> {
    let rec =
        store::read_installed(paths, id)?.ok_or("install record installed.json is missing")?;
    if rec.id != id {
        return Err("install record names a different widget".into());
    }
    let dir = store::widget_dir(paths, id).ok_or("not a valid widget id")?;
    let mut bytes: Vec<Vec<u8>> = Vec::new();
    for name in index::FILES {
        let b = store::read_capped(&dir.join(name), validate::MAX_FILE)
            .map_err(|_| format!("{name} is missing, not a plain file, or too large"))?;
        if rec.files.get(name).is_none_or(|h| *h != hash::sha256(&b)) {
            return Err(format!("{name} changed since it was installed"));
        }
        bytes.push(b);
    }
    let listing = index::Listing {
        id: id.to_owned(),
        repo: index::Repo {
            owner: "x".into(),
            name: "x".into(),
        },
        commit: rec.commit,
        status: index::Status::Alpha,
        tier: index::Tier::Spec,
        name: None,
        version: None,
        sha256: None,
        providers: None,
        capabilities: None,
        layout: None,
    };
    let cat = catalog::snapshot()?;
    validate::check_files(&listing, &cat, &bytes[0], &bytes[1], &bytes[2]).map(|_| ())
}

/// One check per installed widget, plus a warning for stray entries. Returns the checks and the
/// number of valid widget directories.
pub fn check_widgets(paths: &Paths) -> (Vec<Check>, usize) {
    let dir = paths.widgets();
    match inspect(&dir, true) {
        Node::Missing => {
            return (
                vec![c("widgets", Level::Ok, "No widgets installed yet.", None)],
                0,
            );
        }
        Node::Wrong(why) => {
            return (
                vec![c(
                    "widgets",
                    Level::Fail,
                    format!("Widgets directory {} {why}.", p(&dir)),
                    Some("move it away; reinstall widgets with: ow install <id>".into()),
                )],
                0,
            );
        }
        Node::Found { .. } => {}
    }
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return (
            vec![c(
                "widgets",
                Level::Fail,
                format!("Widgets directory {} cannot be listed.", p(&dir)),
                None,
            )],
            0,
        );
    };
    let mut ids = Vec::new();
    let mut stray = 0usize;
    for e in rd.flatten().take(MAX_WIDGETS * 2) {
        let name = e.file_name().to_string_lossy().into_owned();
        // `.lock`, `.stage-*`, `.old-*` belong to `ow install`.
        if name.starts_with('.') {
            continue;
        }
        let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
        if is_dir && install::text::store_id(&name) {
            ids.push(name);
        } else {
            stray += 1;
        }
    }
    ids.sort();
    let total = ids.len();
    ids.truncate(MAX_WIDGETS);
    let mut out = Vec::new();
    for id in &ids {
        out.push(match widget_problem(paths, id) {
            Ok(()) => c(
                &format!("widget {id}"),
                Level::Ok,
                "Files are intact and valid.",
                None,
            ),
            Err(e) => c(
                &format!("widget {id}"),
                Level::Fail,
                show(&e),
                Some(format!("ow remove {id}; then ow install {id}")),
            ),
        });
    }
    if total == 0 {
        out.push(c("widgets", Level::Ok, "No widgets installed yet.", None));
    }
    if total > MAX_WIDGETS {
        out.push(c(
            "widgets",
            Level::Warn,
            format!("Only the first {MAX_WIDGETS} of {total} widgets were checked."),
            None,
        ));
    }
    if stray > 0 {
        out.push(c(
            "widgets-stray",
            Level::Warn,
            format!("{stray} entr(ies) in {} are not widgets (wrong name, a file, or a symlink) and are ignored.", p(&dir)),
            Some("remove them if you did not put them there".into()),
        ));
    }
    (out, total)
}

// ---- unit and plugin (files only) ------------------------------------------------------------

/// The effective `ExecStart=` target of unit text (main file followed by drop-ins, in order):
/// the last non-empty one wins, as in systemd where an empty assignment only resets the list.
/// Quotes are removed and only the program is returned. `%%` is literal, `%h` and `%t` are
/// expanded; any other specifier makes the result unusable (`Err`).
fn exec_start(
    text: &str,
    home: Option<&Path>,
    runtime_base: Option<&Path>,
) -> Option<Result<String, String>> {
    let v = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("ExecStart="))
        .map(|v| v.trim_start_matches(['-', '@', ':', '+', '!']))
        .rfind(|v| !v.trim().is_empty())?;
    let raw = if let Some(rest) = v.strip_prefix('"') {
        rest.split('"').next()?.to_owned()
    } else {
        v.split_whitespace().next()?.to_owned()
    };
    let mut out = String::new();
    let mut it = raw.chars();
    while let Some(ch) = it.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        match it.next() {
            Some('%') => out.push('%'),
            Some('h') => match home {
                Some(h) => out.push_str(&h.to_string_lossy()),
                None => return Some(Err("it uses %h and HOME is not known".into())),
            },
            Some('t') => match runtime_base {
                Some(r) => out.push_str(&r.to_string_lossy()),
                None => return Some(Err("it uses %t and XDG_RUNTIME_DIR is not known".into())),
            },
            Some(other) => {
                return Some(Err(format!(
                    "it uses the specifier %{other}, which is not expanded here"
                )));
            }
            None => return Some(Err("it ends in a lone %".into())),
        }
    }
    Some(Ok(out.replace("$$", "$")))
}

const MAX_DROPINS: usize = 32;

/// Text of `owd.service.d/*.conf` in every unit directory, in name order, bounded. Returns the text
/// and how many files were read, plus a note for any that could not be.
fn dropins(env: &Env) -> (String, usize, usize) {
    let mut text = String::new();
    let (mut read, mut bad) = (0, 0);
    for d in &env.unit_dirs {
        let Ok(rd) = std::fs::read_dir(d.join(format!("{UNIT_NAME}.d"))) else {
            continue;
        };
        let mut names: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "conf"))
            .collect();
        names.sort();
        for f in names.into_iter().take(MAX_DROPINS) {
            match store::read_capped(&f, UNIT_CAP)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
            {
                Some(t) => {
                    text.push('\n');
                    text.push_str(&t);
                    read += 1;
                }
                None => bad += 1,
            }
        }
    }
    (text, read, bad)
}

/// Is the unit installed for this user? Looks at files only; nothing is asked of systemd.
pub fn check_unit(env: &Env) -> Vec<Check> {
    // A link counts as present (`systemctl link` and some packagers use them); it is followed
    // once, by the OS, and where it points is reported.
    let found = env
        .unit_dirs
        .iter()
        .map(|d| d.join(UNIT_NAME))
        .find(|f| std::fs::symlink_metadata(f).is_ok());
    let Some(file) = found else {
        return vec![c(
            "unit",
            Level::Warn,
            "owd.service is not installed for your user, so the daemon cannot start by itself. (Fine if you run owd by hand.)",
            Some("install the package, or run packaging/install-local.sh --yes".into()),
        )];
    };
    let mut out = Vec::new();
    let is_link = std::fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_symlink());
    let resolved = if is_link {
        std::fs::canonicalize(&file).ok()
    } else {
        Some(file.clone())
    };
    let via = if is_link {
        resolved
            .as_ref()
            .map_or_else(String::new, |r| format!(" (a link to {})", p(r)))
    } else {
        String::new()
    };
    let Some(resolved) = resolved.filter(|r| std::fs::metadata(r).is_ok_and(|m| m.is_file()))
    else {
        return vec![c(
            "unit",
            Level::Fail,
            format!(
                "{} is a link that leads nowhere (or not to a file).",
                p(&file)
            ),
            Some("reinstall the unit".into()),
        )];
    };
    let main = store::read_capped(&resolved, UNIT_CAP)
        .ok()
        .and_then(|b| String::from_utf8(b).ok());
    let Some(main) = main else {
        return vec![c(
            "unit",
            Level::Fail,
            format!("{} cannot be read or is too large.", p(&file)),
            None,
        )];
    };
    let (extra, n_drop, n_bad) = dropins(env);
    let combined = format!("{main}{extra}");
    let runtime_base = env
        .paths
        .runtime
        .parent()
        .filter(|_| !env.paths.runtime.as_os_str().is_empty());
    let drop_note = if n_drop + n_bad > 0 {
        format!(
            " {n_drop} drop-in file(s) read{}.",
            if n_bad > 0 {
                format!(", {n_bad} unreadable")
            } else {
                String::new()
            }
        )
    } else {
        String::new()
    };
    match exec_start(&combined, env.home.as_deref(), runtime_base) {
        None => out.push(c(
            "unit",
            Level::Fail,
            format!("{} has no ExecStart= line.", p(&file)),
            Some("reinstall the unit".into()),
        )),
        Some(Err(why)) => out.push(c(
            "unit",
            Level::Warn,
            format!(
                "Unit {}{via} found, but ExecStart was not checked: {why}.{drop_note}",
                p(&file)
            ),
            None,
        )),
        Some(Ok(bin)) => {
            let bp = PathBuf::from(&bin);
            let exe = bp.is_absolute()
                && std::fs::metadata(&bp).is_ok_and(|m| m.is_file() && m.mode() & 0o111 != 0);
            if exe {
                out.push(c(
                    "unit",
                    Level::Ok,
                    format!(
                        "Unit file {}{via} found; it starts {}.{drop_note}",
                        p(&file),
                        show(&bin)
                    ),
                    None,
                ));
            } else {
                out.push(c(
                    "unit",
                    Level::Fail,
                    format!(
                        "Unit {}{via} starts {}, which is missing or not executable.{drop_note}",
                        p(&file),
                        show(&bin)
                    ),
                    Some("reinstall owd, or fix ExecStart= in the unit (or its drop-in)".into()),
                ));
            }
        }
    }
    let wants = env
        .unit_dirs
        .iter()
        .any(|d| std::fs::symlink_metadata(d.join("default.target.wants").join(UNIT_NAME)).is_ok());
    out.push(if wants {
        c("autostart", Level::Ok, "owd.service is enabled (link found in default.target.wants).", None)
    } else {
        c(
            "autostart",
            Level::Warn,
            "owd.service does not look enabled, so it will not start with your session. (File check only: systemd was not asked.)",
            Some("systemctl --user enable --now owd.service".into()),
        )
    });
    out
}

pub fn check_plugin(env: &Env) -> Check {
    let Some(dir) = &env.plugin_dir else {
        return c(
            "plugin",
            Level::Skip,
            "Not checked: the shell's plugin directory is only known for a normal session, not for --config-dir or an unset HOME.",
            None,
        );
    };
    let ours = dir.join(PLUGIN_ID);
    match inspect(&ours, true) {
        Node::Found { .. } => c(
            "plugin",
            Level::Ok,
            format!(
                "Plugin folder {} exists. (Whether the shell has it enabled is not checked.)",
                p(&ours)
            ),
            None,
        ),
        _ => c(
            "plugin",
            Level::Warn,
            format!(
                "Plugin folder {} not found. Path from docs/SPIKES.md (Omarchy 4.0.6); if you installed it elsewhere, ignore this.",
                p(&ours)
            ),
            Some("see plugin/README.md".into()),
        ),
    }
}

// ---- daemon ---------------------------------------------------------------------------------

/// Printable, short text from the daemon: it is another process's output, so it is cut and cleaned.
fn clean(s: &str, max: usize) -> String {
    let t: String = s.chars().take(max).collect();
    show(&t)
}

pub fn check_socket(paths: &Paths, uid: u32) -> Check {
    let dir = paths.runtime.clone();
    match inspect(&dir, true) {
        Node::Missing => c(
            "socket-dir",
            Level::Skip,
            format!(
                "Socket directory {} does not exist: no daemon has run in this session.",
                p(&dir)
            ),
            None,
        ),
        Node::Wrong(why) => c(
            "socket-dir",
            Level::Fail,
            format!("Socket directory {} {why}.", p(&dir)),
            Some("move it away; restart the daemon".into()),
        ),
        Node::Found { uid: u, .. } if u != uid => c(
            "socket-dir",
            Level::Fail,
            format!(
                "Socket directory {} is owned by another user; do not use it.",
                p(&dir)
            ),
            None,
        ),
        Node::Found { mode, .. } if mode & 0o077 != 0 => c(
            "socket-dir",
            Level::Fail,
            format!(
                "Socket directory {} has mode {mode:04o}; group or others can reach the socket.",
                p(&dir)
            ),
            cmd_path("chmod 700", &dir),
        ),
        Node::Found { .. } => {
            let sock = paths.socket();
            match std::fs::symlink_metadata(&sock) {
                Err(_) => c(
                    "socket-dir",
                    Level::Ok,
                    format!(
                        "Socket directory {} is private (0700); no socket file in it.",
                        p(&dir)
                    ),
                    None,
                ),
                Ok(m) if !m.file_type().is_socket() => c(
                    "socket-dir",
                    Level::Fail,
                    format!("{} is not a socket.", p(&sock)),
                    Some("remove it and restart the daemon".into()),
                ),
                Ok(m) if m.mode() & 0o077 != 0 => c(
                    "socket-dir",
                    Level::Fail,
                    format!(
                        "Socket {} has mode {:04o}; it must be 0600.",
                        p(&sock),
                        m.mode() & 0o7777
                    ),
                    Some("restart the daemon".into()),
                ),
                Ok(_) => c(
                    "socket-dir",
                    Level::Ok,
                    format!(
                        "Socket directory {} is private (0700) and the socket is 0600.",
                        p(&dir)
                    ),
                    None,
                ),
            }
        }
    }
}

/// Ask the daemon for `status`, then `vault.status`. Returns the checks.
pub fn check_daemon(env: &Env) -> Vec<Check> {
    let st = match daemon::call_timeout(
        &env.paths,
        "{\"v\":0,\"id\":1,\"op\":\"status\"}\n",
        CALL_TIMEOUT,
    ) {
        Ok(line) => serde_json::from_str::<Value>(&line).ok(),
        Err(e) => {
            let (level, msg, fix) = if e.starts_with("daemon not reachable") {
                (
                    Level::Fail,
                    "The daemon is not running (nothing answers on its socket).".to_owned(),
                    "systemctl --user enable --now owd.service   (or run owd in a terminal)"
                        .to_owned(),
                )
            } else {
                (
                    Level::Fail,
                    format!("Not talking to the socket: {}", clean(&e, 120)),
                    "check the socket directory above; restart the daemon".to_owned(),
                )
            };
            return vec![
                c("daemon", level, msg, Some(fix)),
                c(
                    "version",
                    Level::Skip,
                    "Not checked: the daemon did not answer.",
                    None,
                ),
                c(
                    "vault-state",
                    Level::Skip,
                    "Not checked: the daemon did not answer.",
                    None,
                ),
            ];
        }
    };
    let mut out = Vec::new();
    match st.as_ref().filter(|v| v["ok"] == true) {
        None => {
            out.push(c(
                "daemon",
                Level::Fail,
                "Something answered on the socket but not like owd.",
                Some("restart the daemon".into()),
            ));
            out.push(c(
                "version",
                Level::Skip,
                "Not checked: no valid status.",
                None,
            ));
        }
        Some(v) => {
            out.push(c(
                "daemon",
                Level::Ok,
                "The daemon answers on its socket (peer is you).",
                None,
            ));
            let dv = v["daemonVersion"].as_str().unwrap_or("");
            out.push(if dv == env.ow_version {
                c(
                    "version",
                    Level::Ok,
                    format!("Daemon and ow are both version {}.", clean(dv, 32)),
                    None,
                )
            } else {
                c(
                    "version",
                    Level::Warn,
                    format!(
                        "Daemon is version {}, ow is {}.",
                        clean(dv, 32),
                        env.ow_version
                    ),
                    Some("systemctl --user restart owd.service".into()),
                )
            });
        }
    }
    out.push(vault_state(env));
    out
}

fn vault_state(env: &Env) -> Check {
    let reply = daemon::call_timeout(
        &env.paths,
        "{\"v\":0,\"id\":2,\"op\":\"vault.status\"}\n",
        CALL_TIMEOUT,
    )
    .map_err(|e| clean(&e, 120))
    .and_then(|l| serde_json::from_str::<Value>(&l).map_err(|_| "unreadable reply".to_owned()))
    .and_then(|v| {
        if v["error"].is_null() {
            Ok(v)
        } else {
            Err("the daemon refused the question".to_owned())
        }
    });
    match reply {
        Err(e) => c(
            "vault-state",
            Level::Warn,
            format!("Could not read the vault state: {e}"),
            None,
        ),
        Ok(v) => match v["state"].as_str() {
            Some("unlocked") => {
                let count = v["count"].as_u64().unwrap_or(0);
                let shown = if env.names {
                    let names: Vec<String> = v["names"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .take(20)
                                .map(|n| clean(n, 64))
                                .collect()
                        })
                        .unwrap_or_default();
                    format!("secret name(s): [{}]", names.join(", "))
                } else {
                    "names hidden, add --names to list them".to_owned()
                };
                c(
                    "vault-state",
                    Level::Ok,
                    format!(
                        "Vault is unlocked; {count} secret(s), {shown} (values are never shown)."
                    ),
                    None,
                )
            }
            Some("locked") => c(
                "vault-state",
                Level::Ok,
                "Vault is locked; providers that need a secret wait until: ow vault unlock",
                None,
            ),
            _ => c(
                "vault-state",
                Level::Warn,
                "The daemon gave an unreadable vault state.",
                None,
            ),
        },
    }
}

// ---- run and print --------------------------------------------------------------------------

fn skipped(env: &Env, area: &str, ids: &[&str]) -> Vec<Check> {
    let why = env
        .unknown
        .iter()
        .find(|(a, _)| *a == area)
        .map_or("unknown", |(_, w)| w.as_str());
    ids.iter()
        .map(|id| c(id, Level::Skip, format!("Not checked: {why}."), None))
        .collect()
}

pub fn run(env: &Env) -> Vec<Check> {
    let pa = &env.paths;
    let mut out = Vec::new();
    #[allow(clippy::type_complexity)] // a local table, clearer inline than a named type
    let dirs: [(&str, &str, &str, &Path, (Level, &str)); 3] = [
        (
            "data",
            "data-dir",
            "Data directory",
            &pa.data,
            (
                Level::Warn,
                "The systemd service cannot start without it (a daemon you start by hand can).",
            ),
        ),
        (
            "config",
            "config-dir",
            "Config directory",
            &pa.config,
            (
                Level::Warn,
                "It is created when the service first starts or a widget is installed.",
            ),
        ),
        (
            "state",
            "state-dir",
            "State directory",
            &pa.state,
            (Level::Warn, "It is created when the service first starts."),
        ),
    ];
    for (area, id, what, path, missing) in dirs {
        if env.known(area) {
            out.push(check_private_dir(id, what, path, env.uid, missing));
        } else {
            out.extend(skipped(env, area, &[id]));
        }
    }
    if env.known("runtime") {
        out.push(check_socket(pa, env.uid));
        out.extend(check_daemon(env));
    } else {
        out.extend(skipped(
            env,
            "runtime",
            &["socket-dir", "daemon", "version", "vault-state"],
        ));
    }
    let mut widget_count = 0;
    if env.known("data") {
        out.push(check_vault_file(&pa.vault(), env.uid));
    } else {
        out.extend(skipped(env, "data", &["vault-file"]));
    }
    let mut widgets = Vec::new();
    if env.known("data") {
        let (w, n) = check_widgets(pa);
        widgets = w;
        widget_count = n;
    } else {
        widgets.extend(skipped(env, "data", &["widgets"]));
    }
    if env.known("config") {
        out.push(check_grants(&pa.grants(), widget_count));
    } else {
        out.extend(skipped(env, "config", &["grants"]));
    }
    out.extend(widgets);
    out.extend(check_unit(env));
    out.push(check_plugin(env));
    out
}

pub fn failed(checks: &[Check]) -> bool {
    checks.iter().any(|k| k.level == Level::Fail)
}

pub fn render_text(checks: &[Check]) -> String {
    let mut s = String::new();
    for k in checks {
        s.push_str(&format!(
            "{:<5} {:<12} {}\n",
            k.level.label(),
            show(&k.id),
            k.msg
        ));
        if let Some(f) = &k.fix {
            s.push_str(&format!("      fix: {f}\n"));
        }
    }
    let n = |l| checks.iter().filter(|k| k.level == l).count();
    s.push_str(&format!(
        "\n{} ok, {} warning(s), {} failure(s), {} not checked. Nothing was changed.\n",
        n(Level::Ok),
        n(Level::Warn),
        n(Level::Fail),
        n(Level::Skip)
    ));
    s
}

pub fn render_json(checks: &[Check]) -> String {
    let items: Vec<Value> = checks
        .iter()
        .map(|k| json!({"id": k.id, "level": k.level.label().to_lowercase(), "message": k.msg, "fix": k.fix}))
        .collect();
    let v = json!({"ok": !failed(checks), "checks": items});
    format!("{v}\n")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn me() -> u32 {
        rustix::process::geteuid().as_raw()
    }
    fn mode(path: &Path, m: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(m)).unwrap();
    }
    fn dir(path: &Path, m: u32) {
        std::fs::create_dir_all(path).unwrap();
        mode(path, m);
    }
    fn lv(k: &Check) -> Level {
        k.level
    }
    fn env(root: &Path) -> Env {
        let paths = Paths::under(root);
        Env {
            unit_dirs: vec![root.join("config/systemd/user")],
            plugin_dir: Some(root.join("plugins")),
            uid: me(),
            ow_version: "0.0.0".into(),
            home: Some(root.join("home")),
            names: false,
            unknown: Vec::new(),
            paths,
        }
    }
    const NONE: (Level, &str) = (Level::Fail, "x");

    #[test]
    fn private_dir_verdicts() {
        let d = tempfile::tempdir().unwrap();
        let ok = d.path().join("ok");
        dir(&ok, 0o700);
        assert_eq!(lv(&check_private_dir("i", "D", &ok, me(), NONE)), Level::Ok);
        // Wrong mode, with the exact fix.
        let loose = d.path().join("loose");
        dir(&loose, 0o755);
        let k = check_private_dir("i", "D", &loose, me(), NONE);
        assert_eq!(k.level, Level::Fail);
        assert!(k.msg.contains("0755") && k.fix.unwrap().starts_with("chmod 700"));
        // Another owner.
        let k = check_private_dir("i", "D", &ok, me() + 1, NONE);
        assert_eq!(k.level, Level::Fail);
        assert!(k.msg.contains("another user"));
        // A symlink to a good directory is refused (not followed).
        let link = d.path().join("link");
        symlink(&ok, &link).unwrap();
        let k = check_private_dir("i", "D", &link, me(), NONE);
        assert_eq!(k.level, Level::Fail);
        assert!(k.msg.contains("symbolic link"));
        // A file where a directory belongs.
        let f = d.path().join("f");
        std::fs::write(&f, "x").unwrap();
        assert_eq!(
            lv(&check_private_dir("i", "D", &f, me(), NONE)),
            Level::Fail
        );
        // Missing uses the caller's verdict.
        let miss = d.path().join("miss");
        assert_eq!(
            lv(&check_private_dir(
                "i",
                "D",
                &miss,
                me(),
                (Level::Warn, "w")
            )),
            Level::Warn
        );
    }

    #[test]
    fn vault_file_verdicts_and_it_is_never_read() {
        let d = tempfile::tempdir().unwrap();
        let v = d.path().join("vault.owv");
        assert_eq!(lv(&check_vault_file(&v, me())), Level::Warn);
        std::fs::write(&v, "FAKE-SECRET-CONTENT").unwrap();
        mode(&v, 0o600);
        let k = check_vault_file(&v, me());
        assert_eq!(k.level, Level::Ok);
        assert!(!k.msg.contains("FAKE-SECRET"));
        mode(&v, 0o644);
        assert_eq!(lv(&check_vault_file(&v, me())), Level::Fail);
        mode(&v, 0o400);
        assert_eq!(lv(&check_vault_file(&v, me())), Level::Warn);
        mode(&v, 0o600);
        assert_eq!(lv(&check_vault_file(&v, me() + 1)), Level::Fail);
        let l = d.path().join("link.owv");
        symlink(&v, &l).unwrap();
        assert_eq!(lv(&check_vault_file(&l, me())), Level::Fail);
        // A directory in its place.
        let dd = d.path().join("dir.owv");
        dir(&dd, 0o700);
        assert_eq!(lv(&check_vault_file(&dd, me())), Level::Fail);
    }

    #[test]
    fn grants_verdicts_including_hostile_files() {
        let d = tempfile::tempdir().unwrap();
        let g = d.path().join("grants.toml");
        assert_eq!(lv(&check_grants(&g, 0)), Level::Ok);
        assert_eq!(lv(&check_grants(&g, 2)), Level::Warn);
        let good = "[[grant]]\nwidget = \"w\"\ntopics = [\"system.cpu\"]\n";
        std::fs::write(&g, good).unwrap();
        mode(&g, 0o600);
        let k = check_grants(&g, 1);
        assert_eq!((k.level, k.msg.contains("1 grant")), (Level::Ok, true));
        mode(&g, 0o644);
        assert_eq!(lv(&check_grants(&g, 1)), Level::Fail);
        // Garbage, unknown fields, duplicates.
        for bad in [
            "\u{0}\u{1}garbage",
            "[[grant]]\nbogus = 1\n",
            &format!("{good}{good}"),
        ] {
            std::fs::write(&g, bad).unwrap();
            mode(&g, 0o600);
            let k = check_grants(&g, 1);
            assert_eq!(k.level, Level::Fail, "{bad:?}");
        }
        // Huge file: refused without reading it all into the report.
        std::fs::write(&g, vec![b'#'; 8 * 1024 * 1024]).unwrap();
        mode(&g, 0o600);
        assert_eq!(lv(&check_grants(&g, 1)), Level::Fail);
        // Symlink to a good file, and a FIFO, must not hang or pass.
        let target = d.path().join("t.toml");
        std::fs::write(&target, good).unwrap();
        mode(&target, 0o600);
        let l = d.path().join("l.toml");
        symlink(&target, &l).unwrap();
        assert_eq!(lv(&check_grants(&l, 1)), Level::Fail);
        let fifo = d.path().join("fifo.toml");
        rustix::fs::mknodat(
            rustix::fs::CWD,
            &fifo,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(0o600),
            0,
        )
        .unwrap();
        assert_eq!(lv(&check_grants(&fifo, 1)), Level::Fail);
    }

    #[test]
    fn unit_checks_look_at_files_only() {
        let d = tempfile::tempdir().unwrap();
        let e = env(d.path());
        let k = check_unit(&e);
        assert_eq!((k.len(), k[0].level), (1, Level::Warn));
        // Installed, binary missing.
        let ud = d.path().join("config/systemd/user");
        dir(&ud, 0o755);
        let bin = d.path().join("my bin/owd");
        std::fs::write(
            ud.join(UNIT_NAME),
            format!("[Service]\nExecStart=\"{}\"\n", bin.display()),
        )
        .unwrap();
        let k = check_unit(&e);
        assert_eq!(k[0].level, Level::Fail);
        // Binary present (path with a space), not enabled.
        dir(bin.parent().unwrap(), 0o755);
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        mode(&bin, 0o755);
        let k = check_unit(&e);
        assert_eq!((k[0].level, k[1].level), (Level::Ok, Level::Warn));
        // Enabled = link present.
        dir(&ud.join("default.target.wants"), 0o755);
        symlink(
            ud.join(UNIT_NAME),
            ud.join("default.target.wants").join(UNIT_NAME),
        )
        .unwrap();
        assert_eq!(check_unit(&e)[1].level, Level::Ok);
        // A huge unit file is not ours.
        std::fs::write(ud.join(UNIT_NAME), vec![b'#'; 200_000]).unwrap();
        assert_eq!(check_unit(&e)[0].level, Level::Fail);
        // A symlinked unit file is followed once and judged by its content (no ExecStart here).
        std::fs::remove_file(ud.join(UNIT_NAME)).unwrap();
        symlink(&bin, ud.join(UNIT_NAME)).unwrap();
        assert_eq!(check_unit(&e)[0].level, Level::Fail);
    }

    fn es(t: &str) -> Option<Result<String, String>> {
        exec_start(t, Some(Path::new("/h")), Some(Path::new("/run/user/1")))
    }

    #[test]
    fn exec_start_parsing_and_specifiers() {
        assert_eq!(
            es("ExecStart=/usr/bin/owd\n"),
            Some(Ok("/usr/bin/owd".into()))
        );
        assert_eq!(
            es("ExecStart=\"/a b/owd\" --x\n"),
            Some(Ok("/a b/owd".into()))
        );
        assert_eq!(es("ExecStart=\"/a%%b$$c\"\n"), Some(Ok("/a%b$c".into())));
        assert_eq!(es("ExecStart=%h/bin/owd\n"), Some(Ok("/h/bin/owd".into())));
        assert_eq!(es("ExecStart=%t/x\n"), Some(Ok("/run/user/1/x".into())));
        assert!(matches!(es("ExecStart=%U/x\n"), Some(Err(_))));
        assert!(matches!(
            exec_start("ExecStart=%h/x", None, None),
            Some(Err(_))
        ));
        assert_eq!(es("[Service]\n"), None);
        // A drop-in resets and overrides: the last non-empty assignment wins.
        assert_eq!(
            es("ExecStart=/a\n\nExecStart=\nExecStart=/b\n"),
            Some(Ok("/b".into()))
        );
        assert_eq!(es("ExecStart=/a\n\nExecStart=\n"), Some(Ok("/a".into())));
    }

    #[test]
    fn unit_with_home_specifier_dropin_and_link() {
        let d = tempfile::tempdir().unwrap();
        let e = env(d.path());
        let ud = d.path().join("config/systemd/user");
        dir(&ud, 0o755);
        let home = d.path().join("home");
        dir(&home.join("bin"), 0o755);
        std::fs::write(home.join("bin/owd"), "#!/bin/sh\n").unwrap();
        mode(&home.join("bin/owd"), 0o755);
        // %h in ExecStart is expanded before the executable check.
        std::fs::write(ud.join(UNIT_NAME), "[Service]\nExecStart=%h/bin/owd\n").unwrap();
        assert_eq!(check_unit(&e)[0].level, Level::Ok);
        // A drop-in that points ExecStart at a missing binary is honoured (and noted).
        dir(&ud.join("owd.service.d"), 0o755);
        std::fs::write(
            ud.join("owd.service.d/10.conf"),
            "[Service]\nExecStart=\nExecStart=/nope/owd\n",
        )
        .unwrap();
        let k = check_unit(&e);
        assert_eq!(k[0].level, Level::Fail);
        assert!(k[0].msg.contains("1 drop-in"), "{}", k[0].msg);
        // A drop-in that fixes it.
        std::fs::write(
            ud.join("owd.service.d/20.conf"),
            format!(
                "[Service]\nExecStart=\nExecStart={}/bin/owd\n",
                home.display()
            ),
        )
        .unwrap();
        assert_eq!(check_unit(&e)[0].level, Level::Ok);
        // A specifier we do not expand is a WARN, not a false FAIL.
        std::fs::remove_dir_all(ud.join("owd.service.d")).unwrap();
        std::fs::write(ud.join(UNIT_NAME), "[Service]\nExecStart=%U/x\n").unwrap();
        assert_eq!(check_unit(&e)[0].level, Level::Warn);
        // A symlinked unit file counts as present, and says where it points.
        let real = d.path().join("elsewhere.service");
        std::fs::write(
            &real,
            format!("[Service]\nExecStart={}/bin/owd\n", home.display()),
        )
        .unwrap();
        std::fs::remove_file(ud.join(UNIT_NAME)).unwrap();
        symlink(&real, ud.join(UNIT_NAME)).unwrap();
        let k = check_unit(&e);
        assert_eq!(k[0].level, Level::Ok);
        assert!(k[0].msg.contains("a link to") && k[0].msg.contains("elsewhere.service"));
        // A dangling link is a FAIL.
        std::fs::remove_file(&real).unwrap();
        assert_eq!(check_unit(&e)[0].level, Level::Fail);
    }

    #[test]
    fn fix_commands_are_quoted_or_omitted() {
        let q = cmd_path("chmod 600", Path::new("/tmp/a b"));
        assert_eq!(q.as_deref(), Some("chmod 600 '/tmp/a b'"));
        let hostile = Path::new("/tmp/q'; touch PWNED #");
        let c = cmd_path("chmod 600", hostile).unwrap();
        assert_eq!(c, "chmod 600 '/tmp/q'\\''; touch PWNED #'");
        // Control characters: no command at all, and the message shows no raw bytes.
        let esc = Path::new("/tmp/\u{1b}[31mred\u{7}");
        assert_eq!(cmd_path("chmod 600", esc), None);
        let k = check_private_dir("i", "D", esc, me(), (Level::Fail, "x"));
        assert!(k.fix.is_none());
        let t = render_text(&[k]);
        assert!(!t.contains('\u{1b}') && !t.contains('\u{7}'));
    }

    #[test]
    fn unknown_locations_are_skipped_with_a_reason() {
        let none = |_: &str| None;
        let e = Env::from_lookup(none);
        let ks = run(&e);
        assert!(!failed(&ks), "{ks:?}");
        for id in [
            "data-dir",
            "config-dir",
            "state-dir",
            "socket-dir",
            "daemon",
            "vault-file",
            "grants",
        ] {
            let k = ks.iter().find(|k| k.id == id).unwrap();
            assert_eq!(k.level, Level::Skip, "{id}");
            assert!(k.msg.contains("unset or not absolute"));
        }
        // Relative values are as good as unset.
        let rel = |k: &str| (k == "HOME" || k == "XDG_RUNTIME_DIR").then(|| PathBuf::from("rel"));
        assert!(Env::from_lookup(rel).unknown.len() >= 4);
        // Only the runtime dir missing: file checks still run.
        let d = tempfile::tempdir().unwrap();
        let root = d.path().to_path_buf();
        let only_home = move |k: &str| (k == "HOME").then(|| root.clone());
        let ks = run(&Env::from_lookup(only_home));
        assert_eq!(
            ks.iter().find(|k| k.id == "data-dir").unwrap().level,
            Level::Warn
        );
        assert_eq!(
            ks.iter().find(|k| k.id == "daemon").unwrap().level,
            Level::Skip
        );
    }

    #[test]
    fn plugin_is_reported_never_guessed() {
        let d = tempfile::tempdir().unwrap();
        let mut e = env(d.path());
        assert_eq!(check_plugin(&e).level, Level::Warn);
        dir(&d.path().join("plugins").join(PLUGIN_ID), 0o755);
        assert_eq!(check_plugin(&e).level, Level::Ok);
        e.plugin_dir = None;
        let k = check_plugin(&e);
        assert_eq!(k.level, Level::Skip);
        assert!(k.msg.starts_with("Not checked"));
    }

    #[test]
    fn socket_dir_verdicts() {
        let d = tempfile::tempdir().unwrap();
        let e = env(d.path());
        assert_eq!(check_socket(&e.paths, me()).level, Level::Skip);
        dir(&e.paths.runtime, 0o755);
        assert_eq!(check_socket(&e.paths, me()).level, Level::Fail);
        mode(&e.paths.runtime, 0o700);
        assert_eq!(check_socket(&e.paths, me()).level, Level::Ok);
        // A plain file where the socket belongs.
        std::fs::write(e.paths.socket(), "x").unwrap();
        assert_eq!(check_socket(&e.paths, me()).level, Level::Fail);
        // Not reachable and not trusted: the daemon check fails without sending anything.
        let k = check_daemon(&e);
        assert_eq!(k[0].level, Level::Fail);
        assert!(k.iter().skip(1).all(|c| c.level == Level::Skip));
    }

    #[test]
    fn widgets_hostile_directory() {
        let d = tempfile::tempdir().unwrap();
        let e = env(d.path());
        let (k, n) = check_widgets(&e.paths);
        assert_eq!((n, k[0].level), (0, Level::Ok));
        let w = e.paths.widgets();
        dir(&w, 0o700);
        // Strays: a file, a bad name, a symlink to a directory, a lock file that is ignored.
        std::fs::write(w.join("file"), "x").unwrap();
        dir(&w.join("Bad Name"), 0o700);
        let real = d.path().join("real");
        dir(&real, 0o700);
        symlink(&real, w.join("io.github.a.linked")).unwrap();
        std::fs::write(w.join(".lock"), "").unwrap();
        // A valid-looking id with no record.
        dir(&w.join("io.github.a.norecord"), 0o700);
        let (k, n) = check_widgets(&e.paths);
        assert_eq!(n, 1);
        assert!(
            k.iter()
                .any(|c| c.id == "widget io.github.a.norecord" && c.level == Level::Fail)
        );
        assert!(
            k.iter().any(|c| c.id == "widgets-stray"
                && c.level == Level::Warn
                && c.msg.starts_with("3 "))
        );
        // The widgets directory itself a symlink.
        let e2 = env(&d.path().join("other"));
        dir(&e2.paths.data, 0o700);
        symlink(&real, e2.paths.widgets()).unwrap();
        assert_eq!(check_widgets(&e2.paths).0[0].level, Level::Fail);
    }

    #[test]
    fn output_is_plain_and_json_parses() {
        let ks = vec![
            c("a", Level::Ok, "fine", None),
            c("b\u{1b}[31m", Level::Fail, "bad", Some("do x".into())),
        ];
        let t = render_text(&ks);
        assert!(
            !t.contains('\u{1b}'),
            "escape sequences must not reach the terminal"
        );
        assert!(t.contains("fix: do x") && t.contains("1 failure(s)"));
        let j: Value = serde_json::from_str(&render_json(&ks)).unwrap();
        assert_eq!(j["ok"], false);
        assert_eq!(j["checks"][0]["level"], "ok");
        assert!(failed(&ks) && !failed(&ks[..1]));
    }
}
