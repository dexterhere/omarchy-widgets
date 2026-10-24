#![no_main]
use libfuzzer_sys::fuzz_target;
use ow::install::{catalog, grants_edit, index, json, validate};

// Everything `ow install` parses from the network, for any bytes: never panic, never accept a
// duplicate key, and an edited grants file always parses back to exactly the intended entries.
fuzz_target!(|data: &[u8]| {
    let _ = json::strict(data, json::MAX_DEPTH);
    if let Ok(v) = json::strict(data, json::MAX_DEPTH) {
        let _ = validate::sample_data(&v);
    }
    if let Ok(t) = std::str::from_utf8(data) {
        // The allow-list is never looser than the spoofing floor.
        if ow::install::text::printable(t) {
            assert!(ow::install::text::plain(t));
        }
    }
    let id = "io.github.dexterhere.clock";
    let _ = index::select_listing(data, id);
    let cat = catalog::snapshot().expect("shipped catalog");
    // A fixed, valid listing; the fuzzed bytes play each of the three files in turn.
    let idx = br#"{"schemaVersion":1,"widgets":[{"id":"io.github.dexterhere.clock","repo":"https://github.com/dexterhere/c","commit":"0123456789abcdef0123456789abcdef01234567","status":"alpha","tier":"spec"}]}"#;
    let l = index::select_listing(idx, id).expect("fixed listing");
    let ok_manifest = br#"{"id":"io.github.dexterhere.clock","name":"Clock","version":"0.1.0","description":"d","license":"MIT"}"#;
    let _ = validate::check_files(&l, &cat, data, data, data);
    let _ = validate::check_files(&l, &cat, ok_manifest, data, data);
    // Grants editing: any text that is not valid grants is refused; valid text round-trips.
    if let Ok(s) = std::str::from_utf8(data) {
        let entry = grants_edit::Entry { topics: vec!["system.time".into()], fields: vec!["hm".into()] };
        if let Ok(t) = grants_edit::edit(s, id, &[entry], "2026-10-23T00:00:00Z") {
            let g = ow_core::Grants::parse(&t).expect("edit output must parse");
            assert!(g.find(id, "system.time").is_some());
            let back = grants_edit::edit(&t, id, &[], "x").expect("removal");
            assert!(ow_core::Grants::parse(&back).expect("parse").find(id, "system.time").is_none());
        }
    }
});
