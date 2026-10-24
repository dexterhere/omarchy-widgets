#![forbid(unsafe_code)]
#![allow(clippy::print_stdout)] // CLI output is its job

mod input;
mod vault_cmd;

use ow::{daemon, doctor, install};
use ow_core::Paths;
use std::process::ExitCode;

fn run() -> Result<(), String> {
    ow_core::harden_process().map_err(|e| format!("hardening failed: {e}"))?;
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let sandbox = args.first().map(String::as_str) == Some("--config-dir") && args.len() >= 2;
    let paths = if sandbox {
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
        Some("doctor") => {
            let json = match &args[1..] {
                [] => false,
                [f] if f == "--json" => true,
                _ => return Err("usage: ow doctor [--json]".into()),
            };
            let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
            let checks = doctor::run(&doctor::Env::new(paths, sandbox, home));
            let text = if json {
                doctor::render_json(&checks)
            } else {
                doctor::render_text(&checks)
            };
            print!("{text}");
            if doctor::failed(&checks) {
                // The report is the output; a bare exit status 1 is the machine-readable part.
                std::process::exit(1);
            }
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
        Some("remove") => install::remove(
            &paths,
            &args[1..],
            &mut std::io::stdout(),
            &mut install::TtyUi,
        ),
        _ => Err(format!(
            "usage: ow [--config-dir DIR] status | doctor [--json] | vault ... | install | list | remove\n{}",
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
