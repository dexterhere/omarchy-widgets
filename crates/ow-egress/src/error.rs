use std::fmt;

/// Class of a non-success HTTP status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StatusClass {
    /// 4xx.
    Client,
    /// 5xx.
    Server,
    /// Anything else that is not 2xx or 3xx.
    Other,
}

/// Failure category. Carries no URL, header, body or token, and neither does `Display`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EgressError {
    /// The request was refused locally (host, scheme, path, header or credential rule). Nothing
    /// was sent.
    Blocked,
    /// TLS failed: untrusted certificate, wrong name, bad handshake.
    Tls,
    /// No connection: DNS, refused, unreachable, dropped.
    Offline,
    /// Connect or total timeout, including a body that arrives too slowly.
    Timeout,
    /// The server answered 3xx. Nothing was followed.
    Redirect,
    /// The response body (or its declared length) exceeds the cap.
    TooLarge,
    /// Non-2xx, non-3xx answer. `headers` holds only whitelisted response headers (rate limit,
    /// retry-after, content-type, the caller's expiry header), so a caller can back off.
    Status {
        class: StatusClass,
        code: u16,
        headers: Vec<(String, String)>,
    },
    /// The answer is not usable HTTP (protocol error, encoded body, bad header).
    Format,
}

impl fmt::Display for EgressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Blocked => "request blocked by egress rules",
            Self::Tls => "TLS failure",
            Self::Offline => "network unavailable",
            Self::Timeout => "timed out",
            Self::Redirect => "unexpected redirect",
            Self::TooLarge => "response too large",
            Self::Status {
                class: StatusClass::Client,
                ..
            } => "upstream rejected the request",
            Self::Status {
                class: StatusClass::Server,
                ..
            } => "upstream server error",
            Self::Status { .. } => "unexpected upstream status",
            Self::Format => "unexpected response format",
        })
    }
}

impl std::error::Error for EgressError {}
