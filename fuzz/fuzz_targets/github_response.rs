#![no_main]
use libfuzzer_sys::fuzz_target;

// An upstream body is parsed into two bounded counts or refused with a category; it never
// panics and nothing but those two numbers comes out.
fuzz_target!(|data: &[u8]| {
    if let Ok((open, review)) = ow_providers::parse_github_counts(data) {
        assert!(open <= 1_000_000 && review <= 1_000_000);
    }
});
