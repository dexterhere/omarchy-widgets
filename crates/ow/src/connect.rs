//! `ow connect github`: guided, offline setup of the `github.prs` provider with a fine-grained
//! personal access token (docs/AUTH_PLAN.md 1.4, DECISIONS CONNECT-GITHUB).
//!
//! Rules this file keeps:
//! - NO network call. The token is not tested against GitHub; `--check` asks the running daemon,
//!   whose one audited call it is.
//! - The token comes only from `input` (no-echo terminal prompt, or a stdin line exactly where
//!   `ow vault set` allows it), is never printed, and its length is never shown. Every message
//!   is fixed text: nothing typed (token, passphrase, a mistyped option) is echoed back.
//! - Nothing is restarted or executed. The restart commands are printed as text.
//! - `config.toml` is only replaced after the daemon's own loader accepted the old one, and the
//!   new text is checked with that loader before it is written.
use crate::daemon;
use crate::input::{secret, secret_confirmed, tty_secret};
use crate::install::{Ui, store::write_private_atomic};
use crate::vault_cmd;
use ow_core::{
    Config, ConfigError, DEFAULT_INTERVAL_SECS, Grants, MAX_CONFIG_FILE, MAX_INTERVAL_SECS,
    MIN_INTERVAL_SECS, Paths, secret_name_allowed,
};
use ow_proto::StateInfo;
use ow_vault::{MAX_VALUE, Vault, VaultError};
use serde_json::{Map, Value, json};
use std::io::Write;

pub const USAGE: &str = "usage:\n  ow connect github [--secret-name NAME] [--interval SECS] [--passphrase-stdin]\n  ow connect github --check\n  ow connect github --remove [--yes]\nNAME must match github-[a-z0-9-]{1,40} (default github-main). SECS is clamped to 60..3600 (default 120).";

const TOPIC: &str = "github.prs";
const DEFAULT_NAME: &str = "github-main";
const TOKEN_URL: &str = "https://github.com/settings/personal-access-tokens/new";
const HEADER: &str = "# owd provider bindings. Written by `ow connect github`; owd reads this file only at start.\n# It holds a secret NAME, never a token.\n";

#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Connect,
    Check,
    Remove,
}

#[derive(Debug, PartialEq, Eq)]
struct Opts {
    mode: Mode,
    name: Option<String>,
    interval: Option<u64>,
    pass_stdin: bool,
    yes: bool,
}

fn parse(args: &[String]) -> Result<Opts, String> {
    let bad = |m: &str| Err(format!("{m}\n{USAGE}"));
    if args.first().map(String::as_str) != Some("github") {
        return bad("only `ow connect github` exists");
    }
    let mut o = Opts {
        mode: Mode::Connect,
        name: None,
        interval: None,
        pass_stdin: false,
        yes: false,
    };
    let mut mode_seen = false;
    let mut it = args[1..].iter();
    // Option values are never echoed in errors: a token pasted into the wrong place stays unseen.
    while let Some(a) = it.next() {
        match a.as_str() {
            "--check" | "--remove" if !mode_seen => {
                mode_seen = true;
                o.mode = if a == "--check" {
                    Mode::Check
                } else {
                    Mode::Remove
                };
            }
            "--secret-name" if o.name.is_none() => {
                let v = it.next().ok_or("--secret-name needs a value")?;
                if !secret_name_allowed("github", v) {
                    return bad("the secret name must match github-[a-z0-9-]{1,40}");
                }
                o.name = Some(v.clone());
            }
            "--interval" if o.interval.is_none() => {
                let v = it.next().ok_or("--interval needs a value")?;
                o.interval = Some(
                    v.parse::<u64>()
                        .map_err(|_| "--interval must be a whole number of seconds")?,
                );
            }
            "--passphrase-stdin" if !o.pass_stdin => o.pass_stdin = true,
            "--yes" if !o.yes => o.yes = true,
            _ => return bad("unknown or repeated option (values are not echoed)"),
        }
    }
    let ok = match o.mode {
        Mode::Connect => !o.yes,
        Mode::Check => o.name.is_none() && o.interval.is_none() && !o.pass_stdin && !o.yes,
        Mode::Remove => o.interval.is_none(),
    };
    if ok {
        Ok(o)
    } else {
        bad("these options do not go together")
    }
}

