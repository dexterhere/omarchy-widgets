#![no_main]
use libfuzzer_sys::fuzz_target;
use ow_proto::Request;

// Whatever the bytes, parsing returns Ok or a categorised error, never panics.
fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = Request::parse(s);
    }
});
