//! `ow vault ...`: offline operations on the vault file. Secrets come only from
//! `input` (terminal prompt or stdin), never from arguments.
use crate::input::{secret, secret_confirmed};
use ow_core::Paths;
use ow_vault::{Profile, Vault, VaultError};
use std::path::Path;

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
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["init"] => {
            ensure_dir(&path)?;
            let pass = secret_confirmed("New vault passphrase (min 12 bytes)")?;
            Vault::create(&path, &pass, Profile::Standard).map_err(err)?;
            writeln!(
                out,
                "vault created (passphrase mode, Argon2id standard profile)"
            )
            .map_err(w)
        }
        ["status"] => match ow_vault::inspect(&path) {
            Ok(i) => writeln!(
                out,
                "vault: present, mode {}, profile {:?}",
                i.mode, i.profile
            )
            .map_err(w),
            Err(VaultError::NotFound) => writeln!(out, "vault: not initialised").map_err(w),
            Err(e) => Err(err(e)),
        },
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
            "usage: ow vault init|status|list|set <name>|remove <name>|rotate|wipe --yes\n\
                  secrets are read from the terminal (no echo) or stdin, never from arguments"
                .into(),
        ),
    }
}
