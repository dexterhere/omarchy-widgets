use crate::target::{
    contains, contains_decoded, json_contains, valid_header_name, valid_header_value, valid_host,
    valid_path,
};
use crate::{Credential, EgressError, Method, Request, Response, StatusClass, USER_AGENT};
use std::io::Read;
use std::time::Duration;
use ureq::Agent;
use ureq::config::Config;
use ureq::http::{self, HeaderValue, Uri, header::AUTHORIZATION};
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
use ureq::unversioned::resolver::Resolver;
use ureq::unversioned::transport::DefaultConnector;
use zeroize::Zeroizing;

const HARD_MAX_BODY: usize = 4 * 1024 * 1024;
const HARD_MAX_CONNECT: Duration = Duration::from_secs(30);
const HARD_MAX_TOTAL: Duration = Duration::from_secs(60);
const MAX_REQUEST_BODY: usize = 64 * 1024;
const MAX_REQUEST_HEADERS: usize = 8;
const MAX_RESPONSE_HEADER_BYTES: usize = 16 * 1024;

/// Response headers that may leave this crate (plus the caller's one expiry header).
const SAFE_RESPONSE_HEADERS: &[&str] = &[
    "retry-after",
    "content-type",
    "x-ratelimit-limit",
    "x-ratelimit-remaining",
    "x-ratelimit-reset",
    "x-ratelimit-used",
    "x-ratelimit-resource",
    "ratelimit-limit",
    "ratelimit-remaining",
    "ratelimit-reset",
];

/// Size and time limits. Out-of-range values are clamped to hard maxima (4 MiB, 30 s, 60 s) and
/// to at least 1 byte or 1 ms, so a caller cannot switch the protection off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest accepted response body.
    pub max_body: usize,
    /// DNS plus TCP connect plus TLS setup is bounded by this for the connect phase.
    pub connect: Duration,
    /// Whole request including reading the body, so a slow-drip body is cut off.
    pub total: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_body: 256 * 1024,
            connect: Duration::from_secs(10),
            total: Duration::from_secs(20),
        }
    }
}

impl Limits {
    fn clamped(self) -> Self {
        let floor = Duration::from_millis(1);
        Self {
            max_body: self.max_body.clamp(1, HARD_MAX_BODY),
            connect: self.connect.clamp(floor, HARD_MAX_CONNECT),
            total: self.total.clamp(floor, HARD_MAX_TOTAL),
        }
    }
}

/// Blocking HTTPS client restricted to an allow-list of hosts. One instance per provider.
pub struct Egress {
    agent: Agent,
    hosts: Vec<String>,
    limits: Limits,
}

impl Egress {
    /// `allowed_hosts` are bare lowercase DNS names (for example `api.github.com`). An empty
    /// list, or an entry that is not a plain name, is refused: this is a security boundary, so
    /// a typo must fail loudly rather than widen or silently narrow it.
    pub fn new(allowed_hosts: &[&str]) -> Result<Self, EgressError> {
        Self::with_limits(allowed_hosts, Limits::default())
    }

    pub fn with_limits(allowed_hosts: &[&str], limits: Limits) -> Result<Self, EgressError> {
        let tls = TlsConfig::builder()
            .provider(TlsProvider::Rustls)
            .root_certs(RootCerts::WebPki)
            .build();
        Self::assemble(
            allowed_hosts,
            limits,
            tls,
            ureq::unversioned::resolver::DefaultResolver::default(),
        )
    }

