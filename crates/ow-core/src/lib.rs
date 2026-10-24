//! Shared daemon/CLI plumbing. See docs/DAEMON_SPEC.md sections 4 and 6.
#![forbid(unsafe_code)]

mod audit;
mod config;
mod grants;
mod harden;
mod http;
mod paths;
mod peer;
mod private_file;
mod provider;
mod ratelimit;

pub use audit::AuditLog;
pub use config::{
    Config, ConfigError, DEFAULT_INTERVAL_SECS, MAX_CONFIG_FILE, MAX_INTERVAL_SECS,
    MIN_INTERVAL_SECS, ProviderBinding, secret_name_allowed,
};
pub use grants::{Grant, Grants, GrantsError, MAX_FILE as MAX_GRANTS_FILE};
pub use harden::{check_private_dir, harden_process, private_dir};
pub use http::{
    Bearer, Http, HttpError, HttpRequest, HttpResponse, StatusClass, header as header_value,
};
pub use paths::Paths;
pub use peer::peer_is_self;
pub use provider::{
    FetchCtx, Notice, Provider, ProviderError, SecretHandle, SecretSource, Sensitivity, TopicSpec,
};
pub use ratelimit::RateLimiter;
