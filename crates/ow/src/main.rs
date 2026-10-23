#![forbid(unsafe_code)]
#![allow(clippy::print_stdout)] // CLI output is its job

mod input;
mod vault_cmd;

use ow::{daemon, install};
use ow_core::Paths;
use std::process::ExitCode;

fn run() -> Result<(), String> {
    ow_core::harden_process().map_err(|e| format!("hardening failed: {e}"))?;
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let paths = if args.first().map(String::as_str) == Some("--config-dir") && args.len() >= 2 {
        let dir = args.remove(1);
        args.remove(0);
        Paths::from_config_dir(&dir)?
    } else {
        Paths::from_env()?
    };
    match args.first().map(String::as_str) {
        Some("status") => {
            println!(
                "{}",
                daemon::call(&paths, "{\"v\":0,\"id\":1,\"op\":\"status\"}\n")?
            );
            Ok(())
        }
        Some("vault") => vault_cmd::run(&paths, &args[1..], &mut std::io::stdout()),
        Some("install") => install::install(
            &paths,
            &args[1..],
            &mut std::io::stdout(),
            &|host| Box::new(install::NetSource::new(host)),
            &mut install::TtyUi,
        ),
        Some("list") if args.len() == 1 => install::list(&paths, &mut std::io::stdout()),
        Some("remove") => install::remove(&paths, &args[1..], &mut std::io::stdout()),
        _ => Err(format!(
            "usage: ow [--config-dir DIR] status | vault ... | install | list | remove\n{}",
            install::USAGE
        )),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ow: {e}");
            ExitCode::FAILURE
        }
    }
}
