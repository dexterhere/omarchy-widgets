use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL: u32 = 0;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    BadRequest,
    Denied,
    UnknownTopic,
    ProviderError,
    RateLimited,
    Locked,
    UnsupportedVersion,
    Internal,
}

/// Never carries secrets, tokens or upstream bodies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    Hello {
        client: String,
        #[serde(rename = "clientVersion")]
        client_version: String,
    },
    Snapshot {
        widget: String,
        topic: String,
    },
    Subscribe {
        widget: String,
        topic: String,
        #[serde(default)]
        params: Value,
    },
    Unsubscribe {
        sub: u64,
    },
    Status,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Request {
    pub v: u32,
    pub id: u64,
    #[serde(flatten)]
    pub op: Op,
}

impl Request {
    /// Parse one line. Wrong protocol version and unknown ops are protocol errors,
    /// not parse errors, so the caller can answer with the right `id`.
    pub fn parse(line: &str) -> Result<Request, ErrorCode> {
        let req: Request = serde_json::from_str(line).map_err(|_| ErrorCode::BadRequest)?;
        if req.v != PROTOCOL {
            return Err(ErrorCode::UnsupportedVersion);
        }
        if req.op == Op::Unknown {
            return Err(ErrorCode::BadRequest);
        }
        Ok(req)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Reply {
    Err {
        v: u32,
        id: u64,
        error: ErrorBody,
    },
    Ok {
        v: u32,
        id: u64,
        ok: bool,
        #[serde(flatten)]
        extra: Value,
    },
}

impl Reply {
    pub fn ok(id: u64, extra: Value) -> Self {
        Reply::Ok {
            v: PROTOCOL,
            id,
            ok: true,
            extra,
        }
    }

    pub fn err(id: u64, code: ErrorCode, topic: Option<String>) -> Self {
        Reply::Err {
            v: PROTOCOL,
            id,
            error: ErrorBody { code, topic },
        }
    }

    pub fn to_line(&self) -> String {
        // Serialization of these types cannot fail; fall back to a fixed internal error.
        let mut s = serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"v":0,"error":{"code":"internal"}}"#.into());
        s.push('\n');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_hello_and_ignores_unknown_fields() {
        let r = Request::parse(
            r#"{"v":0,"id":1,"op":"hello","client":"bridge","clientVersion":"0.1.0","extra":1}"#,
        )
        .unwrap();
        assert_eq!(r.id, 1);
        assert!(matches!(r.op, Op::Hello { .. }));
    }

    #[test]
    fn subscribe_params_default() {
        let r =
            Request::parse(r#"{"v":0,"id":3,"op":"subscribe","widget":"w","topic":"t"}"#).unwrap();
        assert!(matches!(r.op, Op::Subscribe { .. }));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(Request::parse("nope"), Err(ErrorCode::BadRequest));
        assert_eq!(
            Request::parse(r#"{"v":0,"id":1,"op":"zzz"}"#),
            Err(ErrorCode::BadRequest)
        );
        assert_eq!(
            Request::parse(r#"{"v":0,"id":1,"op":"snapshot"}"#),
            Err(ErrorCode::BadRequest)
        );
        assert_eq!(
            Request::parse(r#"{"v":1,"id":1,"op":"status"}"#),
            Err(ErrorCode::UnsupportedVersion)
        );
        assert_eq!(
            Request::parse(r#"{"v":0,"op":"status"}"#),
            Err(ErrorCode::BadRequest)
        );
    }

    #[test]
    fn reply_shapes() {
        let ok = Reply::ok(1, json!({"daemonVersion":"0.1.0","protocol":0})).to_line();
        assert_eq!(
            ok,
            "{\"v\":0,\"id\":1,\"ok\":true,\"daemonVersion\":\"0.1.0\",\"protocol\":0}\n"
        );
        let e = Reply::err(2, ErrorCode::Denied, Some("github.prs".into())).to_line();
        assert_eq!(
            e,
            "{\"v\":0,\"id\":2,\"error\":{\"code\":\"denied\",\"topic\":\"github.prs\"}}\n"
        );
    }

    /// Error replies must never echo request content.
    #[test]
    fn error_does_not_echo_input() {
        let line = r#"{"v":0,"id":9,"op":"snapshot","widget":"SECRET-xyz"}"#;
        let code = Request::parse(line).unwrap_err();
        assert!(!Reply::err(9, code, None).to_line().contains("SECRET"));
    }
}
