//! The real hub serves `system.time` (real System provider) only to a granted widget and field.
#![allow(clippy::unwrap_used, clippy::expect_used)] // test code
use ow_core::Paths;
use ow_providers::{Battery, System};
use owd::Server;
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const GRANTS: &str = "[[grant]]\nwidget=\"clock\"\ntopics=[\"system.time\"]\nfields=[\"hm\",\"hms\",\"date\",\"tz\"]\n\n[[grant]]\nwidget=\"partial\"\ntopics=[\"system.time\"]\nfields=[\"hm\"]\n";

fn ask(paths: &Paths, line: &str) -> String {
    let mut s = UnixStream::connect(paths.socket()).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    s.write_all(line.as_bytes()).unwrap();
    let mut out = String::new();
    BufReader::new(s).read_line(&mut out).unwrap();
    out
}

fn snap(paths: &Paths, widget: &str, topic: &str) -> String {
    ask(
        paths,
        &format!(
            "{{\"v\":0,\"id\":1,\"op\":\"snapshot\",\"widget\":\"{widget}\",\"topic\":\"{topic}\"}}\n"
        ),
    )
}

#[test]
fn system_time_reaches_granted_widget_only() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path());
    std::fs::create_dir_all(&paths.config).unwrap();
    std::fs::write(paths.grants(), GRANTS).unwrap();
    std::fs::set_permissions(paths.grants(), std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = Server::bind(
        &paths,
        vec![Arc::new(System::new()), Arc::new(Battery::new())],
    )
    .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (s2, r) = (Arc::clone(&stop), Arc::new(AtomicBool::new(false)));
    let th = std::thread::spawn(move || server.run(&s2, &r));

    // granted: all four fields, with the widget's shapes
    let v: Value = serde_json::from_str(&snap(&paths, "clock", "system.time")).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    let data = &v["data"];
    assert_eq!(data.as_object().unwrap().len(), 4, "{v}");
    assert_eq!(data["hm"].as_str().unwrap().len(), 5);
    assert_eq!(data["hms"].as_str().unwrap().len(), 8);
    assert!(data["date"].is_string() && data["tz"].is_string());

    // field-level grant: only hm
    let v: Value = serde_json::from_str(&snap(&paths, "partial", "system.time")).unwrap();
    assert_eq!(v["data"].as_object().unwrap().len(), 1, "{v}");

    // denied client and ungranted topic get nothing, not even the field names
    for (w, t) in [("stranger", "system.time"), ("clock", "battery.status")] {
        let r = snap(&paths, w, t);
        assert!(r.contains("denied"), "{r}");
        assert!(!r.contains("\"hm\"") && !r.contains("percent"), "{r}");
    }
    stop.store(true, Ordering::Relaxed);
    th.join().unwrap();
}
