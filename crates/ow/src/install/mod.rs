//! `ow install <id>`, `ow list`, `ow remove <id>` for spec widgets (JSON only).
//! Design and trust statements: docs/INSTALL_DESIGN.md. Layout of this module:
//! `json`, `text`, `catalog`, `index`, `validate`, `summary`, `grants_edit` are pure functions;
//! `store` writes files; this file wires them together and owns the only network seam (`Source`).
pub mod catalog;
pub mod grants_edit;
pub mod hash;
pub mod index;
pub mod json;
pub mod store;
pub mod summary;
pub mod text;
pub mod time;
pub mod validate;

use crate::daemon;
use index::{FILES, Listing};
use ow_core::{Grants, GrantsError, Paths};
use std::io::Write;
use store::Installed;
use text::show;

/// The registry index host compiled into the binary. Changed per run only by `--registry`.
pub const DEFAULT_REGISTRY: &str = "omarchy-widgets-web.vercel.app";
const INDEX_PATH: &str = "/registry.json";
const RAW_HOST: &str = "raw.githubusercontent.com";
/// The index grows with the catalog (about 13 KB per widget today); files stay at 256 KB.
const INDEX_CAP: usize = 2 * 1024 * 1024;

pub const USAGE: &str = "usage:\n  ow install <id> [--registry https://<host>] [--allow-planned] [--dry-run] [--yes --accept-access <hash>]\n  ow list\n  ow remove <id>";

/// Where bytes come from. Production has one implementation (`NetSource`, on `ow-egress`);
/// tests implement it over a map. Nothing else in this module touches the network.
pub trait Source {
    fn index(&self) -> Result<Vec<u8>, String>;
    fn file(&self, listing: &Listing, name: &str) -> Result<Vec<u8>, String>;
}

/// The person at the keyboard.
pub trait Ui {
    /// True only when a person can answer (stdin and stdout are terminals).
    fn interactive(&self) -> bool;
    /// Show `prompt`, return the typed line without its newline.
    fn ask(&mut self, prompt: &str) -> Result<String, String>;
}

pub struct NetSource {
    host: String,
}

impl NetSource {
    pub fn new(host: &str) -> Self {
        Self {
            host: host.to_owned(),
        }
    }
}

fn net_err(e: &ow_egress::EgressError) -> String {
    use ow_egress::EgressError as E;
    match e {
        E::Status { code: 404, .. } => "not found (HTTP 404)".into(),
        other => format!("network: {other}"),
    }
}

impl Source for NetSource {
    fn index(&self) -> Result<Vec<u8>, String> {
        let limits = ow_egress::Limits {
            max_body: INDEX_CAP,
            ..ow_egress::Limits::default()
        };
        let eg = ow_egress::Egress::with_limits(&[self.host.as_str()], limits)
            .map_err(|e| net_err(&e))?;
        let r = eg
            .send(&ow_egress::Request::get(&self.host, INDEX_PATH))
            .map_err(|e| net_err(&e))?;
        Ok(r.into_body())
    }

    fn file(&self, l: &Listing, name: &str) -> Result<Vec<u8>, String> {
        let path = index::file_path(l, name)?;
        let eg = ow_egress::Egress::new(&[RAW_HOST]).map_err(|e| net_err(&e))?;
        let r = eg
            .send(&ow_egress::Request::get(RAW_HOST, &path))
            .map_err(|e| net_err(&e))?;
        Ok(r.into_body())
    }
}

/// Typing `yes` on a real terminal.
pub struct TtyUi;

impl Ui for TtyUi {
    fn interactive(&self) -> bool {
        rustix::termios::isatty(rustix::stdio::stdin())
            && rustix::termios::isatty(rustix::stdio::stdout())
    }