/// Accept a token only if it is non-empty, printable ASCII without spaces, within the vault's
/// value limit, and not a classic token. Messages never include any part of the input.
pub fn token_str(b: &[u8]) -> Result<&str, &'static str> {
    if b.is_empty() {
        return Err("the token is empty");
    }
    if b.len() > MAX_VALUE {
        return Err("the token is longer than the vault accepts");
    }
    if !b.iter().all(|c| (0x21..=0x7e).contains(c)) {
        return Err(
            "the token must be printable ASCII with no spaces or line breaks; check the paste",
        );
    }
    if b.starts_with(b"ghp_") {
        return Err(
            "this looks like a classic token, which cannot be limited to read-only pull requests; create a fine-grained token (use `ow vault set` if you really want this one)",
        );
    }
    std::str::from_utf8(b).map_err(|_| "the token must be printable ASCII")
}

fn table(name: &str, secs: u64) -> String {
    format!("[providers.github]\nsecret = \"{name}\"\ninterval_secs = {secs}\n")
}

/// Index range of the one `[providers.github]` table, if exactly one header line exists.
fn github_table(lines: &[&str]) -> Option<(usize, usize)> {
    let mut at = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.trim() == "[providers.github]")
        .map(|(i, _)| i);
    let (start, extra) = (at.next()?, at.next());
    if extra.is_some() {
        return None;
    }
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map_or(lines.len(), |p| start + 1 + p);
    Some((start, end))
}

fn bound_as(text: &str, name: &str, secs: u64) -> bool {
    Config::parse(text).is_ok_and(|c| {
        c.github()
            .is_some_and(|g| g.secret() == name && g.interval().as_secs() == secs)
    })
}

/// New `config.toml` text binding github to `name`. `old` must already have passed the daemon's
/// loader. Only the `[providers.github]` table changes; if that cannot be done safely (dotted
/// keys, inline tables) the file is rewritten whole, which loses nothing the loader would read
/// (it accepts no other table) but does drop comments. The result is re-checked with the loader.
pub fn edit_config(old: Option<&str>, name: &str, secs: u64) -> String {
    let t = table(name, secs);
    if let Some(old) = old.filter(|o| !o.trim().is_empty()) {
        let lines: Vec<&str> = old.lines().collect();
        let cand = match github_table(&lines) {
            Some((s, e)) => {
                let mut c = lines[..s].join("\n");
                if s > 0 {
                    c.push('\n');
                }
                c.push_str(&t);
                if e < lines.len() {
                    c.push('\n');
                    c.push_str(&lines[e..].join("\n"));
                    c.push('\n');
                }
                c
            }
            None if !old.contains("providers") => format!("{}\n\n{t}", old.trim_end()),
            None => String::new(),
        };
        if bound_as(&cand, name, secs) {
            return cand;
        }
    }
    format!("{HEADER}{t}")
}

/// `config.toml` text with no github binding. Same safety rules as `edit_config`.
pub fn remove_binding(old: &str) -> String {
    let lines: Vec<&str> = old.lines().collect();
    if let Some((s, e)) = github_table(&lines) {
        let mut c = lines[..s].join("\n");
        if s > 0 {
            c.push('\n');
        }
        c.push_str(&lines[e..].join("\n"));
        if !c.is_empty() && !c.ends_with('\n') {
            c.push('\n');
        }
        if Config::parse(&c).is_ok_and(|c| c.github().is_none()) {
            return c;
        }
    }
    HEADER.to_owned()
}

fn config_err(e: ConfigError) -> String {
    let what = match e {
        ConfigError::Insecure => "is readable by group/others or not owned by you",
        ConfigError::Invalid | ConfigError::SecretName => {
            "is not accepted by the daemon's own loader"
        }
        ConfigError::TooLarge => "is larger than the 64 KiB limit",
        ConfigError::Io => "cannot be read (it may be a symlink, a special file or unreadable)",
    };
    format!(
        "config.toml {what}. ow will not overwrite it. Fix it (chmod 600, owned by you, valid) or move it away, then run this again."
    )
}

