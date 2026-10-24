#![no_main]
use libfuzzer_sys::fuzz_target;
use ow_core::{Config, MAX_INTERVAL_SECS, MIN_INTERVAL_SECS, secret_name_allowed};

// Whatever the bytes, parsing returns Ok or an error, never panics, and an accepted binding
// always satisfies the rules the daemon relies on.
fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data)
        && let Ok(c) = Config::parse(s)
        && let Some(g) = c.github()
    {
        assert!(secret_name_allowed("github", g.secret()));
        let secs = g.interval().as_secs();
        assert!((MIN_INTERVAL_SECS..=MAX_INTERVAL_SECS).contains(&secs));
    }
});
