//! Shared daemon/CLI plumbing. See docs/DAEMON_SPEC.md sections 4 and 6.
#![forbid(unsafe_code)]

mod audit;
mod grants;
mod harden;
mod paths;
mod peer;
mod provider;
mod ratelimit;

pub use audit::AuditLog;
pub use grants::{Grant, Grants, GrantsError};
pub use harden::{check_private_dir, harden_process, private_dir};
pub use paths::Paths;
pub use peer::peer_is_self;
pub use provider::{
    FetchCtx, Provider, ProviderError, SecretHandle, SecretSource, Sensitivity, TopicSpec,
};
pub use ratelimit::RateLimiter;
