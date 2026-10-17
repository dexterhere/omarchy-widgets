//! Secret input: no-echo prompt on a terminal, plain lines from a pipe. Never argv.
use rustix::fd::BorrowedFd;
use rustix::stdio::stdin;
use rustix::termios::{LocalModes, OptionalActions, isatty, tcgetattr, tcsetattr};
use std::io::Write;
use zeroize::Zeroizing;

const MAX_LINE: usize = 20 * 1024;

/// Restores terminal echo on drop (including early returns).
struct EchoOff<'a> {
    fd: BorrowedFd<'a>,
    saved: rustix::termios::Termios,
}

impl<'a> EchoOff<'a> {
    fn new(fd: BorrowedFd<'a>) -> Result<Self, String> {
        let saved = tcgetattr(fd).map_err(|e| e.to_string())?;
        let mut t = saved.clone();
        t.local_modes.remove(LocalModes::ECHO);
        tcsetattr(fd, OptionalActions::Flush, &t).map_err(|e| e.to_string())?;
        Ok(Self { fd, saved })
    }
}

impl Drop for EchoOff<'_> {
    fn drop(&mut self) {
        let _ = tcsetattr(self.fd, OptionalActions::Flush, &self.saved);
    }
}

/// Read one line byte-by-byte from fd 0 (no std buffering, so no stray copies), strip exactly
/// one trailing `\n` or `\r\n`.
fn read_line() -> Result<Zeroizing<Vec<u8>>, String> {
    let mut out = Zeroizing::new(Vec::new());
    let mut b = [0u8; 1];
    loop {
        match rustix::io::read(stdin(), &mut b) {
            Ok(0) => break,
            Ok(_) if b[0] == b'\n' => break,
            Ok(_) => {
                if out.len() >= MAX_LINE {
                    return Err("input too long".into());
                }
                out.push(b[0]);
            }
            Err(rustix::io::Errno::INTR) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    if out.last() == Some(&b'\r') {
        out.pop();
    }
    Ok(out)
}

/// Prompt (to stderr) without echo when stdin is a terminal; otherwise just read a line.
pub fn secret(prompt: &str) -> Result<Zeroizing<Vec<u8>>, String> {
    let fd = stdin();
    if isatty(fd) {
        eprint!("{prompt}: ");
        let _ = std::io::stderr().flush();
        let guard = EchoOff::new(fd)?;
        let line = read_line();
        drop(guard);
        eprintln!();
        line
    } else {
        read_line()
    }
}

/// Ask twice on a terminal and require a match; from a pipe read once.
pub fn secret_confirmed(prompt: &str) -> Result<Zeroizing<Vec<u8>>, String> {
    let first = secret(prompt)?;
    if isatty(stdin()) {
        let again = secret("Repeat")?;
        if *first != *again {
            return Err("entries do not match".into());
        }
    }
    Ok(first)
}
