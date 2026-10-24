//! Data providers. One module per provider; fixtures for tests.
#![forbid(unsafe_code)]

mod attr;
mod battery;
mod clock;
mod github;
mod system;
mod temp;

pub use battery::Battery;
pub use github::{
    GitHub, HOST as GITHUB_HOST, RATE_LIMIT_HEADERS as GITHUB_RATE_LIMIT_HEADERS,
    parse_counts as parse_github_counts,
};
pub use system::System;

/// Topics that need a credential and are served only when `config.toml` binds one. The daemon
/// answers a request for one of these with state `no_credential` instead of `unknown_topic`
/// while the provider is not configured, so a widget can say "not set up".
pub const CREDENTIAL_TOPICS: &[&str] = &["github.prs"];
