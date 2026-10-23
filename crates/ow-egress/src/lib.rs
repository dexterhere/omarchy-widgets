//! The only network door of the workspace (docs/AUTH_PLAN.md section 3.4, DECISIONS AUTH-EGRESS
//! and AUTH-HTTP). Rules, each enforced in code and by a test:
//!
//! - https only; the caller gives a bare host name and a path, never a URL;
//! - the host must equal an allow-list entry exactly (parsed, lowercase ASCII DNS name; no IP
//!   literal, port, userinfo, fragment, backslash, trailing dot, IDN or punycode);
//! - no redirect is ever followed: any 3xx is `EgressError::Redirect` and nothing is re-sent;
//! - proxy environment variables are ignored; there is no custom CA or insecure option;
//! - response size cap, connect and total timeouts (the total also bounds a slow-drip body);
//! - CR, LF and NUL are refused in every caller-provided value;
//! - the credential appears only in the `Authorization: Bearer` header, set by this crate; a
//!   path, header value or body that contains it is refused;
//! - no decompression, a fixed `User-Agent`, whitelisted response headers only;
//! - errors are categories whose `Display` never carries a URL, header, body or token;
//! - this crate writes nothing to stdout, stderr or any log, and `log` macros are compiled out.
//!
//! Honest limits (see AUTH-HTTP):
//!
//! - It cannot stop another process of the same user from reading daemon memory.
//! - The HTTP library copies the header value into buffers that `Zeroizing` cannot reach, so the
//!   token can outlive its `Credential` in freed memory.
//! - No certificate pinning (decided): any CA in the bundled Mozilla root set can issue for an
//!   allow-listed host.
//! - DNS is not trusted or checked; the TLS name check is the protection. Resolved private or
//!   loopback addresses are not refused (optional hardening, not built).
//! - Trust roots are the compiled-in `webpki-roots` bundle, not the system store, so a corporate
//!   TLS-inspection CA is not accepted and requests fail closed with `EgressError::Tls`.
//! - A real handshake against a real provider host is not covered by tests; tests use a local
//!   server and a throwaway root that exist only in `#[cfg(test)]` code.
#![forbid(unsafe_code)]

mod client;
mod credential;
mod error;
mod request;
mod target;

pub use client::{Egress, Limits};
pub use credential::Credential;
pub use error::{EgressError, StatusClass};
pub use request::{Method, Request, Response};

/// Sent on every request, never configurable.
pub const USER_AGENT: &str = concat!("omarchy-widgets-owd/", env!("CARGO_PKG_VERSION"));