fn print_instructions(out: &mut dyn Write, name: &str) -> std::io::Result<()> {
    writeln!(
        out,
        "\
Connect GitHub (topic github.prs: two numbers, your open pull requests and your review requests)

This command does not open a browser, run any other program or contact GitHub. It does not
test the token; `ow connect github --check` does that through the daemon after a restart.

1. In your browser, create a fine-grained personal access token yourself:
     {TOKEN_URL}
   - Resource owner: your own account.
   - Expiration: set one, 30 to 90 days. A token with no expiry is a standing risk.
   - Repository access: All repositories, or select some (the counts then cover only those).
   - Permissions, repository: Pull requests: Read-only. Metadata: Read-only (GitHub adds it).
     Nothing else, and no account permissions.
   GitHub shows the token once. Do not store it anywhere else.

2. Paste it below. Nothing is shown while you type, and it is asked twice.

What this means, plainly:
 - The token is stored encrypted in the vault file. The daemon holds it in memory only while the
   vault is unlocked, and it is never written to config.toml, logs, argv or the environment.
 - A fine-grained, read-only, expiring token limits the damage of a leak. It is still broader
   than the two numbers shown: it can read pull request contents in the repositories it covers.
 - Any program running as you can ask the unlocked daemon for the numbers a widget is granted.
 - Secret name: {name}
"
    )
}

fn vault_err(e: VaultError) -> String {
    e.to_string()
}

fn ask_passphrase(pass_stdin: bool) -> Result<zeroize::Zeroizing<Vec<u8>>, String> {
    // Same rule as `ow vault set`: on a terminal prompt there; with a pipe on stdin the
    // passphrase comes from /dev/tty (stdin may carry the token), unless --passphrase-stdin.
    if pass_stdin || rustix::termios::isatty(rustix::stdio::stdin()) {
        secret("Vault passphrase")
    } else {
        tty_secret("Vault passphrase")
    }
}

fn restart_advice(out: &mut dyn Write, tail: &str) -> std::io::Result<()> {
    writeln!(
        out,
        "\
The daemon reads config.toml only when it starts, so it does not know yet. ow does not restart
anything. When you are ready, run these yourself:
  systemctl --user restart owd.service
  ow vault unlock
{tail}"
    )
}

fn ensure_vault(paths: &Paths, out: &mut dyn Write, ui: &mut dyn Ui) -> Result<(), String> {
    match ow_vault::inspect(&paths.vault()) {
        Ok(_) => Ok(()),
        Err(VaultError::NotFound) => {
            if !ui.interactive() {
                return Err(
                    "there is no vault yet. Run `ow vault init` in a terminal first.".into(),
                );
            }
            writeln!(out, "There is no vault yet.").map_err(|e| e.to_string())?;
            if ui.ask("Type yes to create one now (the same as `ow vault init`): ")? != "yes" {
                return Err("no vault. Run `ow vault init` when you are ready.".into());
            }
            vault_cmd::run(paths, &["init".to_owned()], out)
        }
        Err(e) => Err(vault_err(e)),
    }
}

