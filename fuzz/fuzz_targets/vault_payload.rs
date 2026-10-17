#![no_main]
use libfuzzer_sys::fuzz_target;

// The decrypted-payload parser (canonical JSON, name/size rules) behind the AEAD tag.
fuzz_target!(|data: &[u8]| {
    let _ = ow_vault::fuzz_payload(data);
});
