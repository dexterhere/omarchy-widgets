//! Secret input: no-echo prompt on a terminal, plain lines from a pipe. Never argv.
use rustix::fd::BorrowedFd;
use rustix::stdio::stdin;
use rustix::termios::{
    LocalModes, OptionalActions, SpecialCodeIndex, isatty, tcgetattr, tcsetattr,
};
use std::io::Write;
use zeroize::Zeroizing;

const MAX_LINE: usize = 20 * 1024;
const CTRL_C: u8 = 0x03;
const BACKSPACE: [u8; 2] = [0x08, 0x7f];

/// Raw-ish mode for the prompt: echo off, canonical line editing off (the kernel line
/// discipline silently truncates lines near 4 KiB) and signal keys off (a SIGINT would kill the
/// process before the terminal is restored; Ctrl-C is handled as input instead).
/// Settings are restored on drop.
struct PromptMode<'a> {
    fd: BorrowedFd<'a>,
    saved: rustix::termios::Termios,
}

impl<'a> PromptMode<'a> {
    fn new(fd: BorrowedFd<'a>) -> Result<Self, String> {
        let saved = tcgetattr(fd).map_err(|e| e.to_string())?;
        let mut t = saved.clone();
        t.local_modes
            .remove(LocalModes::ECHO | LocalModes::ICANON | LocalModes::ISIG);
        t.special_codes[SpecialCodeIndex::VMIN] = 1;
        t.special_codes[SpecialCodeIndex::VTIME] = 0;
        tcsetattr(fd, OptionalActions::Flush, &t).map_err(|e| e.to_string())?;
        Ok(Self { fd, saved })
    }
}

impl Drop for PromptMode<'_> {
    fn drop(&mut self) {
        let _ = tcsetattr(self.fd, OptionalActions::Flush, &self.saved);
    }
}

/// Read one newline-terminated line byte-by-byte from fd 0 (no std buffering). The buffer is
/// allocated once at full size so no freed, unwiped intermediate copies exist. Strips one
/// trailing `\r`. EOF before a newline is an error: a truncated write must not become a secret.
fn read_line(tty: bool) -> Result<Zeroizing<Vec<u8>>, String> {
    let mut out = Zeroizing::new(Vec::with_capacity(MAX_LINE + 1));
    let mut b = [0u8; 1];
    loop {
        match rustix::io::read(stdin(), &mut b) {
            Ok(0) => return Err("unexpected end of input (lines must end with a newline)".into()),
            Ok(_) if b[0] == b'\n' => break,
            Ok(_) if tty && b[0] == CTRL_C => return Err("aborted".into()),
            Ok(_) if tty && BACKSPACE.contains(&b[0]) => {
                out.pop();
            }
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
        let guard = PromptMode::new(fd)?;
        let line = read_line(true);
        drop(guard);
        eprintln!();
        line
    } else {
        read_line(false)
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