fn connect(paths: &Paths, o: &Opts, out: &mut dyn Write, ui: &mut dyn Ui) -> Result<(), String> {
    let w = |e: std::io::Error| e.to_string();
    let cfg_path = paths.config_file();
    // Fail closed before asking for anything: a config we cannot safely edit stops us here.
    let (cfg, old) = Config::load_with_text(&cfg_path).map_err(config_err)?;
    let name = o.name.as_deref().unwrap_or(DEFAULT_NAME);
    let wanted = o
        .interval
        .or_else(|| cfg.github().map(|g| g.interval().as_secs()))
        .unwrap_or(DEFAULT_INTERVAL_SECS);
    let secs = wanted.clamp(MIN_INTERVAL_SECS, MAX_INTERVAL_SECS);
    print_instructions(out, name).map_err(w)?;
    if secs != wanted {
        writeln!(
            out,
            "Polling interval {wanted} s is outside {MIN_INTERVAL_SECS}..{MAX_INTERVAL_SECS}; using {secs} s."
        )
        .map_err(w)?;
    }
    if let Some(g) = cfg.github()
        && g.secret() != name
    {
        writeln!(
            out,
            "NOTE: config.toml already binds github to another secret name; this replaces that binding."
        )
        .map_err(w)?;
    }
    ensure_vault(paths, out, ui)?;
    let pass = ask_passphrase(o.pass_stdin)?;
    let mut vault = Vault::open(&paths.vault(), &pass).map_err(vault_err)?;
    drop(pass);
    let replaced = vault.get(name).is_some();
    let token = secret_confirmed("GitHub token")?;
    let value = token_str(&token)?;
    vault.set(name, value).map_err(vault_err)?;
    drop(token);
    vault_cmd::lock_daemon(paths);
    writeln!(
        out,
        "Stored the token in the vault as {name}{}.",
        if replaced {
            " (replaced the old value)"
        } else {
            ""
        }
    )
    .map_err(w)?;
    let text = edit_config(old.as_deref(), name, secs);
    write_private_atomic(&cfg_path, &text, MAX_CONFIG_FILE, "config file").map_err(|e| {
        format!(
            "the token is stored, but config.toml was not written ({e}). Run this command again."
        )
    })?;
    writeln!(
        out,
        "Wrote {} (mode 0600): [providers.github] secret = \"{name}\", interval_secs = {secs}.\nThe loader accepts only the github provider today, so that table is all the file holds.\n",
        cfg_path.display()
    )
    .map_err(w)?;
    restart_advice(
        out,
        "  ow connect github --check     (or `ow status`, or look at the widget)\nThe check asks the daemon, which then makes the one audited request to api.github.com.",
    )
    .map_err(w)
}

fn remove(paths: &Paths, o: &Opts, out: &mut dyn Write, ui: &mut dyn Ui) -> Result<(), String> {
    let w = |e: std::io::Error| e.to_string();
    let cfg_path = paths.config_file();
    let (cfg, old) = Config::load_with_text(&cfg_path).map_err(config_err)?;
    let name = o
        .name
        .clone()
        .or_else(|| cfg.github().map(|g| g.secret().to_owned()))
        .unwrap_or_else(|| DEFAULT_NAME.to_owned());
    writeln!(
        out,
        "This removes the github binding from config.toml and the secret {name} from the vault."
    )
    .map_err(w)?;
    if !o.yes {
        if !ui.interactive() {
            return Err("not a terminal: run this in a terminal, or add --yes".into());
        }
        if ui.ask("Type yes to remove them: ")? != "yes" {
            return Err("cancelled; nothing was changed".into());
        }
    }
    match ow_vault::inspect(&paths.vault()) {
        Ok(_) => {
            let pass = ask_passphrase(o.pass_stdin)?;
            let mut v = Vault::open(&paths.vault(), &pass).map_err(vault_err)?;
            drop(pass);
            let gone = v.remove(&name).map_err(vault_err)?;
            vault_cmd::lock_daemon(paths);
            writeln!(
                out,
                "{}",
                if gone {
                    "Removed the secret from the vault."
                } else {
                    "The vault had no secret with that name."
                }
            )
            .map_err(w)?;
        }
        Err(VaultError::NotFound) => writeln!(out, "There is no vault.").map_err(w)?,
        Err(e) => return Err(vault_err(e)),
    }
    if cfg.github().is_some() {
        let text = remove_binding(old.as_deref().unwrap_or_default());
        write_private_atomic(&cfg_path, &text, MAX_CONFIG_FILE, "config file")?;
        writeln!(out, "Removed the github binding from config.toml.").map_err(w)?;
    } else {
        writeln!(out, "config.toml had no github binding.").map_err(w)?;
    }
    restart_advice(out, "The github.prs topic then answers no_credential.").map_err(w)
}

