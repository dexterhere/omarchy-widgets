//! Shared daemon/CLI plumbing. See docs/DAEMON_SPEC.md sections 4 and 6.
#![forbid(unsafe_code)]

mod audit;
mod grants;
mod harden;
mod paths;
mod ratelimit;

pub use audit::AuditLog;
pub use grants::{Grant, Grants, GrantsError};
pub use harden::{harden_process, private_dir};
pub use paths::Paths;
pub use ratelimit::RateLimiter;