    /// The single place where an agent is configured; production and tests share it, so the
    /// tests exercise the real settings. Only the TLS roots and the resolver differ in tests.
    fn assemble(
        allowed_hosts: &[&str],
        limits: Limits,
        tls: TlsConfig,
        resolver: impl Resolver,
    ) -> Result<Self, EgressError> {
        if allowed_hosts.is_empty() || !allowed_hosts.iter().all(|h| valid_host(h)) {
            return Err(EgressError::Blocked);
        }
        let limits = limits.clamped();
        let config = Config::builder()
            .https_only(true)
            // None: do not read HTTP_PROXY, HTTPS_PROXY, ALL_PROXY, NO_PROXY (any case).
            .proxy(None)
            // 0: never follow, always hand the 3xx back so it can be reported as an error.
            .max_redirects(0)
            .http_status_as_error(false)
            .user_agent(USER_AGENT)
            // No gzip/brotli feature is compiled in; say so on the wire as well.
            .accept_encoding("identity")
            .max_response_header_size(MAX_RESPONSE_HEADER_BYTES)
            .timeout_resolve(Some(limits.connect))
            .timeout_connect(Some(limits.connect))
            .timeout_global(Some(limits.total))
            .tls_config(tls)
            .build();
        Ok(Self {
            agent: Agent::with_parts(config, DefaultConnector::default(), resolver),
            hosts: allowed_hosts.iter().map(|h| (*h).to_owned()).collect(),
            limits,
        })
    }

    /// Test-only: trust the given roots and resolve names through `resolver`, so the real
    /// validation and agent settings run against a local server. Not compiled outside tests.
    #[cfg(test)]
    pub(crate) fn for_tests(
        allowed_hosts: &[&str],
        limits: Limits,
        tls: TlsConfig,
        resolver: impl Resolver,
    ) -> Result<Self, EgressError> {
        Self::assemble(allowed_hosts, limits, tls, resolver)
    }

    #[cfg(test)]
    pub(crate) fn agent(&self) -> &Agent {
        &self.agent
    }

    pub fn send(&self, request: &Request<'_>) -> Result<Response, EgressError> {
        let prepared = self.prepare(request)?;
        let built = http::Request::builder()
            .method(match request.method {
                Method::Get => http::Method::GET,
                Method::Post => http::Method::POST,
            })
            .uri(prepared.uri);
        let built = prepared
            .headers
            .into_iter()
            .fold(built, |b, (n, v)| b.header(n, v));
        let built = match prepared.auth {
            Some(v) => built.header(AUTHORIZATION, v),
            None => built,
        };
        let result = match prepared.body {
            Some(bytes) => {
                let r = built
                    .header("content-type", "application/json")
                    .body(bytes)
                    .map_err(|_| EgressError::Blocked)?;
                self.agent.run(r)
            }
            None => {
                let r = built.body(()).map_err(|_| EgressError::Blocked)?;
                self.agent.run(r)
            }
        };
        let mut resp = result.map_err(classify)?;
        let code = resp.status().as_u16();
        let headers = pick_headers(&resp, request.expiry_header);
        if resp.status().is_redirection() {
            return Err(EgressError::Redirect);
        }
        if !resp.status().is_success() {
            let class = match code {
                400..=499 => StatusClass::Client,
                500..=599 => StatusClass::Server,
                _ => StatusClass::Other,
            };
            return Err(EgressError::Status {
                class,
                code,
                headers,
            });
        }
        let encoded = resp
            .headers()
            .get_all("content-encoding")
            .iter()
            .any(|v| !v.as_bytes().eq_ignore_ascii_case(b"identity"));
        if encoded {
            return Err(EgressError::Format);
        }
        let max = self.limits.max_body;
        if let Some(len) = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
        {
            if len > max as u64 {
                return Err(EgressError::TooLarge);
            }
        }
        let mut body = Vec::new();
        resp.body_mut()
            .as_reader()
            .take(max as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|e| classify(ureq::Error::from(e)))?;
        if body.len() > max {
            return Err(EgressError::TooLarge);
        }
        Ok(Response {
            status: code,
            headers,
            body,
        })
    }

