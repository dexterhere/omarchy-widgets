/// Maximum bytes in one line, excluding the newline.
pub const MAX_LINE: usize = 64 * 1024;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// Fatal: the peer must be disconnected.
    #[error("frame exceeds {MAX_LINE} bytes")]
    Oversize,
    /// The line is dropped; the stream stays usable.
    #[error("frame is not valid UTF-8")]
    InvalidUtf8,
}

/// Upper bound on unconsumed bytes: one full line plus one full partial line.
const MAX_BUFFERED: usize = 2 * (MAX_LINE + 1);

/// Incremental newline splitter with hard size bounds.
///
/// Contract: feed chunks of at most `MAX_LINE + 1` bytes and call `next_frame` until
/// it returns `Ok(None)` before the next `feed`. Violations fail with `Oversize`.
#[derive(Default)]
pub struct LineDecoder {
    buf: Vec<u8>,
    head: usize,
    scanned: usize,
    overflow: bool,
}

impl LineDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), FrameError> {
        if self.overflow {
            return Err(FrameError::Oversize);
        }
        if self.head > 0 {
            self.buf.drain(..self.head);
            self.head = 0;
        }
        if self.buf.len() + bytes.len() > MAX_BUFFERED {
            self.fail();
            return Err(FrameError::Oversize);
        }
        self.buf.extend_from_slice(bytes);
        Ok(())
    }

    /// Next non-empty complete line, `Ok(None)` if more bytes are needed.
    /// Empty lines are skipped so they cannot be used to force replies.
    pub fn next_frame(&mut self) -> Result<Option<String>, FrameError> {
        loop {
            if self.overflow {
                return Err(FrameError::Oversize);
            }
            let start = self.head + self.scanned;
            let Some(off) = self.buf[start..].iter().position(|&b| b == b'\n') else {
                self.scanned = self.buf.len() - self.head;
                if self.scanned > MAX_LINE {
                    self.fail();
                    return Err(FrameError::Oversize);
                }
                return Ok(None);
            };
            let end = start + off;
            let mut line = &self.buf[self.head..end];
            self.head = end + 1;
            self.scanned = 0;
            if line.len() > MAX_LINE {
                self.fail();
                return Err(FrameError::Oversize);
            }
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
            if line.is_empty() {
                continue;
            }
            return String::from_utf8(line.to_vec())
                .map(Some)
                .map_err(|_| FrameError::InvalidUtf8);
        }
    }

    fn fail(&mut self) {
        self.overflow = true;
        self.buf = Vec::new();
        self.head = 0;
        self.scanned = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_and_buffers_partial() {
        let mut d = LineDecoder::new();
        d.feed(b"{\"a\":1}\n{\"b\"").unwrap();
        assert_eq!(d.next_frame().unwrap().as_deref(), Some("{\"a\":1}"));
        assert_eq!(d.next_frame().unwrap(), None);
        d.feed(b":2}\r\n").unwrap();
        assert_eq!(d.next_frame().unwrap().as_deref(), Some("{\"b\":2}"));
    }

    #[test]
    fn oversize_without_newline_is_fatal_and_sticky() {
        let mut d = LineDecoder::new();
        d.feed(&vec![b'x'; MAX_LINE + 1]).unwrap();
        assert_eq!(d.next_frame(), Err(FrameError::Oversize));
        assert_eq!(d.feed(b"\n"), Err(FrameError::Oversize));
        assert_eq!(d.next_frame(), Err(FrameError::Oversize));
    }

    #[test]
    fn oversize_with_newline_is_fatal() {
        let mut d = LineDecoder::new();
        let mut v = vec![b'x'; MAX_LINE + 1];
        v.push(b'\n');
        d.feed(&v).unwrap();
        assert_eq!(d.next_frame(), Err(FrameError::Oversize));
    }

    #[test]
    fn exactly_max_is_ok() {
        let mut d = LineDecoder::new();
        let mut v = vec![b'x'; MAX_LINE];
        v.push(b'\n');
        d.feed(&v).unwrap();
        assert_eq!(d.next_frame().unwrap().map(|s| s.len()), Some(MAX_LINE));
    }

    #[test]
    fn invalid_utf8_drops_line_keeps_stream() {
        let mut d = LineDecoder::new();
        d.feed(b"\xff\xfe\n{}\n").unwrap();
        assert_eq!(d.next_frame(), Err(FrameError::InvalidUtf8));
        assert_eq!(d.next_frame().unwrap().as_deref(), Some("{}"));
    }

    #[test]
    fn empty_lines_are_skipped() {
        let mut d = LineDecoder::new();
        d.feed(&vec![b'\n'; MAX_LINE]).unwrap();
        assert_eq!(d.next_frame().unwrap(), None);
    }

    #[test]
    fn huge_single_feed_is_rejected() {
        let mut d = LineDecoder::new();
        assert_eq!(
            d.feed(&vec![b'\n'; MAX_BUFFERED + 1]),
            Err(FrameError::Oversize)
        );
    }

    #[test]
    fn byte_at_a_time_is_linear_and_correct() {
        let mut d = LineDecoder::new();
        for _ in 0..MAX_LINE {
            d.feed(b"x").unwrap();
            assert_eq!(d.next_frame().unwrap(), None);
        }
        d.feed(b"\n").unwrap();
        assert_eq!(d.next_frame().unwrap().map(|s| s.len()), Some(MAX_LINE));
    }
}
