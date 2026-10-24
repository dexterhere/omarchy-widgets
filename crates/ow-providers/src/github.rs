//! `github`: `github.prs {open, review}`, two numbers, from one GraphQL request to
//! `api.github.com/graphql` made with the user's fine-grained personal access token
//! (docs/AUTH_PLAN.md 1.1 and 3.4, DECISIONS GITHUB-PROVIDER).
//!
//! Credential rules this file keeps (each has a test):
//! - the token is reached only through `FetchCtx::with_secret`, inside the closure that lends it
//!   to the transport for the one request. It is never stored, returned or formatted;
//! - the transport (`ow_core::Http`) is the only way out. This crate has no HTTP, TLS or socket
//!   dependency, and the token travels only as the separate `Bearer` argument;
//! - the response is parsed into two bounded integers and nothing else. Errors are categories;
//!   no text from GitHub (message, header, body) is kept or forwarded;
//! - the provider never prints or logs.
//!
//! UNVERIFIED AGAINST GITHUB. This was written without network access. Everything below that is a
//! claim about GitHub's behaviour is isolated in a constant marked UNVERIFIED and pinned by one
//! test, so a check against the real API changes one line and one assertion, not the logic:
//! the GraphQL query text (`@me` inside `search`, `first: 1` being accepted for a count-only
//! request), the API-version header, the token-expiry header and its date format, the
//! rate-limit headers, and how a 403 is told apart. `User-Agent` is set by the transport
//! (`ow_egress::USER_AGENT`), not here.
use ow_core::{
    Bearer, FetchCtx, Http, HttpError, HttpRequest, Notice, Provider, ProviderBinding,
    ProviderError, Sensitivity, StatusClass, TopicSpec, header_value,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub const HOST: &str = "api.github.com";
const PATH: &str = "/graphql";
const TOPIC: &str = "github.prs";

/// UNVERIFIED: exact GraphQL syntax. `@me` is documented for the issue search qualifiers; that
/// it works through GraphQL `search` for a fine-grained token, and that `first` is required (or
/// harmless) when only `issueCount` is selected, is assumed.
const QUERY: &str = "query { open: search(query: \"is:pr is:open author:@me\", type: ISSUE, first: 1) { issueCount } review: search(query: \"is:pr is:open review-requested:@me\", type: ISSUE, first: 1) { issueCount } }";
/// UNVERIFIED: REST/GraphQL accept this version header; the value is a date GitHub has published.
const API_VERSION_HEADER: (&str, &str) = ("x-github-api-version", "2022-11-28");
/// UNVERIFIED: name of the response header that carries a token's expiry.
const EXPIRY_HEADER: &str = "github-authentication-token-expiration";
/// UNVERIFIED: its format, `YYYY-MM-DD HH:MM:SS UTC`. Anything else is ignored (no notice).
const EXPIRY_FORMAT: &str = "%Y-%m-%d %H:%M:%S UTC";
/// UNVERIFIED: primary rate-limit headers (remaining count and reset as epoch seconds) and
/// `retry-after` (seconds), used to tell a rate limit from a permission problem on a 403.
const RATE_REMAINING_HEADER: &str = "x-ratelimit-remaining";
const RATE_RESET_HEADER: &str = "x-ratelimit-reset";
const RETRY_AFTER_HEADER: &str = "retry-after";
/// UNVERIFIED: the `type` GraphQL gives an error when the primary limit is hit (HTTP 200).
const GRAPHQL_RATE_LIMITED: &str = "RATE_LIMITED";

/// Response headers this provider reads besides the expiry header. A transport that filters
/// response headers must pass these (`owd` has a test that `ow-egress` does).
pub const RATE_LIMIT_HEADERS: [&str; 3] =
    [RATE_REMAINING_HEADER, RATE_RESET_HEADER, RETRY_AFTER_HEADER];

/// Warn when the token expires within this many seconds (7 days).
const EXPIRES_SOON_SECS: u64 = 7 * 24 * 3600;
/// A count above this is not believed.
const MAX_COUNT: u64 = 1_000_000;

static FIELDS: [&str; 2] = ["open", "review"];

pub struct GitHub {
    http: Arc<dyn Http>,
    secret: &'static str,
    secrets: &'static [&'static str],
    topics: &'static [TopicSpec],
    now: fn() -> u64,
}

