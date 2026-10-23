use crate::hub::{self, Hub};
use crate::unlock::{UnlockError, VaultState};
use ow_core::ProviderError;
use ow_core::{AuditLog, Grant, Grants, RateLimiter};
use ow_proto::{ErrorCode, FrameError, LineDecoder, MAX_LINE, Op, Reply, Request};
use serde_json::{Map, Value, json};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::{Duration, Instant};
use zeroize::{Zeroize, Zeroizing};

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
    pub vault: Arc<VaultState>,
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

    /// Audit an admin event. Fixed vocabulary, no payload: who is always "admin", the topic is
    /// "vault" and the op is one of unlock, lock, autolock, lockout, or the name of
    /// an admin op that was refused.
    fn audit_admin(&self, op: &str, allow: bool) {
        lock(&self.audit).record("admin", "vault", op, allow, 0);
    }

    /// Drop the key and everything derived from it.
    pub(crate) fn lock_vault(&self) {
        self.hub.lock_vault();
    }

    /// Idle check, run by the scheduler: purge and audit once when the timer locked the vault.
    pub(crate) fn vault_tick(&self) {
        if self.vault.tick() {
            self.hub.purge_secret_entries();
            self.audit_admin("autolock", true);
        }
    }

    fn admin_unlock(&self, id: u64, pass: &ow_proto::Passphrase) -> Reply {
        match self.vault.unlock(pass.expose()) {
            Ok(()) => {
                // A fresh unlock re-reads the vault, so anything cached from older secrets goes.
                self.hub.purge_secret_entries();
                self.audit_admin("unlock", true);
                Reply::ok(id, state_body("unlocked"))
            }
            Err(UnlockError::Denied { locked_out }) => {
                self.audit_admin("unlock", false);
                if locked_out {
                    self.audit_admin("lockout", false);
                }
                Reply::err(id, ErrorCode::Denied, None)
            }
            // Rejected without trying: audited once per lockout (above), not per attempt, so
            // a flood of attempts cannot flush the audit log.
            Err(UnlockError::LockedOut | UnlockError::Busy) => {
                Reply::err(id, ErrorCode::RateLimited, None)
            }
        }
    }

    fn admin_status(&self, id: u64) -> Reply {
        let s = self.vault.status();
        let mut body = state_body(if s.unlocked { "unlocked" } else { "locked" });
        if s.unlocked {
            body.insert("count".into(), s.names.len().into());
            body.insert("names".into(), json!(s.names));
        }
        if let Some(d) = s.idle_left {
            body.insert("idleSecsLeft".into(), d.as_secs().into());
        }
        if let Some(d) = s.lockout_left {
            body.insert("lockoutSecsLeft".into(), d.as_secs().max(1).into());
        }
        Reply::ok(id, body)
    }
}

fn state_body(state: &str) -> Map<String, Value> {
    json!({ "state": state })
        .as_object()
        .cloned()
        .unwrap_or_default()
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
        ProviderError::Locked => ErrorCode::Locked,
        _ => ErrorCode::ProviderError,
    }
}