    fn ask(&mut self, prompt: &str) -> Result<String, String> {
        use std::io::{BufRead, Read};
        eprint!("{prompt}");
        let _ = std::io::stderr().flush();
        let mut line = Vec::new();
        std::io::stdin()
            .lock()
            .take(65)
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?;
        if line.last() != Some(&b'\n') {
            return Err("no answer".into());
        }
        line.pop();
        Ok(String::from_utf8_lossy(&line)
            .trim_end_matches('\r')
            .to_owned())
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct InstallArgs {
    pub id: String,
    pub registry: Option<String>,
    pub allow_planned: bool,
    pub dry_run: bool,
    pub yes: bool,
    pub accept: Option<String>,
}

/// `https://<lowercase dns name>` and nothing else. `ow-egress` checks the name again.
pub fn parse_registry(origin: &str) -> Result<String, String> {
    let host = origin
        .strip_prefix("https://")
        .ok_or("--registry must be an https origin such as https://registry.example.org")?;
    let ok = !host.is_empty()
        && host.len() <= 253
        && host.contains('.')
        && !host.contains("..")
        && !host.starts_with(['.', '-'])
        && !host.ends_with(['.', '-'])
        && host
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'.' || c == b'-');
    if ok {
        Ok(host.to_owned())
    } else {
        Err("--registry must be a bare lowercase host name: no port, path, query, userinfo, IP address or trailing slash".into())
    }
}

pub fn parse_install_args(args: &[String]) -> Result<InstallArgs, String> {
    let mut a = InstallArgs::default();
    let mut id: Option<&str> = None;
    let mut it = args.iter();
    while let Some(x) = it.next() {
        let dup = |name: &str| format!("{name} given twice");
        match x.as_str() {
            "--allow-planned" if !a.allow_planned => a.allow_planned = true,
            "--dry-run" if !a.dry_run => a.dry_run = true,
            "--yes" if !a.yes => a.yes = true,
            "--allow-planned" | "--dry-run" | "--yes" => return Err(dup(x)),
            "--registry" | "--accept-access" => {
                let v = it.next().ok_or_else(|| format!("{x} needs a value"))?;
                let slot = if x == "--registry" {
                    &mut a.registry
                } else {
                    &mut a.accept
                };
                if slot.replace(v.clone()).is_some() {
                    return Err(dup(x));
                }
            }
            s if s.starts_with("--registry=") || s.starts_with("--accept-access=") => {
                let (k, v) = s.split_once('=').unwrap_or((s, ""));
                let slot = if k == "--registry" {
                    &mut a.registry
                } else {
                    &mut a.accept
                };
                if slot.replace(v.to_owned()).is_some() {
                    return Err(dup(k));
                }
            }
            s if s.starts_with('-') => {
                return Err(format!("unknown option {}\n{USAGE}", text::show_value(s)));
            }
            s => {
                if id.replace(s).is_some() {
                    return Err(format!("only one widget id at a time\n{USAGE}"));
                }
            }
        }
    }
    a.id = id
        .ok_or_else(|| format!("missing widget id\n{USAGE}"))?
        .to_owned();
    if !text::store_id(&a.id) {
        return Err(
            "not a valid widget id (lowercase letters, digits, '.' and '-', no path parts)".into(),
        );
    }
    if let Some(r) = &a.registry {
        parse_registry(r)?;
    }
    match (a.yes, &a.accept) {
        (true, None) => return Err("--yes is only accepted together with --accept-access <hash> (the access hash printed by --dry-run)".into()),
        (false, Some(_)) => return Err("--accept-access only makes sense with --yes".into()),
        (true, Some(h)) if !text::sha256_hex(h) => return Err("--accept-access takes the 64-character access hash".into()),
        _ => {}
    }
    if a.yes && a.dry_run {
        return Err(
            "--dry-run asks nothing and writes nothing; do not combine it with --yes".into(),
        );
    }
    Ok(a)
}

fn w(e: std::io::Error) -> String {
    e.to_string()
}

fn grants_err(e: GrantsError) -> String {
    match e {
        GrantsError::Insecure => "the grants file is accessible by group/others or not owned by you (chmod 600 it); nothing was changed".into(),
        GrantsError::Invalid => "the grants file is not valid (or is larger than the daemon reads); fix it first, nothing was changed".into(),
        _ => "cannot read the grants file; nothing was changed".into(),
    }
}

struct Fetched {
    listing: Listing,
    validated: validate::Validated,
    catalog: catalog::Catalog,
    planned_override: bool,
    /// The bytes as fetched (and as hashed); these exact bytes are what gets written.
    files: Vec<(&'static str, Vec<u8>)>,
}

/// Fetch the index entry and the three files, and decide. No disk writes.
fn fetch_and_check(src: &dyn Source, id: &str, allow_planned: bool) -> Result<Fetched, String> {
    let idx = src.index().map_err(|e| format!("registry index: {e}"))?;
    let listing = index::select_listing(&idx, id)?;
    let planned_override = index::gate(&listing, allow_planned)?;
    let catalog = catalog::snapshot()?;
    let mut files = Vec::new();
    for f in FILES {
        files.push((f, src.file(&listing, f).map_err(|e| format!("{f}: {e}"))?));
    }
    let validated =
        validate::check_files(&listing, &catalog, &files[0].1, &files[1].1, &files[2].1)?;
    Ok(Fetched {
        listing,
        validated,
        catalog,
        planned_override,
        files,
    })
}

fn reload_message(paths: &Paths) -> String {
    match daemon::reload_grants(paths) {
        Ok(true) => "The running daemon reloaded the grants.".into(),
        Ok(false) => "The daemon is not running; the grants apply when it starts.".into(),
        Err(e) => format!("WARNING: {e}"),
    }
}

/// `ow install`. `make_source` builds the byte source for the chosen registry host.
pub fn install(
    paths: &Paths,
    args: &[String],
    out: &mut dyn Write,
    make_source: &dyn Fn(&str) -> Box<dyn Source>,
    ui: &mut dyn Ui,
) -> Result<(), String> {
    let a = parse_install_args(args)?;
    // Fail before any network access when nobody can answer.
    if !(a.dry_run || a.yes || ui.interactive()) {
        return Err("installing needs your consent, and consent needs a terminal: run this in a terminal (or use --dry-run to read what it would do)".into());
    }
    let host = match &a.registry {
        Some(r) => parse_registry(r)?,
        None => DEFAULT_REGISTRY.to_owned(),
    };
    let src = make_source(&host);
    let f = fetch_and_check(src.as_ref(), &a.id, a.allow_planned)?;
    let dir = store::widget_dir(paths, &a.id).ok_or("not a valid widget id")?;
    let previous = store::read_installed(paths, &a.id).unwrap_or_default();
    let ents = summary::entries(&f.validated);
    let hash = summary::access_hash(&f.listing.id, &f.listing.commit, &ents);
    let grants_path = paths.grants();
    let text = summary::summary(&summary::Facts {
        listing: &f.listing,
        validated: &f.validated,
        catalog: &f.catalog,
        planned_override: f.planned_override,
        previous: previous.as_ref(),
        hashes_verified: f.listing.sha256.is_some(),
        registry_host: &host,
        default_registry: host == DEFAULT_REGISTRY,
        grants_path: &grants_path.display().to_string(),
        widget_dir: &dir.display().to_string(),
    });
    write!(out, "{text}").map_err(w)?;
    if a.dry_run {
        return writeln!(out, "\nDry run: nothing was written, nothing was asked.").map_err(w);
    }
    if a.yes {
        if a.accept.as_deref() != Some(hash.as_str()) {
            return Err("the access hash you accepted is not the access this widget asks for now; nothing was installed".into());
        }
    } else {
        writeln!(out).map_err(w)?;
        if ui.ask("Type yes to install with this access: ")? != "yes" {
            return writeln!(out, "Not installed.").map_err(w);
        }
    }

    // From here on: lock, then widget directory, then grants; any failure undoes the earlier step.
    let lock = store::Lock::acquire(paths)?;
    let old_grants = Grants::read_text(&grants_path)
        .map_err(grants_err)?
        .unwrap_or_default();
    let new_grants = grants_edit::edit(&old_grants, &f.listing.id, &ents, &time::now())?;
    let record = Installed {
        id: f.listing.id.clone(),
        repo: f.listing.repo.url(),
        commit: f.listing.commit.clone(),
        version: f.validated.manifest.version.clone(),
        status: f.listing.status.name().into(),
        tier: f.listing.tier.name().into(),
        installed_at: time::now(),
        files: f.validated.hashes.clone(),
        granted: ents,
    };
    let files: Vec<(&str, &[u8])> = f.files.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    let swap = store::stage_and_swap(&lock, &f.listing.id, &files, &record)?;
    store::write_grants(&grants_path, &new_grants)?;
    swap.finish();
    drop(lock);
    writeln!(
        out,
        "Installed {} {}.",
        f.listing.id,
        show(&f.validated.manifest.version)
    )
    .map_err(w)?;
    writeln!(out, "{}", reload_message(paths)).map_err(w)?;
    if f.planned_override {
        writeln!(out, "Reminder: this widget is only planned and will not show data until the needed provider exists.").map_err(w)?;
    }
    Ok(())
}

/// `ow list`.
pub fn list(paths: &Paths, out: &mut dyn Write) -> Result<(), String> {
    let ids = store::installed_ids(paths);
    if ids.is_empty() {
        return writeln!(out, "No widgets installed.").map_err(w);
    }
    let grants = Grants::load(&paths.grants());
    for id in ids {
        let Some(dir) = store::widget_dir(paths, &id) else {
            continue;
        };
        match store::read_installed(paths, &id) {
            Ok(Some(r)) => {
                writeln!(
                    out,
                    "{}  {}  {}  {}  {}  installed {}",
                    show(&r.id),
                    show(&r.version),
                    show(&r.commit.chars().take(12).collect::<String>()),
                    show(&r.status),
                    show(&r.tier),
                    show(&r.installed_at)
                )
                .map_err(w)?;
                let changed: Vec<&str> = r
                    .files
                    .iter()
                    .filter(|(n, h)| {
                        FILES.contains(&n.as_str())
                            && store::read_capped(&dir.join(n), validate::MAX_FILE)
                                .map_or(true, |b| hash::sha256(&b) != **h)
                    })
                    .map(|(n, _)| n.as_str())
                    .collect();
                if !changed.is_empty() {
                    writeln!(
                        out,
                        "  ! changed or missing since install: {}",
                        changed.join(", ")
                    )
                    .map_err(w)?;
                }
                match &grants {
                    Ok(g) => {
                        if r.granted
                            .iter()
                            .flat_map(|e| &e.topics)
                            .any(|t| g.find(&r.id, t).is_none())
                        {
                            writeln!(out, "  ! no grant in grants.toml for some of its topics (it receives no data)").map_err(w)?;
                        }
                    }
                    Err(_) => writeln!(
                        out,
                        "  ! the grants file cannot be read, grants not checked"
                    )
                    .map_err(w)?,
                }
            }
            Ok(None) | Err(_) => {
                writeln!(
                    out,
                    "{id}  ! install record missing or unreadable (remove it with: ow remove {id})"
                )
                .map_err(w)?;
            }
        }
    }
    Ok(())
}

/// `ow remove <id>`: grants first, then files. Safe to repeat.
pub fn remove(paths: &Paths, args: &[String], out: &mut dyn Write) -> Result<(), String> {
    let [id] = args else {
        return Err(format!("remove takes exactly one widget id\n{USAGE}"));
    };
    if !text::store_id(id) {
        return Err("not a valid widget id".into());
    }
    let lock = store::Lock::acquire(paths)?;
    let grants_path = paths.grants();
    let old = Grants::read_text(&grants_path)
        .map_err(grants_err)?
        .unwrap_or_default();
    let count = Grants::parse(&old)
        .map_err(grants_err)?
        .entries()
        .iter()
        .filter(|g| g.widget == *id)
        .count();
    if count > 0 {
        store::write_grants(
            &grants_path,
            &grants_edit::edit(&old, id, &[], &time::now())?,
        )?;
    }
    let had_files = store::remove_widget_dir(&lock, id)?;
    drop(lock);
    if !had_files && count == 0 {
        return writeln!(out, "{id} is not installed; nothing to do.").map_err(w);
    }
    writeln!(
        out,
        "Removed {id}: files {}, {count} grant entr{}.",
        if had_files {
            "deleted"
        } else {
            "were not there"
        },
        if count == 1 { "y" } else { "ies" }
    )
    .map_err(w)?;
    if count > 0 {
        writeln!(out, "{}", reload_message(paths)).map_err(w)?;
    }
    Ok(())
}
