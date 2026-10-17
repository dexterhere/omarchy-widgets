#![forbid(unsafe_code)]
#![allow(clippy::print_stdout)] // CLI output is its job

mod input;
mod vault_cmd;

use ow_core::Paths;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::ExitCode;

fn status(paths: &Paths) -> Result<String, String> {
    let mut s =
        UnixStream::connect(paths.socket()).map_err(|e| format!("daemon not reachable: {e}"))?;
    s.write_all(b"{\"v\":0,\"id\":1,\"op\":\"status\"}\n")
        .map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(s)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(line.trim_end().to_string())
}

fn run() -> Result<(), String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let paths = if args.first().map(String::as_str) == Some("--config-dir") && args.len() >= 2 {
        let dir = PathBuf::from(args.remove(1));
        args.remove(0);
        Paths::under(&dir)
    } else {
        Paths::from_env()?
    };
    match args.first().map(String::as_str) {
        Some("status") => {
            println!("{}", status(&paths)?);
            Ok(())
        }
        Some("vault") => vault_cmd::run(&paths, &args[1..], &mut std::io::stdout()),
        _ => Err("usage: ow [--config-dir DIR] status | vault ...".into()),
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