/// One line for a snapshot reply: the state category and the two counts, nothing else. The
/// category is an `ow_proto::State` name, `ok`, or `error <code>` for a code from the fixed
/// protocol set. Anything the daemon sent beyond that is dropped.
pub fn render_check(reply: &str) -> String {
    let Ok(Value::Object(v)) = serde_json::from_str::<Value>(reply) else {
        return format!("{TOPIC}: error unreadable_reply");
    };
    let state_name = |m: &Map<String, Value>| {
        StateInfo::from_fields(m).and_then(|i| {
            serde_json::to_value(i.state)
                .ok()
                .and_then(|s| s.as_str().map(str::to_owned))
        })
    };
    if let Some(Value::Object(e)) = v.get("error") {
        if let Some(s) = state_name(e) {
            return format!("{TOPIC}: {s}");
        }
        let code = e.get("code").and_then(Value::as_str).unwrap_or("");
        let known = [
            "bad_request",
            "denied",
            "unknown_topic",
            "provider_error",
            "rate_limited",
            "locked",
            "unsupported_version",
            "internal",
        ];
        return format!(
            "{TOPIC}: error {}",
            if known.contains(&code) {
                code
            } else {
                "unknown"
            }
        );
    }
    if v.get("ok") != Some(&Value::Bool(true)) {
        return format!("{TOPIC}: error unreadable_reply");
    }
    let mut line = format!("{TOPIC}: {}", state_name(&v).unwrap_or_else(|| "ok".into()));
    if let Some(Value::Object(d)) = v.get("data") {
        for k in ["open", "review"] {
            if let Some(n) = d.get(k).and_then(Value::as_u64).filter(|n| *n <= 1_000_000) {
                line.push_str(&format!(" {k}={n}"));
            }
        }
    }
    line
}

fn check(paths: &Paths, out: &mut dyn Write) -> Result<(), String> {
    // The daemon answers a snapshot only for a widget holding a grant, so borrow the id of an
    // installed widget that has one. No new protocol op (see docs/OPEN_QUESTIONS.md).
    let grants = Grants::load(&paths.grants())
        .map_err(|_| "grants.toml cannot be read; run `ow doctor`".to_string())?;
    let widget = grants
        .entries()
        .iter()
        .find(|g| g.topics.iter().any(|t| t == TOPIC))
        .map(|g| g.widget.clone())
        .ok_or(
            "no installed widget holds a grant for github.prs, and the daemon answers only granted widgets. Install one first (`ow install`), then run --check again.",
        )?;
    let req = format!(
        "{}\n",
        json!({"v": 0, "id": 1, "op": "snapshot", "widget": widget, "topic": TOPIC})
    );
    let reply = daemon::call(paths, &req)?;
    writeln!(out, "{}", render_check(&reply)).map_err(|e| e.to_string())
}

