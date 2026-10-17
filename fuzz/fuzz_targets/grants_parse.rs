#![no_main]
use libfuzzer_sys::fuzz_target;
use ow_core::Grants;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(g) = Grants::parse(s) {
            // Whatever parsed must be queryable without panicking.
            let _ = g.find("w", "t");
        }
    }
});
