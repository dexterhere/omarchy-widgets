#![forbid(unsafe_code)]

use ow_core::Paths;
use owd::{NoTopics, Server};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

fn paths_from_args() -> Result<Paths, String> {
    let mut args = std::env::args().skip(1);
    match (args.next().as_deref(), args.next()) {
        (None, _) => Paths::from_env(),
        (Some("--config-dir"), Some(dir)) => Paths::from_config_dir(&dir),
        _ => Err("usage: owd [--config-dir DIR]".into()),
    }
}

fn run() -> Result<(), String> {
    let paths = paths_from_args()?;
    ow_core::harden_process().map_err(|e| format!("hardening failed: {e}"))?;
    let server =
        Server::bind(&paths, Box::new(NoTopics)).map_err(|e| format!("cannot start: {e}"))?;
    let stop = Arc::new(AtomicBool::new(false));
    for sig in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        signal_hook::flag::register(sig, Arc::clone(&stop)).map_err(|e| e.to_string())?;
    }
    let reload = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGHUP, Arc::clone(&reload))
        .map_err(|e| e.to_string())?;
    let _ = sd_notify::notify(&[sd_notify::NotifyState::Ready]);
    eprintln!("owd {} listening", env!("CARGO_PKG_VERSION"));
    server.run(&stop, &reload);
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("owd: {e}");
            ExitCode::FAILURE
        }
    }
}