pub fn run(
    paths: &Paths,
    args: &[String],
    out: &mut dyn Write,
    ui: &mut dyn Ui,
) -> Result<(), String> {
    let o = parse(args)?;
    match o.mode {
        Mode::Connect => connect(paths, &o, out, ui),
        Mode::Check => check(paths, out),
        Mode::Remove => remove(paths, &o, out, ui),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(a: &[&str]) -> Vec<String> {
        a.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn options_parse_and_bad_ones_do_not_echo_their_value() {
        let o = parse(&s(&[
            "github",
            "--secret-name",
            "github-x",
            "--interval",
            "300",
        ]))
        .unwrap();
        assert_eq!(
            (o.name.as_deref(), o.interval),
            (Some("github-x"), Some(300))
        );
        let leaky = "github_pat_FAKE0123456789";
        for bad in [
            s(&["github", "--secret-name", leaky]),
            s(&["github", "--interval", leaky]),
            s(&["github", leaky]),
            s(&["gitlab"]),
            s(&["github", "--check", "--remove"]),
            s(&["github", "--check", "--interval", "60"]),
            s(&["github", "--yes"]),
            s(&["github", "--interval"]),
            s(&["github", "--interval", "60", "--interval", "70"]),
        ] {
            let e = parse(&bad).unwrap_err();
            assert!(!e.contains("FAKE"), "{e}");
        }
        assert!(parse(&s(&["github", "--remove", "--yes"])).is_ok());
    }

    #[test]
    fn token_rules() {
        assert_eq!(
            token_str(b"github_pat_AAAA_bbbb"),
            Ok("github_pat_AAAA_bbbb")
        );
        for bad in [
            &b""[..],
            b"has space",
            b"tab\there",
            "caf\u{e9}".as_bytes(),
            b"ctl\x01",
            b"ghp_classicFAKE",
        ] {
            let e = token_str(bad).unwrap_err();
            assert!(!e.contains("FAKE") && !e.contains("caf"), "{e}");
        }
        assert!(token_str(&vec![b'a'; MAX_VALUE]).is_ok());
        assert!(token_str(&vec![b'a'; MAX_VALUE + 1]).is_err());
    }

    fn secret_and_secs(t: &str) -> (String, u64) {
        let c = Config::parse(t).unwrap();
        let g = c.github().unwrap();
        (g.secret().to_owned(), g.interval().as_secs())
    }

    #[test]
    fn edit_creates_updates_and_keeps_the_rest() {
        let fresh = edit_config(None, "github-main", 120);
        assert_eq!(secret_and_secs(&fresh), ("github-main".into(), 120));
        let empty = edit_config(Some("# only a comment\n"), "github-a", 90);
        assert_eq!(secret_and_secs(&empty), ("github-a".into(), 90));
        assert!(empty.contains("# only a comment"));
        let old = "# keep me\n[providers.github]\nsecret = \"github-old\"\n# inside\ninterval_secs = 300\n";
        let new = edit_config(Some(old), "github-new", 60);
        assert_eq!(secret_and_secs(&new), ("github-new".into(), 60));
        assert!(new.starts_with("# keep me\n") && !new.contains("github-old"));
        // Same binding twice: stable.
        assert_eq!(edit_config(Some(&new), "github-new", 60), new);
        // Forms the table finder cannot see fall back to a whole, valid rewrite.
        for odd in [
            "providers.github.secret = \"github-old\"\n",
            "providers = { github = { secret = \"github-old\" } }\n",
        ] {
            let t = edit_config(Some(odd), "github-new", 200);
            assert_eq!(secret_and_secs(&t), ("github-new".into(), 200), "{odd}");
        }
    }

    #[test]
    fn remove_drops_only_the_table() {
        let old = "# keep\n[providers.github]\nsecret = \"github-x\"\n";
        let t = remove_binding(old);
        assert!(Config::parse(&t).unwrap().github().is_none());
        assert!(t.contains("# keep") && !t.contains("github-x"));
        let t = remove_binding("providers.github.secret = \"github-x\"\n");
        assert!(Config::parse(&t).unwrap().github().is_none());
    }

    #[test]
    fn check_line_has_only_a_category_and_counts() {
        let r = |j: &str| render_check(j);
        assert_eq!(
            r(
                r#"{"v":0,"id":1,"ok":true,"topic":"github.prs","ts":1,"stale":false,"data":{"open":3,"review":1}}"#
            ),
            "github.prs: ok open=3 review=1"
        );
        assert_eq!(
            r(
                r#"{"ok":true,"stale":true,"data":{"open":3,"review":1},"state":"rate_limited","until":9}"#
            ),
            "github.prs: rate_limited open=3 review=1"
        );
        assert_eq!(
            r(r#"{"error":{"code":"locked","topic":"github.prs","state":"locked"}}"#),
            "github.prs: locked"
        );
        assert_eq!(
            r(r#"{"error":{"code":"denied","topic":"github.prs"}}"#),
            "github.prs: error denied"
        );
        // Text from upstream or an odd daemon is dropped, never printed.
        for hostile in [
            r#"{"ok":true,"data":{"open":"FAKE-tok","review":-1,"x":"FAKE-tok"},"state":"FAKE-tok"}"#,
            r#"{"error":{"code":"FAKE-tok","state":"FAKE-tok","message":"FAKE-tok"}}"#,
            "FAKE-tok not json",
        ] {
            assert!(!r(hostile).contains("FAKE"), "{hostile}");
        }
    }
}
