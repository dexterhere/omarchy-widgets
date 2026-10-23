#![no_main]
use libfuzzer_sys::fuzz_target;
use ow_proto::{LineDecoder, MAX_LINE, Op, Request, unlock_request};

// Admin-op parsing, the one path that carries a passphrase. For any bytes: never panic, never
// show the passphrase in Debug output or in an error, and a valid unlock decodes to exactly the
// bytes that were encoded.
fuzz_target!(|data: &[u8]| {
    // Raw bytes through the decoder into the parser, as the daemon does.
    let mut d = LineDecoder::new();
    for chunk in data.chunks(MAX_LINE + 1) {
        if d.feed(chunk).is_err() {
            break;
        }
        while let Ok(Some(line)) = d.next_frame() {
            if let Ok(r) = Request::parse(&line) {
                let shown = format!("{r:?}");
                if matches!(r.op, Op::VaultUnlock { .. }) {
                    assert!(shown.contains("redacted"));
                }
            }
        }
    }
    // Round trip: the encoder and the parser agree for any non-empty passphrase.
    if let Some(line) = unlock_request(1, data) {
        match Request::parse(line.trim_end()).map(|r| r.op) {
            Ok(Op::VaultUnlock { passphrase }) => assert_eq!(passphrase.expose(), data),
            _ => panic!("encoded unlock did not parse"),
        }
    }
    // Hostile shapes around the admin names.
    if let Ok(s) = std::str::from_utf8(data) {
        for op in ["vault.unlock", "vault.lock", "vault.status"] {
            let _ = Request::parse(&format!(r#"{{"v":0,"id":1,"op":"{op}","passphrase":"{s}"}}"#));
            let _ = Request::parse(&format!(r#"{{"v":0,"id":1,"op":"{op}","widget":"{s}"}}"#));
        }
    }
});
