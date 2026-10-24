#![no_main]
use libfuzzer_sys::fuzz_target;
use ow::connect::{edit_config, remove_binding};
use ow_core::Config;

// The text `ow connect github` writes must always be accepted by the daemon's own loader and
// bind exactly what was asked, whatever the old file held; removal must leave no binding.
fuzz_target!(|data: &[u8]| {
    let Some((&b, rest)) = data.split_first() else {
        return;
    };
    let Ok(old) = std::str::from_utf8(rest) else {
        return;
    };
    let secs = u64::from(b) * 20;
    let new = edit_config(Some(old), "github-fuzz", secs.clamp(60, 3600));
    let c = Config::parse(&new).expect("edited config must load");
    let g = c.github().expect("binding present");
    assert_eq!(g.secret(), "github-fuzz");
    assert_eq!(g.interval().as_secs(), secs.clamp(60, 3600));
    let gone = remove_binding(&new);
    assert!(
        Config::parse(&gone)
            .expect("removal must load")
            .github()
            .is_none()
    );
});
