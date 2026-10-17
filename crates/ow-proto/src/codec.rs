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

/// Incremental newline splitter with a hard size bound.
#[derive(Default)]
pub struct LineDecoder {
    buf: Vec<u8>,
    overflow: bool,
}

impl LineDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append received bytes. Memory stays bounded: at most `MAX_LINE` bytes of a
    /// partial line are kept, plus whatever complete lines are in `bytes`.
    pub fn feed(&mut self, bytes: &[u8]) {
        if self.overflow {
            return;
        }
        self.buf.extend_from_slice(bytes);
    }

    /// Next complete line, `Ok(None)` if more bytes are needed.
    pub fn next_frame(&mut self) -> Result<Option<String>, FrameError> {
        if self.overflow {
            return Err(FrameError::Oversize);
        }
        let Some(pos) = self.buf.iter().position(|&b| b == b'\n') else {
            if self.buf.len() > MAX_LINE {
                self.fail();
                return Err(FrameError::Oversize);
            }
            return Ok(None);
        };
        if pos > MAX_LINE {
            self.fail();
            return Err(FrameError::Oversize);
        }
        let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
        line.pop(); // newline
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        String::from_utf8(line)
            .map(Some)
            .map_err(|_| FrameError::InvalidUtf8)
    }

    fn fail(&mut self) {
        self.overflow = true;
        self.buf = Vec::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_and_buffers_partial() {
        let mut d = LineDecoder::new();
        d.feed(b"{\"a\":1}\n{\"b\"");
        assert_eq!(d.next_frame().unwrap().as_deref(), Some("{\"a\":1}"));
        assert_eq!(d.next_frame().unwrap(), None);
        d.feed(b":2}\r\n");
        assert_eq!(d.next_frame().unwrap().as_deref(), Some("{\"b\":2}"));
    }

    #[test]
    fn oversize_without_newline_is_fatal() {
        let mut d = LineDecoder::new();
        d.feed(&vec![b'x'; MAX_LINE + 1]);
        assert_eq!(d.next_frame(), Err(FrameError::Oversize));
        d.feed(b"\n");
        assert_eq!(d.next_frame(), Err(FrameError::Oversize));
    }

    #[test]
    fn oversize_with_newline_is_fatal() {
        let mut d = LineDecoder::new();
        let mut v = vec![b'x'; MAX_LINE + 1];
        v.push(b'\n');
        d.feed(&v);
        assert_eq!(d.next_frame(), Err(FrameError::Oversize));
    }

    #[test]
    fn exactly_max_is_ok() {
        let mut d = LineDecoder::new();
        let mut v = vec![b'x'; MAX_LINE];
        v.push(b'\n');
        d.feed(&v);
        assert_eq!(d.next_frame().unwrap().map(|s| s.len()), Some(MAX_LINE));
    }

    #[test]
    fn invalid_utf8_drops_line_keeps_stream() {
        let mut d = LineDecoder::new();
        d.feed(b"\xff\xfe\n{}\n");
        assert_eq!(d.next_frame(), Err(FrameError::InvalidUtf8));
        assert_eq!(d.next_frame().unwrap().as_deref(), Some("{}"));
    }
}
