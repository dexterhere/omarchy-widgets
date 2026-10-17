#![no_main]
use libfuzzer_sys::fuzz_target;

// Hostile vault files: bounded work, no panic. Random bytes must essentially never open.
fuzz_target!(|data: &[u8]| {
    let _ = ow_vault::fuzz_open(data);
});