    /// Every local rule, before any socket exists.
    fn prepare(&self, req: &Request<'_>) -> Result<Prepared, EgressError> {
        if !valid_host(req.host) || !self.hosts.iter().any(|h| h == req.host) {
            return Err(EgressError::Blocked);
        }
        if !valid_path(req.path) || req.headers.len() > MAX_REQUEST_HEADERS {
            return Err(EgressError::Blocked);
        }
        if req.expiry_header.is_some_and(|n| !valid_header_name(n)) {
            return Err(EgressError::Blocked);
        }
        let mut headers = Vec::with_capacity(req.headers.len());
        for (name, value) in &req.headers {
            if !valid_header_name(name) || !valid_header_value(value) {
                return Err(EgressError::Blocked);
            }
            headers.push(((*name).to_owned(), (*value).to_owned()));
        }
        let body = match (req.method, req.body) {
            (Method::Post, Some(v)) => {
                let bytes = serde_json::to_vec(v).map_err(|_| EgressError::Blocked)?;
                if bytes.len() > MAX_REQUEST_BODY {
                    return Err(EgressError::Blocked);
                }
                Some(bytes)
            }
            (Method::Get, None) => None,
            _ => return Err(EgressError::Blocked),
        };
        let auth = match req.credential {
            Some(c) => {
                let token = c.expose();
                // The credential may only travel in the Authorization header. Refuse if the
                // caller managed to put it anywhere else (best effort: literal match, also after percent-decoding, and in
                // unserialised JSON strings and keys).
                let leaks = contains_decoded(req.path.as_bytes(), token)
                    || headers
                        .iter()
                        .any(|(_, v)| contains_decoded(v.as_bytes(), token))
                    || req.body.is_some_and(|v| json_contains(v, token))
                    || body.as_deref().is_some_and(|b| contains(b, token));
                if leaks {
                    return Err(EgressError::Blocked);
                }
                Some(bearer(c)?)
            }
            None => None,
        };
        // Re-parse what we are about to send and confirm the parser agrees with our reading.
        let url = format!("https://{}{}", req.host, req.path);
        let uri: Uri = url.parse().map_err(|_| EgressError::Blocked)?;
        let same = uri.scheme_str() == Some("https")
            && uri.host() == Some(req.host)
            && uri.port().is_none()
            && uri.authority().map(|a| a.as_str()) == Some(req.host);
        if !same {
            return Err(EgressError::Blocked);
        }
        Ok(Prepared {
            uri,
            headers,
            body,
            auth,
        })
    }
}

struct Prepared {
    uri: Uri,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    auth: Option<HeaderValue>,
}

fn bearer(c: &Credential) -> Result<HeaderValue, EgressError> {
    let mut v = Zeroizing::new(Vec::with_capacity(7 + c.expose().len()));
    v.extend_from_slice(b"Bearer ");
    v.extend_from_slice(c.expose());
    let mut value = HeaderValue::from_bytes(&v).map_err(|_| EgressError::Blocked)?;
    // Makes the http crate print "Sensitive" instead of the value if it is ever Debug-printed.
    value.set_sensitive(true);
    Ok(value)
}

fn pick_headers(resp: &http::Response<ureq::Body>, extra: Option<&str>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, value) in resp.headers() {
        let n = name.as_str();
        if !(SAFE_RESPONSE_HEADERS.contains(&n) || extra == Some(n)) {
            continue;
        }
        if let Ok(v) = value.to_str() {
            if valid_header_value(v) {
                out.push((n.to_owned(), v.to_owned()));
            }
        }
    }
    out
}

/// Map library errors to categories. The library error is dropped, never formatted: its text
/// can contain URLs.
fn classify(e: ureq::Error) -> EgressError {
    use std::io::ErrorKind::{TimedOut, WouldBlock};
    use ureq::Error as E;
    match e {
        E::Timeout(_) => EgressError::Timeout,
        E::HostNotFound | E::ConnectionFailed => EgressError::Offline,
        E::Io(io) if matches!(io.kind(), TimedOut | WouldBlock) => EgressError::Timeout,
        // The TLS layer reports certificate and handshake failures as io errors that wrap a
        // rustls error.
        E::Io(io) if io.get_ref().is_some_and(|i| i.is::<rustls::Error>()) => EgressError::Tls,
        E::Io(_) => EgressError::Offline,
        E::Tls(_) | E::Pem(_) | E::Rustls(_) => EgressError::Tls,
        E::BodyExceedsLimit(_) => EgressError::TooLarge,
        E::TooManyRedirects | E::RedirectFailed => EgressError::Redirect,
        E::BadUri(_) | E::Http(_) | E::InvalidProxyUrl | E::RequireHttpsOnly(_) => {
            EgressError::Blocked
        }
        _ => EgressError::Format,
    }
}
