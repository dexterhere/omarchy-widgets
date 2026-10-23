//! Talking to a running `owd` over its socket (one request, one reply line).
use ow_core::Paths;
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

/// Replies are small; anything longer is not our daemon.
const MAX_REPLY: u64 = 64 * 1024;
/// Generous: an unlock runs Argon2 on the daemon side.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Send one request line (newline included) and return the raw reply line.
pub fn call(paths: &Paths, request: &str) -> Result<String, String> {
    // Nothing is written until both checks pass, so a squatter on the socket path gets no bytes.
    let sock = paths.socket();
    match sock.parent().map(ow_core::check_private_dir) {
        Some(Ok(())) => {}
        // No directory means no daemon; connect below reports that.
        Some(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
        _ => {
            return Err(
                "refusing to use the daemon socket: its directory is not private and owned by you"
                    .into(),
            );
        }
    }
    let mut s = UnixStream::connect(&sock).map_err(|e| format!("daemon not reachable: {e}"))?;
    if !rustix::net::sockopt::socket_peercred(&s).is_ok_and(|c| c.uid == rustix::process::geteuid())
    {
        return Err("refusing to use the daemon socket: it is not served by your user".into());
    }
    s.set_read_timeout(Some(TIMEOUT))
        .map_err(|e| e.to_string())?;
    s.set_write_timeout(Some(TIMEOUT))
        .map_err(|e| e.to_string())?;
    s.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(s.take(MAX_REPLY))
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    if line.is_empty() {
        return Err("daemon closed the connection without a reply".into());
    }
    Ok(line.trim_end().to_owned())
}

/// Like `call`, but parses the reply and turns an error reply into a user-facing message.
/// Messages are fixed text per error code: nothing from the request is echoed.
pub fn call_ok(paths: &Paths, request: &str) -> Result<Value, String> {
    let reply: Value =
        serde_json::from_str(&call(paths, request)?).map_err(|_| "unreadable reply".to_string())?;
    match reply["error"]["code"].as_str() {
        None => Ok(reply),
        Some("denied") => Err(
            "unlock failed (wrong passphrase, or the vault cannot be opened). Details are not \
             disclosed."
                .into(),
        ),
        Some("rate_limited") => Err(
            "too many failed attempts, or another attempt is running. Wait, then try again; \
             `ow vault status` shows the remaining time."
                .into(),
        ),
        Some("bad_request") => Err("the daemon refused the request as malformed".into()),
        Some(_) => Err("the daemon reported an error".into()),
    }
}
