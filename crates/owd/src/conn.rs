use crate::hub::{self, Hub};
use ow_core::ProviderError;
use ow_core::{AuditLog, Grants, RateLimiter};
use ow_proto::{ErrorCode, FrameError, LineDecoder, MAX_LINE, Op, Reply, Request};
use serde_json::{Map, Value, json};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Mutex, MutexGuard, PoisonError, RwLock};
use std::time::{Duration, Instant};

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

pub(crate) struct Shared {
    pub grants: RwLock<Grants>,
    pub grants_path: PathBuf,
    pub audit: Mutex<AuditLog>,
    pub limiter: Mutex<RateLimiter>,
    pub hub: Hub,
}

/// Lock, recovering from poison: a panicked handler must not disable auditing.
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    /// Re-read grants.toml. On any error, drop to "no grants" (deny everything) and
    /// report it: a half-edited or insecure file must never keep old access alive.
    pub(crate) fn reload_grants(&self) -> Result<(), ow_core::GrantsError> {
        let result = Grants::load(&self.grants_path);
        let new = result
            .as_ref()
            .map_or_else(|_| Grants::default(), Clone::clone);
        *self.grants.write().unwrap_or_else(PoisonError::into_inner) = new;
        result.map(drop)
    }
}

/// Everything a request handler needs to answer on this connection.
struct Ctx {
    id: u64,
    tx: SyncSender<String>,
}

impl Ctx {
    /// Queue a line for the writer thread. False once the connection is gone.
    fn send(&self, line: String) -> bool {
        self.tx.send(line).is_ok()
    }
}

fn code_for(e: ProviderError) -> ErrorCode {
    match e {
        ProviderError::UnknownTopic => ErrorCode::UnknownTopic,
        ProviderError::BadParams => ErrorCode::BadRequest,
        _ => ErrorCode::ProviderError,
    }
}

/// Grant check, rate limit, audit. Order matters: only granted pairs reach the limiter,
/// so its memory is bounded by the grants file, not by what clients invent.
/// Returns the reply to send, plus the new subscription id when one was created.
fn data_op(
    shared: &Shared,
    ctx: &Ctx,
    id: u64,
    op: &str,
    widget: &str,
    topic: String,
    params: Map<String, Value>,
) -> (Reply, Option<u64>) {
    let deny = |code| {
        // Best effort: a denied request releases nothing, so a failed audit write is harmless.
        lock(&shared.audit).record(widget, &topic, op, false, 0);
        (Reply::err(id, code, Some(topic.clone())), None)
    };
    let grant = shared
        .grants
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .find(widget, &topic)
        .cloned();
    let Some(grant) = grant else {
        return deny(ErrorCode::Denied);
    };
    if !lock(&shared.limiter).allow(widget, &topic, Instant::now()) {
        return deny(ErrorCode::RateLimited);
    }
    // Fail closed: nothing is released (or subscribed) unless its audit line was written.
    let fail = |code| (Reply::err(id, code, Some(topic.clone())), None);
    if op == "subscribe" {
        if !lock(&shared.audit).record(widget, &topic, op, true, 0) {
            return fail(ErrorCode::Internal);
        }
        return match shared
            .hub
            .subscribe(ctx.id, widget, &topic, params, ctx.tx.clone())
        {
            Ok(sub) => (
                Reply::ok(
                    id,
                    json!({ "sub": sub })
                        .as_object()
                        .cloned()
                        .unwrap_or_default(),
                ),
                Some(sub),
            ),
            Err(code) => fail(code),
        };
    }
    match shared.hub.snapshot(&topic, &params) {
        Err(e) => {
            let code = if lock(&shared.audit).record(widget, &topic, op, true, 0) {
                code_for(e)
            } else {
                ErrorCode::Internal
            };
            fail(code)
        }
        Ok(snap) => {
            let data = grant.filter(snap.data);
            let bytes = serde_json::to_string(&data).map_or(0, |s| s.len());
            if !lock(&shared.audit).record(widget, &topic, op, true, bytes) {
                return fail(ErrorCode::Internal);
            }
            let body = json!({"topic": topic, "ts": snap.ts, "stale": snap.stale, "data": data});
            (
                Reply::ok(id, body.as_object().cloned().unwrap_or_default()),
                None,
            )
        }
    }
}