fn system_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl GitHub {
    /// `binding` comes from `config.toml` (`ow_core::Config`) and has already passed the secret
    /// name pattern; this is the only way to give the provider a secret name.
    ///
    /// The `Provider` trait returns `'static` slices, so the (single, validated) name and the
    /// topic spec with its configured interval are leaked once. This runs once at daemon start.
    pub fn new(binding: &ProviderBinding, http: Arc<dyn Http>) -> Self {
        let secret: &'static str = Box::leak(binding.secret().to_owned().into_boxed_str());
        let secrets: &'static [&'static str] = Box::leak(Box::new([secret]));
        let topics: &'static [TopicSpec] = Box::leak(Box::new([TopicSpec::new(
            TOPIC,
            Sensitivity::High,
            binding.interval(),
            &FIELDS,
        )]));
        Self {
            http,
            secret,
            secrets,
            topics,
            now: system_now,
        }
    }

    #[cfg(test)]
    fn with_clock(mut self, now: fn() -> u64) -> Self {
        self.now = now;
        self
    }
}

/// Why a response body is not the two counts.
#[derive(Debug, PartialEq, Eq)]
enum BadBody {
    /// GraphQL reported the rate limit.
    RateLimited,
    /// Anything else.
    Format,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Count {
    #[serde(rename = "issueCount")]
    issue_count: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    open: Count,
    review: Count,
}

#[derive(Deserialize)]
struct Reply {
    data: Option<Data>,
    errors: Option<Value>,
}

/// Parse a GraphQL response into `(open, review)`. Only the two integers survive; `errors`
/// (even next to data), `null` data, extra selections, wrong types, negatives and absurd values
/// are all refused. Public for the fuzz target.
pub fn parse_counts(body: &[u8]) -> Result<(u64, u64), ProviderError> {
    parse(body).map_err(|e| match e {
        BadBody::RateLimited => ProviderError::RateLimited { until: None },
        BadBody::Format => ProviderError::Format,
    })
}

fn parse(body: &[u8]) -> Result<(u64, u64), BadBody> {
    let reply: Reply = serde_json::from_slice(body).map_err(|_| BadBody::Format)?;
    if let Some(errors) = reply.errors {
        let limited = errors.as_array().is_some_and(|a| {
            a.iter()
                .any(|e| e.get("type").and_then(Value::as_str) == Some(GRAPHQL_RATE_LIMITED))
        });
        return Err(if limited {
            BadBody::RateLimited
        } else {
            BadBody::Format
        });
    }
    let data = reply.data.ok_or(BadBody::Format)?;
    let (open, review) = (data.open.issue_count, data.review.issue_count);
    if open > MAX_COUNT || review > MAX_COUNT {
        return Err(BadBody::Format);
    }
    Ok((open, review))
}

/// A header holding only decimal digits (at most 18, so it fits a u64): epoch seconds or a
/// delay in seconds. Anything else, a date form of `retry-after` included, is "not given".
fn number(headers: &[(String, String)], name: &str) -> Option<u64> {
    let v = header_value(headers, name)?.trim();
    if v.is_empty() || v.len() > 18 || !v.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    v.parse().ok()
}

/// Epoch seconds of an expiry header value in `EXPIRY_FORMAT`.
fn parse_expiry(v: &str) -> Option<u64> {
    if v.len() > 40 {
        return None;
    }
    let dt = jiff::civil::DateTime::strptime(EXPIRY_FORMAT, v.trim()).ok()?;
    let ts = dt.to_zoned(jiff::tz::TimeZone::UTC).ok()?.timestamp();
    u64::try_from(ts.as_second()).ok()
}

impl GitHub {
    /// 401 is a rejected credential. 403 and 429 are a rate limit when the headers say so (429,
    /// remaining 0, or a retry-after), otherwise a permission problem. The bodies of non-2xx
    /// answers are not available to a provider, so the message text cannot be consulted.
    fn status_error(
        &self,
        class: StatusClass,
        code: u16,
        headers: &[(String, String)],
    ) -> ProviderError {
        match (class, code) {
            (_, 401) => ProviderError::Unauthorized,
            (StatusClass::Client, 403 | 429) => {
                let remaining = number(headers, RATE_REMAINING_HEADER);
                let retry = number(headers, RETRY_AFTER_HEADER);
                if code == 429 || remaining == Some(0) || retry.is_some() {
                    let until = number(headers, RATE_RESET_HEADER)
                        .or_else(|| retry.map(|s| (self.now)().saturating_add(s)));
                    ProviderError::RateLimited { until }
                } else {
                    ProviderError::Insufficient
                }
            }
            _ => ProviderError::Unavailable,
        }
    }

