use ow_proto::{ErrorCode, FrameError, LineDecoder, MAX_LINE, Op, Reply, Request};
use serde_json::{Map, json};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

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

fn handle(line: &str) -> Reply {
    match Request::parse(line) {
        Err(e) => Reply::err(e.id.unwrap_or(0), e.code, None),
        Ok(req) => match req.op {
            Op::Hello { .. } | Op::Status => Reply::ok(req.id, info()),
            // Deny by default until grants exist (docs/DAEMON_SPEC.md section 6).
            Op::Snapshot { topic, .. } | Op::Subscribe { topic, .. } => {
                Reply::err(req.id, ErrorCode::Denied, Some(topic))
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

pub(crate) fn serve(mut stream: UnixStream) {
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
                    let r = handle(&line);
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
