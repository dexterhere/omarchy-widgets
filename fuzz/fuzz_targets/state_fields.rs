#![no_main]
use libfuzzer_sys::fuzz_target;
use ow_proto::{ErrorBody, StateInfo};
use serde_json::{Map, Value};

// A reply body from a hostile or newer daemon: the client-side state parser either yields a
// known state with an optional number or nothing, and what it yields survives a round trip.
fuzz_target!(|data: &[u8]| {
    if let Ok(Value::Object(m)) = serde_json::from_slice::<Value>(data) {
        if let Some(info) = StateInfo::from_fields(&m) {
            let mut out = Map::new();
            info.put(&mut out);
            assert_eq!(StateInfo::from_fields(&out), Some(info));
        }
        let _ = serde_json::from_value::<ErrorBody>(Value::Object(m));
    }
});
