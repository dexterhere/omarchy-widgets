//! Secret input: no-echo prompt on a terminal, plain lines from a pipe, or a prompt on the
//! controlling terminal (`/dev/tty`) regardless of stdin. Never argv.
use rustix::fd::BorrowedFd;
use rustix::stdio::stdin;
use rustix::termios::{
    LocalModes, OptionalActions, SpecialCodeIndex, isatty, tcgetattr, tcsetattr,
};
use std::io::Write;
use zeroize::Zeroizing;

const MAX_LINE: usize = 20 * 1024;
const CTRL_C: u8 = 0x03;
const ESC: u8 = 0x1b;
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

/// Where we are inside a terminal escape sequence (arrow keys, Alt-combos). Only used on a tty.
#[derive(Clone, Copy, PartialEq)]
enum Esc {
    None,
    /// Saw ESC, next byte picks the kind.
    Start,
    /// ESC O: one more byte (SS3, e.g. application-mode arrows).
    Ss3,
    /// ESC [: parameters until a final byte 0x40..=0x7E.
    Csi,
}

/// Read one newline-terminated line byte-by-byte from fd 0 (no std buffering). The buffer is
/// allocated once at full size so no freed, unwiped intermediate copies exist. Strips one
/// trailing `\r`. EOF before a newline is an error: a truncated write must not become a secret.
fn read_line(tty: bool) -> Result<Zeroizing<Vec<u8>>, String> {
    read_line_from(tty, |b| rustix::io::read(stdin(), b))
}

fn read_line_from(
    tty: bool,
    mut read: impl FnMut(&mut [u8]) -> rustix::io::Result<usize>,
) -> Result<Zeroizing<Vec<u8>>, String> {
    let mut out = Zeroizing::new(Vec::with_capacity(MAX_LINE + 1));
    let mut b = [0u8; 1];
    let mut esc = Esc::None;
    loop {
        match read(&mut b) {
            Ok(0) => return Err("unexpected end of input (lines must end with a newline)".into()),
            Ok(_) if b[0] == b'\n' => break,
            Ok(_) if tty && esc != Esc::None => {
                esc = match (esc, b[0]) {
                    (Esc::Start, b'[') => Esc::Csi,
                    (Esc::Start, b'O') => Esc::Ss3,
                    (Esc::Csi, 0x00..=0x3f) => Esc::Csi,
                    _ => Esc::None,
                };
            }
            Ok(_) if tty && b[0] == ESC => esc = Esc::Start,
            Ok(_) if tty && b[0] == CTRL_C => return Err("aborted".into()),
            Ok(_) if tty && BACKSPACE.contains(&b[0]) => {
                // Remove a whole UTF-8 character: continuation bytes, then the lead byte.
                while out.last().is_some_and(|c| c & 0xC0 == 0x80) {
                    out.pop();
                }
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
        // Mode first, then the prompt: input typed once the prompt shows is never flushed.
        let guard = PromptMode::new(fd)?;
        eprint!("{prompt}: ");
        let _ = std::io::stderr().flush();
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

#[cfg(test)]
mod tests {
    use super::read_line_from;

    fn tty_line(input: &[u8]) -> Vec<u8> {
        let mut it = input.iter();
        read_line_from(true, |b| match it.next() {
            Some(&c) => {
                b[0] = c;
                Ok(1)
            }
            None => Ok(0),
        })
        .unwrap()
        .to_vec()
    }

    #[test]
    fn backspace_removes_a_whole_multibyte_char() {
        // "aé€😀" then three backspaces then "b": 😀 (4 bytes), € (3), é (2) are each one erase.
        let got = tty_line("aé€😀\x7f\x7f\x7fb\n".as_bytes());
        assert_eq!(got, b"ab");
        assert_eq!(tty_line(b"\x7f\x08a\n"), b"a");
    }

    #[test]
    fn escape_sequences_are_dropped_not_stored() {
        assert_eq!(tty_line(b"ab\x1b[Dc\x1b[1;5C\x1bOAd\x1bxe\n"), b"abcde");
    }
}
