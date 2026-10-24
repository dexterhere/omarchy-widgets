use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fmt;
use zeroize::Zeroizing;

pub const PROTOCOL: u32 = 0;

/// Longest passphrase `vault.unlock` accepts, in bytes (the same bound `ow` applies to input).
/// Hex-encoded on the wire it is 40 KiB, inside the 64 KiB frame limit.
pub const MAX_PASSPHRASE: usize = 20 * 1024;

/// A passphrase in transit. Zeroized on drop; `Debug` is redacted; there is no `Display`,
/// `Serialize` or `Deserialize`, so it cannot be logged or echoed by accident.
pub struct Passphrase(Zeroizing<Vec<u8>>);

impl Passphrase {
    /// The bytes, exactly as the user typed them (not necessarily UTF-8).
    pub fn expose(&self) -> &[u8] {
        &self.0
    }

    fn from_hex(hex: &str) -> Option<Passphrase> {
        let h = hex.as_bytes();
        if h.is_empty() || !h.len().is_multiple_of(2) || h.len() > 2 * MAX_PASSPHRASE {
            return None;
        }
        let nib = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
        let mut out = Zeroizing::new(Vec::with_capacity(h.len() / 2));
        // The length is even (checked above), so there is no remainder to ignore.
        let (pairs, _) = h.as_chunks::<2>();
        for [hi, lo] in pairs {
            out.push(nib(*hi)? << 4 | nib(*lo)?);
        }
        Some(Passphrase(out))
    }
}

impl fmt::Debug for Passphrase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Passphrase(<redacted>)")
    }
}

impl Clone for Passphrase {
    fn clone(&self) -> Self {
        Passphrase(Zeroizing::new(self.0.to_vec()))
    }
}

/// Compares without an early exit. Only for tests; never use it to make a decision.
impl PartialEq for Passphrase {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self
                .0
                .iter()
                .zip(other.0.iter())
                .fold(0, |a, (x, y)| a | (x ^ y))
                == 0
    }
}

/// Build the `vault.unlock` request line (newline included). The hex encoding keeps the
/// passphrase free of JSON escapes (so no parser scratch buffer ever holds it) and lets it be
/// arbitrary bytes. `None` if the passphrase is empty or longer than `MAX_PASSPHRASE`.
pub fn unlock_request(id: u64, passphrase: &[u8]) -> Option<Zeroizing<String>> {
    if passphrase.is_empty() || passphrase.len() > MAX_PASSPHRASE {
        return None;
    }
    let mut s = Zeroizing::new(String::with_capacity(80 + 2 * passphrase.len()));
    s.push_str(&format!(
        "{{\"v\":{PROTOCOL},\"id\":{id},\"op\":\"vault.unlock\",\"passphrase\":\""
    ));
    for b in passphrase {
        for n in [b >> 4, b & 15] {
            s.push(char::from_digit(u32::from(n), 16)?);
        }
    }
    s.push_str("\"}\n");
    Some(s)
}

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

/// Why a credential-backed topic has no fresh value (docs/DAEMON_SPEC.md section 5, "States").
/// A closed set: a widget can tell these apart without the daemon ever sending text that came
/// from upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// The vault is locked; `ow vault unlock`.
    Locked,
    /// No secret is bound to the provider, or the bound name is not in the vault.
    NoCredential,
    /// The source rejected the credential (revoked, wrong, expired).
    Unauthorized,
    /// The source refused for lack of permission.
    Insufficient,
    /// Over the source's limit. `until` (when present) is the epoch second it resets.
    RateLimited,
    /// Data is served and fresh, but the credential expires soon. `until` is the expiry.
    ExpiredSoon,
    /// No route to the source (DNS, connect, TLS, timeout).
    Offline,
    /// The source answered 5xx or something that could not be understood.
    Unavailable,
}

/// A state and the one optional number that goes with it. Serialized flat (`"state"`,
/// `"until"`) into snapshot replies, pushes and error bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateInfo {
    pub state: State,
    /// Epoch seconds: when the state ends (`rate_limited`) or the credential expires
    /// (`expired_soon`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<u64>,
}