    fn transport_error(&self, e: HttpError) -> ProviderError {
        match e {
            HttpError::Tls | HttpError::Offline | HttpError::Timeout => ProviderError::Offline,
            HttpError::Status {
                class,
                code,
                headers,
            } => self.status_error(class, code, &headers),
            HttpError::TooLarge | HttpError::Format => ProviderError::Format,
            // Blocked (a bug on our side), Redirect (GitHub does not redirect this endpoint) and
            // any future category.
            _ => ProviderError::Unavailable,
        }
    }
}

impl Provider for GitHub {
    fn id(&self) -> &'static str {
        "github"
    }

    fn topics(&self) -> &'static [TopicSpec] {
        self.topics
    }

    fn egress_hosts(&self) -> &'static [&'static str] {
        &[HOST]
    }

    fn secrets(&self) -> &'static [&'static str] {
        self.secrets
    }

    fn fetch(
        &self,
        topic: &str,
        _params: &Map<String, Value>,
        ctx: &FetchCtx<'_>,
    ) -> Result<Map<String, Value>, ProviderError> {
        if topic != TOPIC {
            return Err(ProviderError::UnknownTopic);
        }
        let body = json!({ "query": QUERY });
        let request = HttpRequest {
            host: HOST,
            path: PATH,
            body: &body,
            headers: &[API_VERSION_HEADER],
            extra_response_header: Some(EXPIRY_HEADER),
        };
        // The token exists only inside this closure: it builds the credential and performs the
        // request, and what comes out is the response, which does not contain it.
        let answer = ctx.with_secret(self.secret, |token| {
            self.http.post_json(&request, &Bearer::new(token))
        })?;
        let response = answer.map_err(|e| self.transport_error(e))?;
        let (open, review) = match parse(&response.body) {
            Ok(c) => c,
            Err(BadBody::RateLimited) => {
                let until = number(&response.headers, RATE_RESET_HEADER);
                return Err(ProviderError::RateLimited { until });
            }
            Err(BadBody::Format) => return Err(ProviderError::Format),
        };
        let now = (self.now)();
        if let Some(at) = response.header(EXPIRY_HEADER).and_then(parse_expiry)
            && at > now
            && at - now <= EXPIRES_SOON_SECS
        {
            ctx.notify(Notice::ExpiresSoon { at });
        }
        let mut out = Map::new();
        out.insert("open".into(), open.into());
        out.insert("review".into(), review.into());
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use ow_core::{HttpResponse, SecretSource};
    use std::sync::Mutex;

    const TOKEN: &str = "FAKE-pat-0123456789abcdef";
    const NOW: u64 = 1_800_000_000;

    /// What the fake transport saw.
    #[derive(Clone)]
    struct Seen {
        host: String,
        path: String,
        body: String,
        headers: Vec<(String, String)>,
        extra: Option<String>,
        credential: String,
    }

    struct Fake {
        answer: Mutex<Result<HttpResponse, HttpError>>,
        seen: Mutex<Vec<Seen>>,
    }

    impl Fake {
        fn new(answer: Result<HttpResponse, HttpError>) -> Arc<Self> {
            Arc::new(Fake {
                answer: Mutex::new(answer),
                seen: Mutex::new(Vec::new()),
            })
        }
    }

    impl Http for Fake {
        fn post_json(
            &self,
            r: &HttpRequest<'_>,
            c: &Bearer<'_>,
        ) -> Result<HttpResponse, HttpError> {
            self.seen.lock().unwrap().push(Seen {
                host: r.host.into(),
                path: r.path.into(),
                body: r.body.to_string(),
                headers: r
                    .headers
                    .iter()
                    .map(|(n, v)| ((*n).into(), (*v).into()))
                    .collect(),
                extra: r.extra_response_header.map(Into::into),
                credential: c.expose().into(),
            });
            self.answer.lock().unwrap().clone()
        }
    }

    struct Src(Option<&'static str>);
    impl SecretSource for Src {
        fn with_secret(&self, name: &str, f: &mut dyn FnMut(&str)) -> Result<(), ProviderError> {
            match (name, self.0) {
                ("github-main", Some(v)) => {
                    f(v);
                    Ok(())
                }
                _ => Err(ProviderError::NoSecret),
            }
        }
    }

    fn binding(text: &str) -> ProviderBinding {
        ow_core::Config::parse(text)
            .unwrap()
            .github()
            .unwrap()
            .clone()
    }

    fn provider(http: Arc<dyn Http>) -> GitHub {
        GitHub::new(
            &binding("[providers.github]\nsecret = \"github-main\"\n"),
            http,
        )
        .with_clock(|| NOW)
    }

    fn ok(body: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, HttpError> {
        Ok(HttpResponse {
            status: 200,
            headers: headers
                .iter()
                .map(|(n, v)| ((*n).into(), (*v).into()))
                .collect(),
            body: body.as_bytes().to_vec(),
        })
    }

    const GOOD: &str = r#"{"data":{"open":{"issueCount":4},"review":{"issueCount":2}}}"#;

    fn status(code: u16, headers: &[(&str, &str)]) -> Result<HttpResponse, HttpError> {
        let class = match code {
            400..=499 => StatusClass::Client,
            500..=599 => StatusClass::Server,
            _ => StatusClass::Other,
        };
        Err(HttpError::Status {
            class,
            code,
            headers: headers
                .iter()
                .map(|(n, v)| ((*n).into(), (*v).into()))
                .collect(),
        })
    }

    type Run = (
        Result<Map<String, Value>, ProviderError>,
        Option<Notice>,
        Arc<Fake>,
    );

    /// Run one fetch against the fake with the fake token available.
    fn run(answer: Result<HttpResponse, HttpError>) -> Run {
        let fake = Fake::new(answer);
        let p = provider(fake.clone());
        let src = Src(Some(TOKEN));
        let ctx = FetchCtx::new(p.secrets(), &src);
        let r = p.fetch(TOPIC, &Map::new(), &ctx);
        (r, ctx.take_notice(), fake)
    }

    // ---- constants that describe GitHub, pinned one by one (UNVERIFIED) ----

    #[test]
    fn unverified_query_text() {
        assert!(QUERY.contains("is:pr is:open author:@me"));
        assert!(QUERY.contains("is:pr is:open review-requested:@me"));
        assert!(QUERY.contains("type: ISSUE"));
        assert_eq!(QUERY.matches("issueCount").count(), 2);
        // counts only: nothing that returns titles, authors or repositories
        for word in ["nodes", "edges", "title", "author {", "repository"] {
            assert!(!QUERY.contains(word), "{word}");
        }
    }

    #[test]
    fn unverified_api_version_header() {
        assert_eq!(API_VERSION_HEADER, ("x-github-api-version", "2022-11-28"));
    }

    #[test]
    fn unverified_expiry_header_and_format() {
        assert_eq!(EXPIRY_HEADER, "github-authentication-token-expiration");
        assert_eq!(parse_expiry("2027-01-15 08:00:00 UTC"), Some(1_800_000_000));
        for bad in [
            "",
            "2027-01-15",
            "2027-01-15T08:00:00Z",
            "2027-01-15 08:00:00 +0100",
            "soon",
            "-1",
            "2027-13-45 99:00:00 UTC",
        ] {
            assert_eq!(parse_expiry(bad), None, "{bad}");
        }
        assert_eq!(parse_expiry(&"9".repeat(100)), None);
    }

    #[test]
    fn unverified_rate_limit_headers() {
        assert_eq!(RATE_REMAINING_HEADER, "x-ratelimit-remaining");
        assert_eq!(RATE_RESET_HEADER, "x-ratelimit-reset");
        assert_eq!(RETRY_AFTER_HEADER, "retry-after");
        assert_eq!(GRAPHQL_RATE_LIMITED, "RATE_LIMITED");
    }

    // ---- the request ----

    #[test]
    fn one_request_to_the_graphql_endpoint_with_the_token_only_as_the_credential() {
        let (r, notice, fake) = run(ok(GOOD, &[]));
        let out = r.unwrap();
        assert_eq!(Value::Object(out), json!({"open":4,"review":2}));
        assert_eq!(notice, None);
        let seen = fake.seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "exactly one request per fetch");
        let s = &seen[0];
        assert_eq!(
            (s.host.as_str(), s.path.as_str()),
            ("api.github.com", "/graphql")
        );
        assert_eq!(s.credential, TOKEN);
        assert_eq!(
            s.headers,
            vec![("x-github-api-version".to_owned(), "2022-11-28".to_owned())]
        );
        assert_eq!(
            s.extra.as_deref(),
            Some("github-authentication-token-expiration")
        );
        let body: Value = serde_json::from_str(&s.body).unwrap();
        assert_eq!(body.as_object().unwrap().len(), 1);
        assert_eq!(body["query"], QUERY);
        // The credential is nowhere but the credential argument.
        for place in [&s.host, &s.path, &s.body] {
            assert!(!place.contains(TOKEN));
        }
        assert!(
            s.headers
                .iter()
                .all(|(n, v)| !n.contains(TOKEN) && !v.contains(TOKEN))
        );
    }

    #[test]
    fn unknown_topic_makes_no_request() {
        let fake = Fake::new(ok(GOOD, &[]));
        let p = provider(fake.clone());
        let src = Src(Some(TOKEN));
        let ctx = FetchCtx::new(p.secrets(), &src);
        assert_eq!(
            p.fetch("github.nope", &Map::new(), &ctx),
            Err(ProviderError::UnknownTopic)
        );
        assert!(fake.seen.lock().unwrap().is_empty());
    }

    // ---- the secret ----

    #[test]
    fn missing_or_locked_secret_means_no_request() {
        let fake = Fake::new(ok(GOOD, &[]));
        let p = provider(fake.clone());
        let ctx = FetchCtx::new(p.secrets(), &Src(None));
        assert_eq!(
            p.fetch(TOPIC, &Map::new(), &ctx),
            Err(ProviderError::NoSecret)
        );
        assert!(fake.seen.lock().unwrap().is_empty());
        struct Locked;
        impl SecretSource for Locked {
            fn with_secret(&self, _: &str, _: &mut dyn FnMut(&str)) -> Result<(), ProviderError> {
                Err(ProviderError::Locked)
            }
        }
        let fake = Fake::new(ok(GOOD, &[]));
        let p = provider(fake.clone());
        let ctx = FetchCtx::new(p.secrets(), &Locked);
        assert_eq!(
            p.fetch(TOPIC, &Map::new(), &ctx),
            Err(ProviderError::Locked)
        );
        assert!(fake.seen.lock().unwrap().is_empty());
        // And with no context at all.
        assert_eq!(
            p.fetch(TOPIC, &Map::new(), &FetchCtx::none()),
            Err(ProviderError::NoSecret)
        );
    }

    #[test]
    fn the_provider_can_name_only_its_bound_secret() {
        let p = provider(Fake::new(ok(GOOD, &[])));
        assert_eq!(p.secrets(), ["github-main"]);
        let src = Src(Some(TOKEN));
        let ctx = FetchCtx::new(p.secrets(), &src);
        assert!(ctx.with_secret("github-main", |_| ()).is_ok());
        assert_eq!(
            ctx.with_secret("gmail-app-pw", |_| ()),
            Err(ProviderError::NoSecret)
        );
        // The binding is validated config: another name never reaches the provider.
        assert!(ow_core::Config::parse("[providers.github]\nsecret = \"gmail-app-pw\"\n").is_err());
    }

    // ---- the answer ----

    #[test]
    fn counts_are_parsed_strictly() {
        let p = |s: &str| parse_counts(s.as_bytes());
        assert_eq!(p(GOOD), Ok((4, 2)));
        assert_eq!(
            p(r#"{"data":{"open":{"issueCount":0},"review":{"issueCount":1000000}}}"#),
            Ok((0, 1_000_000))
        );
        // extra top-level keys are tolerated, extra selections are not
        assert_eq!(
            p(r#"{"data":{"open":{"issueCount":1},"review":{"issueCount":1}},"extensions":{}}"#),
            Ok((1, 1))
        );
        for bad in [
            "",
            "null",
            "[]",
            "{}",
            r#"{"data":null}"#,
            r#"{"data":{}}"#,
            r#"{"data":{"open":{"issueCount":1}}}"#,
            r#"{"data":{"open":{"issueCount":-1},"review":{"issueCount":1}}}"#,
            r#"{"data":{"open":{"issueCount":1.5},"review":{"issueCount":1}}}"#,
            r#"{"data":{"open":{"issueCount":"4"},"review":{"issueCount":1}}}"#,
            r#"{"data":{"open":{"issueCount":null},"review":{"issueCount":1}}}"#,
            r#"{"data":{"open":{"issueCount":true},"review":{"issueCount":1}}}"#,
            r#"{"data":{"open":{"issueCount":[1]},"review":{"issueCount":1}}}"#,
            r#"{"data":{"open":{"issueCount":1000001},"review":{"issueCount":1}}}"#,
            r#"{"data":{"open":{"issueCount":18446744073709551616},"review":{"issueCount":1}}}"#,
            r#"{"data":{"open":{"issueCount":1,"nodes":[]},"review":{"issueCount":1}}}"#,
            r#"{"data":{"open":{"issueCount":1},"review":{"issueCount":1},"viewer":{"login":"x"}}}"#,
            r#"{"data":{"open":5,"review":{"issueCount":1}}}"#,
            r#"{"data":{"open":{"issueCount":1},"review":{"issueCount":1}}} trailing"#,
        ] {
            assert_eq!(p(bad), Err(ProviderError::Format), "{bad}");
        }
    }

    #[test]
    fn graphql_errors_are_refused_even_next_to_data() {
        let with_data = r#"{"data":{"open":{"issueCount":1},"review":{"issueCount":1}},"errors":[{"message":"FAKE-upstream-text"}]}"#;
        assert_eq!(
            parse_counts(with_data.as_bytes()),
            Err(ProviderError::Format)
        );
        for e in [r#"[{"message":"x"}]"#, "[]", r#""oops""#, "{}", "0"] {
            let body = format!(r#"{{"data":null,"errors":{e}}}"#);
            assert_eq!(
                parse_counts(body.as_bytes()),
                Err(ProviderError::Format),
                "{e}"
            );
        }
        let limited = r#"{"errors":[{"type":"RATE_LIMITED","message":"FAKE-upstream-text"}]}"#;
        assert_eq!(
            parse_counts(limited.as_bytes()),
            Err(ProviderError::RateLimited { until: None })
        );
    }

    #[test]
    fn a_graphql_rate_limit_carries_the_reset_header_as_a_number() {
        let body = r#"{"errors":[{"type":"RATE_LIMITED"}]}"#;
        let (r, _, _) = run(ok(body, &[("x-ratelimit-reset", "1800000500")]));
        assert_eq!(
            r,
            Err(ProviderError::RateLimited {
                until: Some(1_800_000_500)
            })
        );
        let (r, _, _) = run(ok(body, &[("x-ratelimit-reset", "tomorrow")]));
        assert_eq!(r, Err(ProviderError::RateLimited { until: None }));
    }

    #[test]
    fn upstream_text_never_reaches_the_result_or_its_formatting() {
        let hostile =
            format!(r#"{{"data":null,"errors":[{{"message":"FAKE-upstream {TOKEN}"}}]}}"#);
        let (r, _, _) = run(ok(&hostile, &[("x-evil", TOKEN)]));
        let shown = format!("{r:?} {}", r.clone().unwrap_err());
        assert!(!shown.contains("FAKE") && !shown.contains(TOKEN), "{shown}");
    }

    // ---- statuses ----

    #[test]
    fn statuses_map_to_the_documented_states() {
        let t = |a: Result<HttpResponse, HttpError>| run(a).0.unwrap_err();
        assert_eq!(t(status(401, &[])), ProviderError::Unauthorized);
        assert_eq!(t(status(403, &[])), ProviderError::Insufficient);
        assert_eq!(
            t(status(403, &[("x-ratelimit-remaining", "12")])),
            ProviderError::Insufficient
        );
        assert_eq!(
            t(status(
                403,
                &[
                    ("x-ratelimit-remaining", "0"),
                    ("x-ratelimit-reset", "1800000900")
                ]
            )),
            ProviderError::RateLimited {
                until: Some(1_800_000_900)
            }
        );
        assert_eq!(
            t(status(403, &[("x-ratelimit-remaining", "0")])),
            ProviderError::RateLimited { until: None }
        );
        assert_eq!(
            t(status(429, &[])),
            ProviderError::RateLimited { until: None }
        );
        assert_eq!(
            t(status(403, &[("retry-after", "60")])),
            ProviderError::RateLimited {
                until: Some(NOW + 60)
            }
        );
        // a reset header wins over retry-after; junk values are "not given"
        assert_eq!(
            t(status(
                429,
                &[("retry-after", "60"), ("x-ratelimit-reset", "1800000100")]
            )),
            ProviderError::RateLimited {
                until: Some(1_800_000_100)
            }
        );
        assert_eq!(
            t(status(
                429,
                &[("retry-after", "Wed, 21 Oct 2026 07:28:00 GMT")]
            )),
            ProviderError::RateLimited { until: None }
        );
        assert_eq!(
            t(status(403, &[("x-ratelimit-remaining", "-1")])),
            ProviderError::Insufficient
        );
        assert_eq!(t(status(500, &[])), ProviderError::Unavailable);
        assert_eq!(t(status(502, &[])), ProviderError::Unavailable);
        assert_eq!(
            t(status(503, &[("retry-after", "5")])),
            ProviderError::Unavailable
        );
        assert_eq!(t(status(404, &[])), ProviderError::Unavailable);
        assert_eq!(t(status(422, &[])), ProviderError::Unavailable);
    }

    #[test]
    fn transport_failures_map_to_offline_or_unavailable() {
        let t = |e: HttpError| run(Err(e)).0.unwrap_err();
        for e in [HttpError::Tls, HttpError::Offline, HttpError::Timeout] {
            assert_eq!(t(e), ProviderError::Offline);
        }
        for e in [HttpError::Blocked, HttpError::Redirect] {
            assert_eq!(t(e), ProviderError::Unavailable);
        }
        for e in [HttpError::TooLarge, HttpError::Format] {
            assert_eq!(t(e), ProviderError::Format);
        }
        // 200 with a body that is not JSON
        assert_eq!(
            run(ok("<html>FAKE</html>", &[])).0,
            Err(ProviderError::Format)
        );
    }

    // ---- expiry ----

    #[test]
    fn expiry_within_seven_days_is_a_notice_and_the_data_is_still_served() {
        let at = |secs: u64| {
            let ts = jiff::Timestamp::from_second((NOW + secs) as i64).unwrap();
            ts.strftime("%Y-%m-%d %H:%M:%S UTC").to_string()
        };
        for (secs, expect) in [
            (3 * 86_400, true),
            (7 * 86_400, true),
            (7 * 86_400 + 1, false),
            (60 * 86_400, false),
        ] {
            let (r, notice, _) = run(ok(
                GOOD,
                &[("github-authentication-token-expiration", &at(secs))],
            ));
            assert_eq!(r.unwrap()["open"], 4);
            assert_eq!(
                notice,
                expect.then_some(Notice::ExpiresSoon { at: NOW + secs }),
                "{secs}"
            );
        }
        // already past, garbage, absent, or a date in another format: no notice, data served
        for h in [at(0), "tomorrow".to_owned(), String::new()] {
            let (r, notice, _) = run(ok(GOOD, &[("github-authentication-token-expiration", &h)]));
            assert!(r.is_ok());
            assert_eq!(notice, None, "{h}");
        }
        let (_, notice, _) = run(ok(GOOD, &[]));
        assert_eq!(notice, None);
    }

    // ---- shape ----

    #[test]
    fn topic_spec_follows_the_binding() {
        let http: Arc<dyn Http> = Fake::new(ok(GOOD, &[]));
        let spec = |t: &str| GitHub::new(&binding(t), http.clone()).topics()[0].interval;
        let s = |extra: &str| format!("[providers.github]\nsecret = \"github-a\"\n{extra}");
        assert_eq!(spec(&s("")).as_secs(), 120);
        assert_eq!(spec(&s("interval_secs = 5")).as_secs(), 60);
        assert_eq!(spec(&s("interval_secs = 600")).as_secs(), 600);
        let p = provider(http);
        let t = &p.topics()[0];
        assert_eq!((t.name, t.fields), ("github.prs", &["open", "review"][..]));
        assert_eq!(t.sensitivity, Sensitivity::High);
        assert!(t.params.is_empty());
        assert_eq!(p.egress_hosts(), ["api.github.com"]);
        assert_eq!(p.id(), "github");
    }

    #[test]
    fn declared_topics_are_listed_as_credential_topics() {
        let p = provider(Fake::new(ok(GOOD, &[])));
        for t in p.topics() {
            assert!(crate::CREDENTIAL_TOPICS.contains(&t.name));
        }
    }

    /// The provider never prints or logs, and never asks for a handle that outlives a closure.
    #[test]
    fn source_has_no_output_or_handle_escape() {
        let src = include_str!("github.rs");
        let code = &src[..src.find("#[cfg(test)]\nmod tests").unwrap()];
        for banned in [
            "println!",
            "eprintln!",
            "print!",
            "eprint!",
            "dbg!",
            "log::",
            "tracing::",
            "write!(",
            "format!(",
            "panic!",
            "unwrap()",
            "expect(",
            "ctx.secret(",
            "SecretHandle",
            "std::fs",
            "std::env",
            "std::io",
        ] {
            assert!(!code.contains(banned), "github.rs uses `{banned}`");
        }
    }
}
