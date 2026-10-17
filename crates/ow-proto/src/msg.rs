use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

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
        params: Map<String, Value>,
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

/// Parse failure; `id` is set when the request id could still be read, so the
/// caller can correlate the error reply.
#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    pub id: Option<u64>,
    pub code: ErrorCode,
}

#[derive(Deserialize)]
struct Probe {
    v: u32,
    id: u64,
}

impl Request {
    /// Parse one line. The version is checked before the typed parse so a newer
    /// protocol gets `unsupported_version` rather than `bad_request`.
    pub fn parse(line: &str) -> Result<Request, ParseError> {
        let fail = |id, code| ParseError { id, code };
        let probe: Probe =
            serde_json::from_str(line).map_err(|_| fail(None, ErrorCode::BadRequest))?;
        if probe.v != PROTOCOL {
            return Err(fail(Some(probe.id), ErrorCode::UnsupportedVersion));
        }
        match serde_json::from_str::<Request>(line) {
            Ok(r) if r.op != Op::Unknown => Ok(r),
            _ => Err(fail(Some(probe.id), ErrorCode::BadRequest)),
        }
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
        extra: Map<String, Value>,
    },
}

impl Reply {
    /// Reserved keys (`v`, `id`, `ok`) in `extra` are dropped so payloads cannot spoof them.
    pub fn ok(id: u64, mut extra: Map<String, Value>) -> Self {
        for k in ["v", "id", "ok"] {
            extra.remove(k);
        }
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
        // Serializing string-keyed maps cannot fail; keep a fixed fallback anyway.
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

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }

    fn code(line: &str) -> (Option<u64>, ErrorCode) {
        let e = Request::parse(line).unwrap_err();
        (e.id, e.code)
    }

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
    fn subscribe_params_default_and_must_be_object() {
        let ok = r#"{"v":0,"id":3,"op":"subscribe","widget":"w","topic":"t"}"#;
        assert!(matches!(
            Request::parse(ok).unwrap().op,
            Op::Subscribe { .. }
        ));
        let bad = r#"{"v":0,"id":3,"op":"subscribe","widget":"w","topic":"t","params":[1]}"#;
        assert_eq!(code(bad), (Some(3), ErrorCode::BadRequest));
    }

    #[test]
    fn errors_keep_the_request_id() {
        assert_eq!(code("nope"), (None, ErrorCode::BadRequest));
        assert_eq!(
            code(r#"{"v":0,"id":42,"op":"zzz"}"#),
            (Some(42), ErrorCode::BadRequest)
        );
        assert_eq!(
            code(r#"{"v":0,"id":5,"op":"snapshot"}"#),
            (Some(5), ErrorCode::BadRequest)
        );
        assert_eq!(
            code(r#"{"v":0,"op":"status"}"#),
            (None, ErrorCode::BadRequest)
        );
    }

    #[test]
    fn newer_version_wins_over_shape_errors() {
        assert_eq!(
            code(r#"{"v":1,"id":7,"op":"snapshot"}"#),
            (Some(7), ErrorCode::UnsupportedVersion)
        );
    }

    #[test]
    fn reply_shapes() {
        let ok = Reply::ok(1, obj(json!({"daemonVersion":"0.1.0","protocol":0}))).to_line();
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

    #[test]
    fn payload_cannot_spoof_reserved_keys() {
        let l = Reply::ok(1, obj(json!({"id":99,"v":7,"ok":false,"x":1}))).to_line();
        assert_eq!(l, "{\"v\":0,\"id\":1,\"ok\":true,\"x\":1}\n");
    }

    /// Error replies must never echo request content.
    #[test]
    fn error_does_not_echo_input() {
        let e =
            Request::parse(r#"{"v":0,"id":9,"op":"snapshot","widget":"SECRET-xyz"}"#).unwrap_err();
        assert!(!Reply::err(9, e.code, None).to_line().contains("SECRET"));
    }
}