impl StateInfo {
    /// Read the state out of a reply, push or error body. `None` when there is none, or when it
    /// is not exactly a known state with an optional unsigned number: anything else (an
    /// unknown name, a string `until`) is ignored rather than passed on.
    pub fn from_fields(fields: &Map<String, Value>) -> Option<StateInfo> {
        let state = serde_json::from_value(fields.get("state")?.clone()).ok()?;
        let until = match fields.get("until") {
            None => None,
            Some(u) => Some(u.as_u64()?),
        };
        Some(StateInfo { state, until })
    }

    /// Add `state` (and `until`) to a reply or push payload.
    pub fn put(&self, out: &mut Map<String, Value>) {
        if let Ok(Value::Object(m)) = serde_json::to_value(self) {
            out.extend(m);
        }
    }
}

/// Never carries secrets, tokens or upstream bodies. `state`/`until` are set for
/// credential-backed topics (see `State`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<State>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<u64>,
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
    /// Admin ops (same uid only; see docs/DAEMON_SPEC.md section 5). `VaultUnlock` is never
    /// built by serde: `Request::parse` decodes it separately so the passphrase never passes
    /// through serde's buffering of tagged/flattened content.
    #[serde(skip_deserializing)]
    VaultUnlock {
        passphrase: Passphrase,
    },
    #[serde(rename = "vault.lock")]
    VaultLock,
    #[serde(rename = "vault.status")]
    VaultStatus,
    /// Re-read `grants.toml` (what SIGHUP does). No payload.
    #[serde(rename = "grants.reload")]
    GrantsReload,
    #[serde(other)]
    Unknown,
}

/// Wire names of the admin ops; widget-facing ops are never in this list.
const ADMIN_OPS: [&str; 4] = [
    "vault.unlock",
    "vault.lock",
    "vault.status",
    "grants.reload",
];

impl Op {
    /// Audit/log name for admin ops, `None` for everything else.
    pub fn admin_name(&self) -> Option<&'static str> {
        match self {
            Op::VaultUnlock { .. } => Some(ADMIN_OPS[0]),
            Op::VaultLock => Some(ADMIN_OPS[1]),
            Op::VaultStatus => Some(ADMIN_OPS[2]),
            Op::GrantsReload => Some(ADMIN_OPS[3]),
            _ => None,
        }
    }
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
    /// Set when an admin op was refused because it claimed a widget identity.
    pub admin_op: Option<&'static str>,
}

/// First pass: only what is needed to route the frame. Unknown fields (including a
/// passphrase) are skipped without being copied.
#[derive(Deserialize)]
struct Probe {
    v: u32,
    id: u64,
    #[serde(default)]
    op: Option<Value>,
    /// True when the key is present, whatever its value (null included).
    #[serde(default, deserialize_with = "present")]
    widget: bool,
}

fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    serde::de::IgnoredAny::deserialize(d).map(|_| true)
}

#[derive(Deserialize)]
struct UnlockBody {
    passphrase: Zeroizing<String>,
}

impl Request {
    /// Parse one line. The version is checked before the typed parse so a newer
    /// protocol gets `unsupported_version` rather than `bad_request`.
    pub fn parse(line: &str) -> Result<Request, ParseError> {
        let fail = |id, code| ParseError {
            id,
            code,
            admin_op: None,
        };
        let probe: Probe =
            serde_json::from_str(line).map_err(|_| fail(None, ErrorCode::BadRequest))?;
        if probe.v != PROTOCOL {
            return Err(fail(Some(probe.id), ErrorCode::UnsupportedVersion));
        }
        let admin = probe
            .op
            .as_ref()
            .and_then(Value::as_str)
            .and_then(|o| ADMIN_OPS.iter().copied().find(|a| *a == o));
        if let Some(name) = admin {
            // Widget requests carry a widget id; admin requests never do. A frame mixing the
            // two is refused so a widget-shaped request can never reach an admin handler.
            if probe.widget {
                return Err(ParseError {
                    admin_op: Some(name),
                    ..fail(Some(probe.id), ErrorCode::Denied)
                });
            }
            if name == "vault.unlock" {
                return parse_unlock(line, probe.id);
            }
        }
        match serde_json::from_str::<Request>(line) {
            Ok(r) if r.op != Op::Unknown => Ok(r),
            _ => Err(fail(Some(probe.id), ErrorCode::BadRequest)),
        }
    }
}

