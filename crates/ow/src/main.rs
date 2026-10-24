#![forbid(unsafe_code)]
#![allow(clippy::print_stdout)] // CLI output is its job

use ow::{connect, daemon, doctor, install, vault_cmd};
use ow_core::Paths;
use std::process::ExitCode;

fn run() -> Result<(), String> {
    ow_core::harden_process().map_err(|e| format!("hardening failed: {e}"))?;
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let sandbox = args.first().map(String::as_str) == Some("--config-dir") && args.len() >= 2;
    let dir = if sandbox {
        let dir = args.remove(1);
        args.remove(0);
        Some(dir)
    } else {
        None
    };
    // `doctor` must still report what it can when HOME or XDG_RUNTIME_DIR is missing.
    if args.first().map(String::as_str) == Some("doctor") {
        return doctor_cmd(dir.as_deref(), &args[1..]);
    }
    let paths = match &dir {
        Some(d) => Paths::from_config_dir(d)?,
        None => Paths::from_env()?,
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
        Some("connect") => connect::run(
            &paths,
            &args[1..],
            &mut std::io::stdout(),
            &mut install::TtyUi,
        ),
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
            "usage: ow [--config-dir DIR] status | doctor [--json] [--names] | vault ... | connect github | install | list | remove\n{}\n{}",
            connect::USAGE,
            install::USAGE
        )),
    }
}

fn doctor_cmd(dir: Option<&str>, flags: &[String]) -> Result<(), String> {
    let (mut json, mut names) = (false, false);
    for f in flags {
        match f.as_str() {
            "--json" => json = true,
            "--names" => names = true,
            _ => return Err("usage: ow doctor [--json] [--names]".into()),
        }
    }
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let mut env = match dir {
        Some(d) => doctor::Env::new(Paths::from_config_dir(d)?, true, home),
        None => doctor::Env::from_lookup(|k| std::env::var_os(k).map(std::path::PathBuf::from)),
    };
    env.names = names;
    let checks = doctor::run(&env);
    print!(
        "{}",
        if json {
            doctor::render_json(&checks)
        } else {
            doctor::render_text(&checks)
        }
    );
    if doctor::failed(&checks) {
        // The report is the output; exit status 1 is the machine-readable part.
        std::process::exit(1);
    }
    Ok(())
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
