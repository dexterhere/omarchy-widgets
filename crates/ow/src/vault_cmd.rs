//! `ow vault ...`: offline operations on the vault file. Secrets come only from
//! `input` (terminal prompt or stdin), never from arguments.
use crate::daemon;
use crate::input::{secret, secret_confirmed};
use ow_core::Paths;
use ow_vault::{Profile, Vault, VaultError};
use std::path::Path;

const UNLOCK_ID: u64 = 1;

fn err(e: VaultError) -> String {
    e.to_string()
}

fn ensure_dir(p: &Path) -> Result<(), String> {
    match p.parent() {
        Some(dir) => ow_core::private_dir(dir).map_err(|e| e.to_string()),
        None => Ok(()),
    }
}

fn utf8(b: &[u8]) -> Result<&str, String> {
    std::str::from_utf8(b).map_err(|_| "value is not valid UTF-8".to_string())
}

pub fn run(paths: &Paths, args: &[String], out: &mut dyn std::io::Write) -> Result<(), String> {
    let path = paths.vault();
    let w = |e: std::io::Error| e.to_string();
    // Every subcommand, not just init, refuses a vault directory that is not private.
    ensure_dir(&path)?;
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["init"] => {
            let pass = secret_confirmed("New vault passphrase (min 12 bytes)")?;
            Vault::create(&path, &pass, Profile::Standard).map_err(err)?;
            writeln!(
                out,
                "vault created (passphrase mode, Argon2id standard profile)"
            )
            .map_err(w)
        }
        ["status"] => {
            match ow_vault::inspect(&path) {
                Ok(i) => writeln!(
                    out,
                    "vault: present, mode {}, profile {:?}",
                    i.mode, i.profile
                )
                .map_err(w)?,
                Err(VaultError::NotFound) => {
                    writeln!(out, "vault: not initialised").map_err(w)?;
                }
                Err(e) => return Err(err(e)),
            }
            daemon_status(paths, out)
        }
        ["unlock"] => {
            let pass = secret("Vault passphrase")?;
            let req = ow_proto::unlock_request(UNLOCK_ID, &pass)
                .ok_or("passphrase is empty or too long")?;
            daemon::call_ok(paths, &req)?;
            writeln!(out, "daemon unlocked").map_err(w)
        }
        ["lock"] => {
            daemon::call_ok(paths, "{\"v\":0,\"id\":1,\"op\":\"vault.lock\"}\n")?;
            writeln!(out, "daemon locked").map_err(w)
        }
        ["list"] => {
            let v = Vault::open(&path, &secret("Vault passphrase")?).map_err(err)?;
            for n in v.names() {
                writeln!(out, "{n}").map_err(w)?;
            }
            Ok(())
        }
        ["set", name] => {
            let mut v = Vault::open(&path, &secret("Vault passphrase")?).map_err(err)?;
            let value = secret_confirmed(&format!("Value for {name}"))?;
            v.set(name, utf8(&value)?).map_err(err)?;
            writeln!(out, "stored {name}").map_err(w)
        }
        ["remove", name] => {
            let mut v = Vault::open(&path, &secret("Vault passphrase")?).map_err(err)?;
            let removed = v.remove(name).map_err(err)?;
            writeln!(
                out,
                "{}",
                if removed { "removed" } else { "no such secret" }
            )
            .map_err(w)
        }
        ["rotate"] => {
            let mut v = Vault::open(&path, &secret("Current passphrase")?).map_err(err)?;
            let new = secret_confirmed("New passphrase (min 12 bytes)")?;
            v.rotate_passphrase(&new, Profile::Standard).map_err(err)?;
            writeln!(out, "passphrase and data key rotated").map_err(w)
        }
        ["wipe", "--yes"] => {
            ow_vault::wipe(&path).map_err(err)?;
            writeln!(out, "vault file removed. Copies you made elsewhere (backups, snapshots, dotfile repos) are NOT erased and stay decryptable with the passphrase.")
                .map_err(w)
        }
        _ => Err(
            "usage: ow vault init|status|unlock|lock|list|set <name>|remove <name>|rotate|wipe --yes\n\
                  secrets are read from the terminal (no echo) or stdin, never from arguments"
                .into(),
        ),
    }
}

/// Daemon half of `vault status`. An unreachable daemon is not an error here: the file facts
/// above are still true.
fn daemon_status(paths: &Paths, out: &mut dyn std::io::Write) -> Result<(), String> {
    let w = |e: std::io::Error| e.to_string();
    let Ok(v) = daemon::call_ok(paths, "{\"v\":0,\"id\":1,\"op\":\"vault.status\"}\n") else {
        return writeln!(out, "daemon: not running or not reachable").map_err(w);
    };
    if v["state"] == "unlocked" {
        let names: Vec<&str> = v["names"]
            .as_array()
            .map(|a| a.iter().filter_map(|n| n.as_str()).collect())
            .unwrap_or_default();
        writeln!(
            out,
            "daemon: unlocked, {} secret(s) [{}], locks after {} s idle",
            names.len(),
            names.join(", "),
            v["idleSecsLeft"].as_u64().unwrap_or(0)
        )
        .map_err(w)
    } else if let Some(s) = v["lockoutSecsLeft"].as_u64() {
        writeln!(out, "daemon: locked; unlock attempts blocked for {s} s").map_err(w)
    } else {
        writeln!(out, "daemon: locked").map_err(w)
    }
}