/// `vault.unlock`: the passphrase is deserialized straight into a zeroizing string (no tagged
/// or flattened serde buffering), then decoded into zeroizing bytes. Errors carry no content.
fn parse_unlock(line: &str, id: u64) -> Result<Request, ParseError> {
    let bad = || ParseError {
        id: Some(id),
        code: ErrorCode::BadRequest,
        admin_op: None,
    };
    // Hex has no escapes; refusing backslashes keeps serde's escape scratch buffer out of play.
    if line.contains('\\') {
        return Err(bad());
    }
    let body: UnlockBody = serde_json::from_str(line).map_err(|_| bad())?;
    let passphrase = Passphrase::from_hex(&body.passphrase).ok_or_else(bad)?;
    Ok(Request {
        v: PROTOCOL,
        id,
        op: Op::VaultUnlock { passphrase },
    })
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
            error: ErrorBody {
                code,
                topic,
                state: None,
                until: None,
            },
        }
    }

    /// An error reply that also says why (see `State`).
    pub fn err_state(id: u64, code: ErrorCode, topic: Option<String>, info: StateInfo) -> Self {
        Reply::Err {
            v: PROTOCOL,
            id,
            error: ErrorBody {
                code,
                topic,
                state: Some(info.state),
                until: info.until,
            },
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
    fn state_names_are_the_documented_ones() {
        let all = [
            (State::Locked, "locked"),
            (State::NoCredential, "no_credential"),
            (State::Unauthorized, "unauthorized"),
            (State::Insufficient, "insufficient"),
            (State::RateLimited, "rate_limited"),
            (State::ExpiredSoon, "expired_soon"),
            (State::Offline, "offline"),
            (State::Unavailable, "unavailable"),
        ];
        for (st, name) in all {
            assert_eq!(serde_json::to_value(st).unwrap(), json!(name));
            assert_eq!(serde_json::from_value::<State>(json!(name)).unwrap(), st);
        }
        // Not a state: refused, so a client cannot be fed a made-up category.
        assert!(serde_json::from_value::<State>(json!("rate-limited")).is_err());
        assert!(serde_json::from_value::<State>(json!("FAKE")).is_err());
    }

    #[test]
    fn state_reaches_the_wire_as_an_enum_and_a_number_only() {
        let info = StateInfo {
            state: State::RateLimited,
            until: Some(1_800_000_000),
        };
        let e = Reply::err_state(3, ErrorCode::ProviderError, Some("github.prs".into()), info)
            .to_line();
        assert_eq!(
            e,
            "{\"v\":0,\"id\":3,\"error\":{\"code\":\"provider_error\",\"topic\":\"github.prs\",\"state\":\"rate_limited\",\"until\":1800000000}}\n"
        );
        let mut m = Map::new();
        info.put(&mut m);
        assert_eq!(
            Value::Object(m),
            json!({"state":"rate_limited","until":1_800_000_000u64})
        );
        let mut m = Map::new();
        StateInfo {
            state: State::Offline,
            until: None,
        }
        .put(&mut m);
        assert_eq!(Value::Object(m), json!({"state":"offline"}));
        // Plain errors are unchanged on the wire.
        assert_eq!(
            Reply::err(2, ErrorCode::Denied, None).to_line(),
            "{\"v\":0,\"id\":2,\"error\":{\"code\":\"denied\"}}\n"
        );
    }

    #[test]
    fn from_fields_accepts_only_known_states_and_numbers() {
        let f = |v: Value| StateInfo::from_fields(v.as_object().unwrap());
        assert_eq!(
            f(json!({"state":"rate_limited","until":9,"data":{"a":1}})),
            Some(StateInfo {
                state: State::RateLimited,
                until: Some(9)
            })
        );
        assert_eq!(f(json!({"state":"offline"})).unwrap().until, None);
        assert_eq!(f(json!({"stale":true})), None);
        for bad in [
            json!({"state":"offline","until":"soon"}),
            json!({"state":"offline","until":-3}),
            json!({"state":"Paused until 14:32"}),
            json!({"state":7}),
        ] {
            assert_eq!(f(bad.clone()), None, "{bad}");
        }
    }

    #[test]
    fn error_body_parses_with_and_without_state_and_refuses_text_states() {
        let plain: ErrorBody = serde_json::from_str(r#"{"code":"locked"}"#).unwrap();
        assert_eq!((plain.state, plain.until), (None, None));
        let full: ErrorBody =
            serde_json::from_str(r#"{"code":"provider_error","state":"expired_soon","until":5}"#)
                .unwrap();
        assert_eq!(
            (full.state, full.until),
            (Some(State::ExpiredSoon), Some(5))
        );
        for bad in [
            r#"{"code":"locked","state":"Token expires in 5 days"}"#,
            r#"{"code":"locked","state":{"x":1}}"#,
            r#"{"code":"locked","until":"soon"}"#,
            r#"{"code":"locked","until":-1}"#,
            r#"{"code":"locked","until":1.5}"#,
        ] {
            assert!(serde_json::from_str::<ErrorBody>(bad).is_err(), "{bad}");
        }
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

    const PW: &str = "FAKE-passphrase-Zk3!";

    fn hex(s: &str) -> String {
        s.bytes().map(|b| format!("{b:02x}")).collect()
    }

    fn unlock_line(id: u64, pass_field: &str) -> String {
        format!(r#"{{"v":0,"id":{id},"op":"vault.unlock","passphrase":"{pass_field}"}}"#)
    }

    #[test]
    fn unlock_roundtrips_arbitrary_bytes() {
        let raw: &[u8] = b"p\xff\x00\"\\ \xe2\x82\xac";
        let line = unlock_request(7, raw).unwrap();
        assert!(line.ends_with('\n') && !line[..line.len() - 1].contains('\n'));
        match Request::parse(line.trim_end()).unwrap() {
            Request {
                id: 7,
                op: Op::VaultUnlock { passphrase },
                ..
            } => assert_eq!(passphrase.expose(), raw),
            other => panic!("{other:?}"),
        }
        assert!(unlock_request(1, b"").is_none());
        assert!(unlock_request(1, &vec![b'x'; MAX_PASSPHRASE + 1]).is_none());
        assert!(unlock_request(1, &vec![b'x'; MAX_PASSPHRASE]).is_some());
    }

    #[test]
    fn lock_and_status_parse() {
        for (op, want) in [
            ("vault.lock", Op::VaultLock),
            ("vault.status", Op::VaultStatus),
            ("grants.reload", Op::GrantsReload),
        ] {
            let r = Request::parse(&format!(r#"{{"v":0,"id":2,"op":"{op}"}}"#)).unwrap();
            assert_eq!(r.op, want);
        }
    }

    #[test]
    fn grants_reload_is_an_admin_op_that_never_comes_from_a_widget_shaped_frame() {
        let r = Request::parse(r#"{"v":0,"id":9,"op":"grants.reload"}"#).unwrap();
        assert_eq!((r.id, r.op.admin_name()), (9, Some("grants.reload")));
        // Wrong version and near-miss names are not admin ops.
        assert_eq!(
            code(r#"{"v":1,"id":9,"op":"grants.reload"}"#),
            (Some(9), ErrorCode::UnsupportedVersion)
        );
        for near in [
            "grants.reloa",
            "Grants.reload",
            "grants.reload ",
            "grants.set",
            "grants.",
        ] {
            let l = format!(r#"{{"v":0,"id":9,"op":"{near}"}}"#);
            assert_eq!(code(&l), (Some(9), ErrorCode::BadRequest), "{near}");
        }
        // A widget claim of any shape is refused and named, as for the vault ops.
        for w in ["\"w\"", "null", "{}", "[]", "0"] {
            let e = Request::parse(&format!(
                r#"{{"v":0,"id":9,"op":"grants.reload","widget":{w}}}"#
            ))
            .unwrap_err();
            assert_eq!(
                (e.code, e.admin_op),
                (ErrorCode::Denied, Some("grants.reload"))
            );
        }
        // Not reachable through a data op's topic either.
        let r = Request::parse(
            r#"{"v":0,"id":9,"op":"snapshot","widget":"w","topic":"grants.reload"}"#,
        )
        .unwrap();
        assert!(r.op.admin_name().is_none());
    }

    /// Nothing that formats a request or a parse failure may show the passphrase, in either
    /// form (plain or hex).
    #[test]
    fn debug_and_parse_errors_never_echo_the_passphrase() {
        let ok = Request::parse(&unlock_line(3, &hex(PW))).unwrap();
        let shown = format!("{ok:?} {:?}", ok.op.clone());
        assert!(shown.contains("redacted"));
        for needle in [PW, &hex(PW)] {
            assert!(!shown.contains(needle), "{shown}");
        }
        let bad_lines = [
            unlock_line(3, &format!("{}zz", hex(PW))),        // not hex
            unlock_line(3, &format!("{}0", hex(PW))),         // odd length
            unlock_line(3, ""),                               // empty
            unlock_line(3, &"ab".repeat(MAX_PASSPHRASE + 1)), // too long
            unlock_line(3, &format!("{}\\u0041", hex(PW))),   // escape
            format!(r#"{{"v":0,"id":3,"op":"vault.unlock","passphrase":5,"x":"{PW}"}}"#),
            format!(r#"{{"v":0,"id":3,"op":"vault.unlock","pass":"{PW}"}}"#),
            format!(r#"{{"v":1,"id":3,"op":"vault.unlock","passphrase":"{PW}"}}"#),
            format!(r#"{{"v":0,"id":3,"op":"vault.unlock","widget":"w","passphrase":"{PW}"}}"#),
            format!(
                r#"{{"v":0,"id":3,"op":"snapshot","widget":"w","topic":"t","passphrase":"{PW}"}}"#
            ),
        ];
        for l in &bad_lines {
            let shown = match Request::parse(l) {
                Ok(r) => format!("{r:?}"),
                Err(e) => format!(
                    "{e:?} {}",
                    Reply::err(e.id.unwrap_or(0), e.code.clone(), None).to_line()
                ),
            };
            for needle in [PW, &hex(PW)] {
                assert!(!shown.contains(needle), "{l} -> {shown}");
            }
        }
        // The specific categories.
        assert_eq!(code(&bad_lines[0]), (Some(3), ErrorCode::BadRequest));
        assert_eq!(code(&bad_lines[4]), (Some(3), ErrorCode::BadRequest));
        assert_eq!(
            code(&bad_lines[7]),
            (Some(3), ErrorCode::UnsupportedVersion)
        );
    }

    /// `"widget":null` is still a widget claim: the key's presence is what counts.
    #[test]
    fn null_or_odd_widget_values_still_count_as_a_claim() {
        for w in ["null", "\"\"", "0", "[]", "{}", "false"] {
            for op in [
                "vault.unlock",
                "vault.lock",
                "vault.status",
                "grants.reload",
            ] {
                let l = format!(
                    r#"{{"v":0,"id":4,"op":"{op}","widget":{w},"passphrase":"{}"}}"#,
                    hex(PW)
                );
                let e = Request::parse(&l).unwrap_err();
                assert_eq!((e.code, e.admin_op), (ErrorCode::Denied, Some(op)), "{l}");
            }
        }
    }

    /// A duplicated key must not let a frame be read one way by the router and another way by
    /// the handler: whatever the order, a frame that carries a `widget` key never becomes an
    /// admin request, and a duplicated `op` is refused outright.
    /// True when `$t` implements `$tr` (a probe that compiles either way).
    macro_rules! implements {
        ($t:ty, $tr:path) => {{
            #[allow(dead_code)]
            trait No {
                const V: bool = false;
            }
            struct W<T: ?Sized>(std::marker::PhantomData<T>);
            impl<T: ?Sized> No for W<T> {}
            impl<T: ?Sized + $tr> W<T> {
                #[allow(dead_code)] // unused when the bound does not hold: that is the probe
                const V: bool = true;
            }
            <W<$t>>::V
        }};
    }

    /// The type's doc says a passphrase cannot be displayed, serialized or deserialized.
    #[test]
    fn passphrase_cannot_be_displayed_serialized_or_deserialized() {
        assert!(implements!(String, std::fmt::Display), "probe is broken");
        assert!(!implements!(Passphrase, std::fmt::Display));
        assert!(!implements!(Passphrase, serde::Serialize));
        assert!(!implements!(Passphrase, serde::de::DeserializeOwned));
        assert!(!implements!(Passphrase, Copy));
    }

    #[test]
    fn duplicate_keys_cannot_smuggle_an_admin_op_past_the_widget_rule() {
        for op in [
            "vault.unlock",
            "vault.lock",
            "vault.status",
            "grants.reload",
        ] {
            let pw = hex(PW);
            let lines = [
                format!(
                    r#"{{"v":0,"id":4,"op":"{op}","widget":"w","widget":null,"passphrase":"{pw}"}}"#
                ),
                format!(
                    r#"{{"v":0,"id":4,"op":"{op}","widget":null,"widget":"w","passphrase":"{pw}"}}"#
                ),
                format!(
                    r#"{{"v":0,"id":4,"op":"{op}","op":"snapshot","widget":"w","topic":"t","passphrase":"{pw}"}}"#
                ),
                format!(
                    r#"{{"v":0,"id":4,"op":"snapshot","op":"{op}","widget":"w","topic":"t","passphrase":"{pw}"}}"#
                ),
                format!(r#"{{"v":0,"id":4,"op":"{op}","op":"status","passphrase":"{pw}"}}"#),
                format!(r#"{{"v":0,"id":4,"op":"status","op":"{op}","passphrase":"{pw}"}}"#),
                format!(r#"{{"v":0,"v":1,"id":4,"op":"{op}","passphrase":"{pw}"}}"#),
                format!(r#"{{"v":0,"id":4,"id":5,"op":"{op}","passphrase":"{pw}"}}"#),
            ];
            for l in lines {
                if let Ok(r) = Request::parse(&l) {
                    assert!(
                        r.op.admin_name().is_none() && !l.contains("widget"),
                        "{l} parsed as {:?}",
                        r.op.admin_name()
                    );
                }
            }
        }
    }

    #[test]
    fn admin_ops_claiming_a_widget_are_denied_and_named() {
        for op in [
            "vault.unlock",
            "vault.lock",
            "vault.status",
            "grants.reload",
        ] {
            let e = Request::parse(&format!(
                r#"{{"v":0,"id":4,"op":"{op}","widget":"evil","passphrase":"{}"}}"#,
                hex(PW)
            ))
            .unwrap_err();
            assert_eq!(
                (e.id, e.code, e.admin_op),
                (Some(4), ErrorCode::Denied, Some(op))
            );
        }
        // A data op that merely carries a passphrase field parses as the data op it is.
        let r = Request::parse(&format!(
            r#"{{"v":0,"id":5,"op":"snapshot","widget":"w","topic":"vault.unlock","passphrase":"{PW}"}}"#
        ))
        .unwrap();
        assert!(matches!(r.op, Op::Snapshot { .. }) && r.op.admin_name().is_none());
    }

    /// Admin names reachable only through the explicit parse paths: the serde-derived path
    /// must not be able to build an unlock (it would bypass the zeroizing decode).
    #[test]
    fn serde_path_cannot_build_unlock() {
        assert!(
            serde_json::from_str::<Op>(&format!(
                r#"{{"op":"vault.unlock","passphrase":"{}"}}"#,
                hex(PW)
            ))
            .is_ok_and(|o| o == Op::Unknown)
        );
    }
}
