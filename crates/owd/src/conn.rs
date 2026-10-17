use ow_core::{AuditLog, Grants, RateLimiter};
use ow_proto::{ErrorCode, FrameError, LineDecoder, MAX_LINE, Op, Reply, Request};
use serde_json::{Map, Value, json};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// True only for the daemon's own uid.
fn peer_is_self(stream: &UnixStream) -> bool {
    rustix::net::sockopt::socket_peercred(stream).is_ok_and(|c| c.uid == rustix::process::geteuid())
}

fn info() -> Map<String, serde_json::Value> {
    json!({"daemonVersion": env!("CARGO_PKG_VERSION"), "protocol": ow_proto::PROTOCOL})
        .as_object()
        .cloned()
        .unwrap_or_default()
}

/// Where topic data comes from. Providers implement this in a later step.
pub trait TopicSource: Send + Sync {
    fn snapshot(&self, topic: &str) -> Option<Map<String, Value>>;
}

/// No providers yet: every topic is unknown.
pub struct NoTopics;

impl TopicSource for NoTopics {
    fn snapshot(&self, _topic: &str) -> Option<Map<String, Value>> {
        None
    }
}

pub(crate) struct Shared {
    pub grants: Grants,
    pub audit: Mutex<AuditLog>,
    pub limiter: Mutex<RateLimiter>,
    pub source: Box<dyn TopicSource>,
}

/// Lock, recovering from poison: a panicked handler must not disable auditing.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Grant check, rate limit, audit. Order matters: only granted pairs reach the limiter,
/// so its memory is bounded by the grants file, not by what clients invent.
fn data_op(
    shared: &Shared,
    id: u64,
    op: &str,
    widget: &str,
    topic: String,
    snapshot: bool,
) -> Reply {
    let deny = |code| {
        lock(&shared.audit).record(widget, &topic, op, false, 0);
        Reply::err(id, code, Some(topic.clone()))
    };
    let Some(grant) = shared.grants.find(widget, &topic) else {
        return deny(ErrorCode::Denied);
    };
    if !lock(&shared.limiter).allow(widget, &topic, Instant::now()) {
        return deny(ErrorCode::RateLimited);
    }
    // ponytail: subscribe has no scheduler until the local-providers step; it reports unknown_topic.
    let data = if snapshot {
        shared.source.snapshot(&topic)
    } else {
        None
    };
    let Some(data) = data else {
        lock(&shared.audit).record(widget, &topic, op, true, 0);
        return Reply::err(id, ErrorCode::UnknownTopic, Some(topic));
    };
    let data = grant.filter(data);
    let bytes = serde_json::to_string(&data).map_or(0, |s| s.len());
    lock(&shared.audit).record(widget, &topic, op, true, bytes);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let body = json!({"topic": topic, "ts": ts, "stale": false, "data": data});
    Reply::ok(id, body.as_object().cloned().unwrap_or_default())
}

fn handle(line: &str, shared: &Shared) -> Reply {
    match Request::parse(line) {
        Err(e) => Reply::err(e.id.unwrap_or(0), e.code, None),
        Ok(req) => match req.op {
            Op::Hello { .. } | Op::Status => Reply::ok(req.id, info()),
            Op::Snapshot { widget, topic } => {
                data_op(shared, req.id, "snapshot", &widget, topic, true)
            }
            Op::Subscribe { widget, topic, .. } => {
                data_op(shared, req.id, "subscribe", &widget, topic, false)
            }
            Op::Unsubscribe { .. } | Op::Unknown => Reply::err(req.id, ErrorCode::BadRequest, None),
        },
    }
}

/// Time a new connection has to send its first complete request.
const FIRST_FRAME: Duration = Duration::from_secs(10);
/// Idle limit once a client has proven it speaks the protocol.
const IDLE: Duration = Duration::from_secs(600);
/// A peer that cannot take replies for this long is dropped.
const WRITE: Duration = Duration::from_secs(5);
/// Consecutive unparsable frames tolerated before disconnecting.
const MAX_BAD_FRAMES: u32 = 8;

pub(crate) fn serve(mut stream: UnixStream, shared: &Shared) {
    if !peer_is_self(&stream)
        || stream.set_read_timeout(Some(FIRST_FRAME)).is_err()
        || stream.set_write_timeout(Some(WRITE)).is_err()
    {
        return;
    }
    let mut bad = 0u32;
    let mut dec = LineDecoder::new();
    let mut chunk = vec![0u8; MAX_LINE + 1];
    loop {
        let n = match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        if dec.feed(&chunk[..n]).is_err() {
            let _ = stream.write_all(
                Reply::err(0, ErrorCode::BadRequest, None)
                    .to_line()
                    .as_bytes(),
            );
            return;
        }
        loop {
            let reply = match dec.next_frame() {
                Ok(Some(line)) => {
                    let r = handle(&line, shared);
                    if matches!(r, Reply::Err { .. }) {
                        bad += 1;
                    } else {
                        bad = 0;
                        // A real request was served: switch from the strict to the idle timeout.
                        if stream.set_read_timeout(Some(IDLE)).is_err() {
                            return;
                        }
                    }
                    r
                }
                Ok(None) => break,
                Err(FrameError::InvalidUtf8) => {
                    bad += 1;
                    Reply::err(0, ErrorCode::BadRequest, None)
                }
                Err(FrameError::Oversize) => {
                    let _ = stream.write_all(
                        Reply::err(0, ErrorCode::BadRequest, None)
                            .to_line()
                            .as_bytes(),
                    );
                    return;
                }
            };
            if stream.write_all(reply.to_line().as_bytes()).is_err() || bad >= MAX_BAD_FRAMES {
                return;
            }
        }
    }
}
