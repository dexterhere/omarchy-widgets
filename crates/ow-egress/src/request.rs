use crate::Credential;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Method {
    Get,
    Post,
}

/// What a provider asks for. The caller gives a bare host name (must be on the allow-list) and
/// a path with optional query, never a scheme or URL. No `Debug`: it may borrow a `Credential`.
pub struct Request<'a> {
    pub(crate) method: Method,
    pub(crate) host: &'a str,
    pub(crate) path: &'a str,
    pub(crate) body: Option<&'a serde_json::Value>,
    pub(crate) credential: Option<&'a Credential>,
    pub(crate) expiry_header: Option<&'a str>,
    pub(crate) headers: Vec<(&'a str, &'a str)>,
}

impl<'a> Request<'a> {
    pub fn get(host: &'a str, path: &'a str) -> Self {
        Self::new(Method::Get, host, path, None)
    }

    pub fn post(host: &'a str, path: &'a str, body: &'a serde_json::Value) -> Self {
        Self::new(Method::Post, host, path, Some(body))
    }

    fn new(
        method: Method,
        host: &'a str,
        path: &'a str,
        body: Option<&'a serde_json::Value>,
    ) -> Self {
        Self {
            method,
            host,
            path,
            body,
            credential: None,
            expiry_header: None,
            headers: Vec::new(),
        }
    }

    /// The wrapper sends it as `Authorization: Bearer ...` and nowhere else.
    pub fn credential(mut self, credential: &'a Credential) -> Self {
        self.credential = Some(credential);
        self
    }

    /// Name of one extra response header to return (the token expiry header of the provider).
    pub fn expiry_header(mut self, name: &'a str) -> Self {
        self.expiry_header = Some(name);
        self
    }

    /// A fixed, non-secret request header (for example an API version). Names such as
    /// `authorization`, `host`, `cookie`, `user-agent` and framing headers are refused when the
    /// request is sent, as is any value containing control characters or the credential.
    pub fn header(mut self, name: &'a str, value: &'a str) -> Self {
        self.headers.push((name, value));
        self
    }
}

/// A 2xx answer.
pub struct Response {
    pub(crate) status: u16,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

impl Response {
    pub fn status(&self) -> u16 {
        self.status
    }

    /// Whitelisted headers only, lowercase names.
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn into_body(self) -> Vec<u8> {
        self.body
    }
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("headers", &self.headers.len())
            .field("body_len", &self.body.len())
            .finish()
    }
}
