//! The transport a network provider talks to (docs/AUTH_PLAN.md 3.4). A provider sees this trait
//! and nothing else: no HTTP library, no TLS, no `ow-egress`. `owd` supplies the production
//! implementation on `ow-egress`; tests supply a fake.
//!
//! Rules a caller can rely on and an implementation must keep:
//! - the credential is passed only as the separate `Bearer` argument and is sent only as
//!   `Authorization: Bearer ...`; it is never part of the path, body or any other header;
//! - `host` must be on the implementation's allow-list, https only, no redirect is followed;
//! - the answer carries a status class, only whitelisted response headers and a bounded body;
//! - errors are categories: no URL, header, body or token in them or in their `Display`.
use serde_json::Value;
use std::fmt;

/// A bearer secret lent for one request. Deliberately no `Display`, `Clone`, `Serialize` or
/// `PartialEq`; `Debug` is redacted. It borrows the string, so it cannot outlive the closure of
/// `FetchCtx::with_secret` that supplied it.
pub struct Bearer<'a>(&'a str);

impl<'a> Bearer<'a> {
    pub fn new(token: &'a str) -> Self {
        Bearer(token)
    }

    /// For transport implementations only: they put it in the `Authorization` header.
    pub fn expose(&self) -> &str {
        self.0
    }
}

impl fmt::Debug for Bearer<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Bearer(<redacted>)")
    }
}

/// One JSON POST. No `Debug`: keep it that way so a future field cannot be printed by accident.
pub struct HttpRequest<'a> {
    /// Bare host name (for example `api.github.com`), not a URL.
    pub host: &'static str,
    /// Absolute path, optional query. Never carries the credential.
    pub path: &'a str,
    pub body: &'a Value,
    /// Fixed, non-secret request headers (an API version, say). The transport refuses names it
    /// owns (`authorization`, `host`, `user-agent`, ...).
    pub headers: &'a [(&'static str, &'static str)],
    /// Name of one extra response header to hand back (the token-expiry header), besides the
    /// rate-limit headers every transport passes.
    pub extra_response_header: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StatusClass {
    /// 4xx.
    Client,
    /// 5xx.
    Server,
    Other,
}

/// A 2xx answer. Header names are lowercase.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        header(&self.headers, name)
    }
}

impl fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// Case-insensitive lookup in a header list.
pub fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum HttpError {
    /// Refused locally; nothing was sent.
    #[error("request blocked")]
    Blocked,
    #[error("TLS failure")]
    Tls,
    /// DNS, connect or network failure.
    #[error("network unavailable")]
    Offline,
    #[error("timed out")]
    Timeout,
    /// A 3xx answer; nothing was followed.
    #[error("unexpected redirect")]
    Redirect,
    #[error("response too large")]
    TooLarge,
    #[error("unexpected response format")]
    Format,
    /// A non-2xx, non-3xx answer. `headers` holds whitelisted response headers only; the body is
    /// not available.
    #[error("upstream status")]
    Status {
        class: StatusClass,
        code: u16,
        headers: Vec<(String, String)>,
    },
}

pub trait Http: Send + Sync {
    fn post_json(
        &self,
        request: &HttpRequest<'_>,
        credential: &Bearer<'_>,
    ) -> Result<HttpResponse, HttpError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_debug_is_redacted() {
        let b = Bearer::new("FAKE-pat-0123456789abcdef");
        assert!(!format!("{b:?}").contains("FAKE"));
        assert_eq!(b.expose(), "FAKE-pat-0123456789abcdef");
    }

    #[test]
    fn header_lookup_ignores_case() {
        let h = vec![("x-ratelimit-reset".to_owned(), "5".to_owned())];
        assert_eq!(header(&h, "X-RateLimit-Reset"), Some("5"));
        assert_eq!(header(&h, "retry-after"), None);
    }

    #[test]
    fn error_display_is_a_category() {
        let e = HttpError::Status {
            class: StatusClass::Client,
            code: 403,
            headers: vec![("x".into(), "FAKE-secret".into())],
        };
        assert!(!e.to_string().contains("FAKE"));
    }
}
