//! Production `ow_core::Http`: a thin adapter over `ow_egress::Egress`, the workspace's one
//! network door. `owd` is the only daemon-side user of `ow-egress` (see the build rule in
//! crates/ow-egress/tests/only_egress_touches_the_network.rs). All rules (https only, exact
//! host allow-list, no redirects, no proxy variables, size cap, timeouts, credential only in
//! the `Authorization` header) are enforced by `ow-egress`; this file only converts types.
use ow_core::{Bearer, Http, HttpError, HttpRequest, HttpResponse, StatusClass};
use ow_egress::{Credential, Egress, EgressError, Request};

pub struct EgressHttp(Egress);

impl EgressHttp {
    /// One instance per provider, restricted to that provider's `egress_hosts()`.
    pub fn new(hosts: &[&str]) -> Result<Self, String> {
        Egress::new(hosts)
            .map(Self)
            .map_err(|e| format!("egress: {e}"))
    }
}

fn convert(e: EgressError) -> HttpError {
    match e {
        EgressError::Blocked => HttpError::Blocked,
        EgressError::Tls => HttpError::Tls,
        EgressError::Offline => HttpError::Offline,
        EgressError::Timeout => HttpError::Timeout,
        EgressError::Redirect => HttpError::Redirect,
        EgressError::TooLarge => HttpError::TooLarge,
        EgressError::Status {
            class,
            code,
            headers,
        } => HttpError::Status {
            class: match class {
                ow_egress::StatusClass::Client => StatusClass::Client,
                ow_egress::StatusClass::Server => StatusClass::Server,
                _ => StatusClass::Other,
            },
            code,
            headers,
        },
        // Format and any future category: the least specific one.
        _ => HttpError::Format,
    }
}

impl Http for EgressHttp {
    fn post_json(
        &self,
        request: &HttpRequest<'_>,
        credential: &Bearer<'_>,
    ) -> Result<HttpResponse, HttpError> {
        // `from_bytes` takes ownership of the copy and wipes it on drop (also when it refuses).
        let credential =
            Credential::from_bytes(credential.expose().as_bytes().to_vec()).map_err(convert)?;
        let mut req =
            Request::post(request.host, request.path, request.body).credential(&credential);
        if let Some(name) = request.extra_response_header {
            req = req.expiry_header(name);
        }
        for (name, value) in request.headers {
            req = req.header(name, value);
        }
        let resp = self.0.send(&req).map_err(convert)?;
        Ok(HttpResponse {
            status: resp.status(),
            headers: resp.headers().to_vec(),
            body: resp.into_body(),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use serde_json::json;

    const TOKEN: &str = "FAKE-pat-0123456789abcdef";

    fn http() -> EgressHttp {
        EgressHttp::new(&["api.github.com"]).unwrap()
    }

    /// Every case below is refused by `ow-egress` before a socket exists, so no test here can
    /// reach the network, even if a rule were to break and the request were accepted by mistake
    /// the target is a host that does not resolve (`.invalid`).
    fn post(
        host: &'static str,
        path: &str,
        headers: &[(&'static str, &'static str)],
        tok: &str,
    ) -> Result<HttpResponse, HttpError> {
        let body = json!({"query": "{ x }"});
        http().post_json(
            &HttpRequest {
                host,
                path,
                body: &body,
                headers,
                extra_response_header: None,
            },
            &Bearer::new(tok),
        )
    }

    #[test]
    fn a_host_outside_the_allow_list_is_blocked() {
        for host in [
            "evil.invalid",
            "api.github.com.evil.invalid",
            "github.com",
            "127.0.0.1",
        ] {
            assert_eq!(
                post(host, "/graphql", &[], TOKEN).err(),
                Some(HttpError::Blocked),
                "{host}"
            );
        }
    }

    #[test]
    fn the_credential_is_refused_anywhere_but_the_authorization_header() {
        let leaky_path = format!("/graphql?t={TOKEN}");
        assert_eq!(
            post("api.github.com", &leaky_path, &[], TOKEN).err(),
            Some(HttpError::Blocked)
        );
        let pct: String = TOKEN.bytes().map(|b| format!("%{b:02x}")).collect();
        let leaky = format!("/graphql?t={pct}");
        assert_eq!(
            post("api.github.com", &leaky, &[], TOKEN).err(),
            Some(HttpError::Blocked)
        );
        // A header may not carry it, and the headers the transport owns may not be set.
        let h: &[(&'static str, &'static str)] = &[("x-trace", TOKEN)];
        assert_eq!(
            post("api.github.com", "/graphql", h, TOKEN).err(),
            Some(HttpError::Blocked)
        );
        let h: &[(&'static str, &'static str)] = &[("authorization", "Bearer other")];
        assert_eq!(
            post("api.github.com", "/graphql", h, TOKEN).err(),
            Some(HttpError::Blocked)
        );
    }

    #[test]
    fn a_malformed_credential_is_blocked_not_sent() {
        for bad in ["", "has space", "line\nbreak", "ünï"] {
            assert_eq!(
                post("api.github.com", "/graphql", &[], bad).err(),
                Some(HttpError::Blocked),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn egress_errors_convert_to_categories_without_content() {
        use EgressError as E;
        assert_eq!(convert(E::Tls), HttpError::Tls);
        assert_eq!(convert(E::Offline), HttpError::Offline);
        assert_eq!(convert(E::Timeout), HttpError::Timeout);
        assert_eq!(convert(E::Redirect), HttpError::Redirect);
        assert_eq!(convert(E::TooLarge), HttpError::TooLarge);
        assert_eq!(convert(E::Format), HttpError::Format);
        assert_eq!(
            convert(E::Status {
                class: ow_egress::StatusClass::Server,
                code: 502,
                headers: vec![("retry-after".into(), "5".into())],
            }),
            HttpError::Status {
                class: StatusClass::Server,
                code: 502,
                headers: vec![("retry-after".into(), "5".into())],
            }
        );
    }

    /// `ow-egress` hands back only whitelisted response headers. If GitHub's rate-limit headers
    /// were missing from that list, a rate limit would silently be reported as a permission
    /// problem, so the two lists are checked against each other (a text check: the egress list
    /// is private to that crate).
    #[test]
    fn egress_passes_the_rate_limit_headers_the_github_provider_reads() {
        let egress = include_str!("../../ow-egress/src/client.rs");
        let list = &egress[egress.find("const SAFE_RESPONSE_HEADERS").unwrap()..];
        let list = &list[..list.find("];").unwrap()];
        for h in ow_providers::GITHUB_RATE_LIMIT_HEADERS {
            assert!(
                list.contains(&format!("\"{h}\"")),
                "ow-egress would drop `{h}`"
            );
        }
    }

    #[test]
    fn an_empty_or_invalid_host_list_is_refused_at_construction() {
        assert!(EgressHttp::new(&[]).is_err());
        assert!(EgressHttp::new(&["https://api.github.com"]).is_err());
    }
}
