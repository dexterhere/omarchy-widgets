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
                if let Op::VaultUnlock { passphrase } = &r.op {
                    assert!(shown.contains("redacted"));
                    let p = passphrase.expose();
                    // Debug output never contains the passphrase bytes (a one-byte passphrase
                    // could appear by chance inside the fixed text, so check longer ones).
                    if p.len() >= 4 {
                        assert!(!shown.as_bytes().windows(p.len()).any(|w| w == p));
                    }
                }
            }
            // Any admin op frame that has a `widget` key, whatever its value, must be refused.
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                let admin = v["op"].as_str().is_some_and(|o| o.starts_with("vault."));
                let named = ["vault.unlock", "vault.lock", "vault.status"]
                    .contains(&v["op"].as_str().unwrap_or(""));
                if admin && named && v.get("widget").is_some() && v["v"] == 0 {
                    assert!(Request::parse(&line).is_err());
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
            for w in [format!("\"{s}\""), "null".into(), s.to_owned()] {
                let line = format!(r#"{{"v":0,"id":1,"op":"{op}","widget":{w},"passphrase":"ab"}}"#);
                // Valid JSON or not, a parse that succeeds must never be an admin op with a widget key.
                if let Ok(r) = Request::parse(&line) {
                    assert!(r.op.admin_name().is_none(), "{line}");
                }
            }
        }
    }
});
