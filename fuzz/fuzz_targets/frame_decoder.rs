#![no_main]
use libfuzzer_sys::fuzz_target;
use ow_proto::{LineDecoder, MAX_LINE};

// Arbitrary chunking of arbitrary bytes: never panic, never yield a line over the limit.
fuzz_target!(|data: &[u8]| {
    let mut d = LineDecoder::new();
    for chunk in data.chunks(1 + data.first().copied().unwrap_or(0) as usize) {
        if d.feed(&chunk[..chunk.len().min(MAX_LINE + 1)]).is_err() {
            return;
        }
        while let Ok(Some(line)) = d.next_frame() {
            assert!(line.len() <= MAX_LINE);
        }
    }
});