/// Grant check, rate limit, audit. Order matters: only granted pairs reach the limiter,
/// so its memory is bounded by the grants file, not by what clients invent.
/// Returns the reply to send, plus the new subscription id when one was created.
/// The grant covering this widget and topic right now (reloads take effect immediately).
fn grant_for(shared: &Shared, widget: &str, topic: &str) -> Option<Grant> {
    shared
        .grants
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .find(widget, topic)
        .cloned()
}

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
    if grant_for(shared, widget, &topic).is_none() {
        return deny(ErrorCode::Denied);
    }
    if !lock(&shared.limiter).allow(widget, &topic, Instant::now()) {
        return deny(ErrorCode::RateLimited);
    }
    let fail = |code| (Reply::err(id, code, Some(topic.clone())), None);
    // A secret-backed topic cannot be served (or subscribed) while the vault is locked.
    if shared.hub.needs_secret(&topic) && !shared.vault.is_unlocked() {
        return deny(ErrorCode::Locked);
    }
    if op == "subscribe" {
        // Validate and register first, so the audit line only says "allow" for what happened.
        // The subscription stays inactive until its reply is queued.
        let sub = match shared
            .hub
            .subscribe(ctx.id, widget, &topic, params, ctx.tx.clone())
        {
            Ok(sub) => sub,
            Err(code) => return fail(code),
        };
        // Fail closed: no audit line, no subscription.
        if !lock(&shared.audit).record(widget, &topic, op, true, 0) {
            shared.hub.unsubscribe(ctx.id, sub);
            return fail(ErrorCode::Internal);
        }
        let body = json!({ "sub": sub })
            .as_object()
            .cloned()
            .unwrap_or_default();
        return (Reply::ok(id, body), Some(sub));
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
            // Re-resolve the grant AFTER the (possibly slow) fetch: a revocation that landed
            // meanwhile must not be answered with the old, wider grant.
            let Some(grant) = grant_for(shared, widget, &topic) else {
                return deny(ErrorCode::Denied);
            };
            let epoch = snap.epoch;
            let data = grant.filter(snap.data);
            let bytes = serde_json::to_string(&data).map_or(0, |s| s.len());
            if !lock(&shared.audit).record(widget, &topic, op, true, bytes) {
                return fail(ErrorCode::Internal);
            }
            // Last gate before the reply is built: the key may have changed during the fetch.
            if !shared.hub.still_valid(&topic, epoch) {
                return fail(ErrorCode::Locked);
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
            if let Some(op) = e.admin_op {
                // Admin op dressed as a widget request: refused and audited, no payload.
                shared.audit_admin(op, false);
            }
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
        Op::VaultUnlock { passphrase } => (shared.admin_unlock(req.id, &passphrase), None),
        Op::VaultLock => {
            shared.lock_vault();
            shared.audit_admin("lock", true);
            (Reply::ok(req.id, state_body("locked")), None)
        }
        Op::VaultStatus => (shared.admin_status(req.id), None),
        Op::Unknown => (Reply::err(req.id, ErrorCode::BadRequest, None), None),
    };
    let mut alive = ctx.send(reply.to_line());
    if let Some(sub) = new_sub {
        // Pushes start only now that the reply is queued ahead of them.
        shared.hub.activate(sub);
        // First value immediately, so clients do not wait a full interval.
        alive &= initial_push(shared, ctx, sub);
    }
    (false, alive)
}

/// First value for a new subscription, if one is already cached (otherwise the scheduler
/// fetches it on its next tick, about 50 ms, without blocking this connection). Same rules as
/// a live push: grant resolved just before sending, audited (fail closed) with its size.
fn initial_push(shared: &Shared, ctx: &Ctx, sub: u64) -> bool {
    let Some((topic, params, widget)) = shared.hub.sub_target(sub) else {
        return true;
    };
    let Some(snap) = shared.hub.cached(&topic, &params) else {
        return true;
    };
    let Some(grant) = grant_for(shared, &widget, &topic) else {
        lock(&shared.audit).record(&widget, &topic, "push", false, 0);
        shared.hub.unsubscribe(ctx.id, sub);
        return true;
    };
    let data = grant.filter(snap.data.clone());
    let bytes = serde_json::to_string(&data).map_or(0, |s| s.len());
    if !lock(&shared.audit).record(&widget, &topic, "push", true, bytes) {
        return true;
    }
    // Checked and queued under the hub state lock: a vault lock cannot slip in between.
    let line = hub::push_line(sub, &topic, &snap, &data);
    shared.hub.send_if_live(sub, &topic, snap.epoch, line)
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
const QUEUE: usize = 16;

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
    // Reads may carry a passphrase: wipe the chunk after every use.
    let mut chunk = Zeroizing::new(vec![0u8; MAX_LINE + 1]);
    let bad_reply = || Reply::err(0, ErrorCode::BadRequest, None).to_line();
    loop {
        let n = match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        let fed = dec.feed(&chunk[..n]);
        chunk[..n].zeroize();
        if fed.is_err() {
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
