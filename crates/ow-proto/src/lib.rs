//! Wire protocol v0 for `owd`: line-delimited JSON, see docs/DAEMON_SPEC.md section 5.
//!
//! Pure, IO-free codec so it can be fuzzed and used from sync or async code.
//! Error values never echo request content (it may contain secrets).
#![forbid(unsafe_code)]

mod codec;
mod msg;

pub use codec::{FrameError, LineDecoder, MAX_LINE};
pub use msg::{
    ErrorBody, ErrorCode, MAX_PASSPHRASE, Op, PROTOCOL, ParseError, Passphrase, Reply, Request,
    unlock_request,
};
