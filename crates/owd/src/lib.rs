//! `owd` core: socket server, provider hub and per-connection protocol handling.
//!
//! Identity: `SO_PEERCRED` rejects other uids. It cannot tell the bridge from any other
//! process of the same user (docs/DAEMON_SPEC.md section 2), so this is not a defence
//! against a malicious same-user process.
#![forbid(unsafe_code)]

mod conn;
mod hub;
mod server;
mod unlock;

pub use server::Server;
pub use unlock::{FREE_FAILURES, IDLE_TIMEOUT, LOCKOUT_BASE, LOCKOUT_MAX, VaultPolicy};