/// Handle one line; replies (and the initial push after a subscribe) go through the writer
/// queue. Returns whether the frame was unparsable (counts toward the bad-frame cap) and
/// whether the connection is still alive.
fn handle(line: &str, shared: &Shared, ctx: &Ctx) -> (bool, bool) {
    let req = match Request::parse(line) {
        Err(e) => {
            return (
                true,
                ctx.send(Reply::err(e.id.unwrap_or(0), e.code, None).to_line()),
            );
        }
        Ok(r) => r,
    };
    let (reply, new_sub) = match req.op {
        Op::Hello { .. } | Op::Status => (Reply::ok(req.id, info()), None),
        Op::Snapshot { widget, topic } => {
            data_op(shared, ctx, req.id, "snapshot", &widget, topic, Map::new())
        }
        Op::Subscribe {
            widget,
            topic,
            params,
        } => data_op(shared, ctx, req.id, "subscribe", &widget, topic, params),
        Op::Unsubscribe { sub } => {
            let r = if shared.hub.unsubscribe(ctx.id, sub) {
                Reply::ok(req.id, Map::new())
            } else {
                Reply::err(req.id, ErrorCode::BadRequest, None)
            };
            (r, None)
        }
        Op::Unknown => (Reply::err(req.id, ErrorCode::BadRequest, None), None),
    };
    let mut alive = ctx.send(reply.to_line());
    if let Some(sub) = new_sub {
        // First value immediately, after the reply, so clients do not wait a full interval.
        alive &= initial_push(shared, ctx, sub);
    }
    (false, alive)
}

fn initial_push(shared: &Shared, ctx: &Ctx, sub: u64) -> bool {
    let Some((topic, params)) = shared.hub.sub_target(sub) else {
        return true;
    };
    let Ok(snap) = shared.hub.snapshot(&topic, &params) else {
        return true;
    };
    let widget_grant = {
        // The subscription's widget is re-resolved through the same grant check as live pushes.
        let st = shared.hub.widget_of(sub);
        st.and_then(|w| {
            shared
                .grants
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .find(&w, &topic)
                .cloned()
        })
    };
    let Some(grant) = widget_grant else {
        return true;
    };
    let data = grant.filter(snap.data.clone());
    ctx.send(hub::push_line(sub, &topic, &snap, &data))
}

/// Time a new connection has to send its first complete request.
const FIRST_FRAME: Duration = Duration::from_secs(10);
/// Idle limit once a client has proven it speaks the protocol.
const IDLE: Duration = Duration::from_secs(600);
/// A peer that cannot take replies for this long is dropped.
const WRITE: Duration = Duration::from_secs(5);
/// Consecutive unparsable frames tolerated before disconnecting.
const MAX_BAD_FRAMES: u32 = 8;

/// Messages queued per connection before a slow reader is dropped.
const QUEUE: usize = 64;

pub(crate) fn serve(stream: UnixStream, shared: &Shared, conn_id: u64) {
    if !peer_is_self(&stream)
        || stream.set_read_timeout(Some(FIRST_FRAME)).is_err()
        || stream.set_write_timeout(Some(WRITE)).is_err()
    {
        return;
    }
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let (tx, rx) = sync_channel::<String>(QUEUE);
    // One writer thread per connection serialises replies and pushes. It ends when every
    // sender (this handler and the connection's subscriptions) is gone, or on a write error.
    let spawned = std::thread::Builder::new().spawn(move || {
        for line in rx {
            if writer.write_all(line.as_bytes()).is_err() {
                let _ = writer.shutdown(std::net::Shutdown::Both);
                break;
            }
        }
    });
    if spawned.is_err() {
        return;
    }
    let ctx = Ctx { id: conn_id, tx };
    read_loop(stream, shared, &ctx);
    shared.hub.remove_conn(conn_id);
}

fn read_loop(mut stream: UnixStream, shared: &Shared, ctx: &Ctx) {
    let mut bad = 0u32;
    let mut dec = LineDecoder::new();
    let mut chunk = vec![0u8; MAX_LINE + 1];
    let bad_reply = || Reply::err(0, ErrorCode::BadRequest, None).to_line();
    loop {
        let n = match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        if dec.feed(&chunk[..n]).is_err() {
            let _ = ctx.send(bad_reply());
            return;
        }
        loop {
            match dec.next_frame() {
                Ok(Some(line)) => {
                    let (unparsable, alive) = handle(&line, shared, ctx);
                    if !alive {
                        return;
                    }
                    if unparsable {
                        bad += 1;
                    } else {
                        bad = 0;
                        // A well-formed request: switch from the strict to the idle timeout.
                        if stream.set_read_timeout(Some(IDLE)).is_err() {
                            return;
                        }
                    }
                }
                Ok(None) => break,
                Err(FrameError::InvalidUtf8) => {
                    bad += 1;
                    if !ctx.send(bad_reply()) {
                        return;
                    }
                }
                Err(FrameError::Oversize) => {
                    let _ = ctx.send(bad_reply());
                    return;
                }
            }
            if bad >= MAX_BAD_FRAMES {
                return;
            }
        }
    }
}
