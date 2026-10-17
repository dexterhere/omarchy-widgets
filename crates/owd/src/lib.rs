//! `owd` core: socket server and per-connection protocol handling.
//!
//! Identity: `SO_PEERCRED` rejects other uids. It cannot tell the bridge from any other
//! process of the same user (docs/DAEMON_SPEC.md section 2), so this is not a defence
//! against a malicious same-user process.
#![forbid(unsafe_code)]

mod conn;
mod server;

pub use conn::{NoTopics, TopicSource};
pub use server::{Server, harden_process};
