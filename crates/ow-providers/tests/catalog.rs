//! Topics, field names and JSON types must match the web catalog (registry/providers.json) and
//! the sampleData of the clock, battery and system-monitor widgets. Output must fit the 48 KiB cap.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use ow_core::{FetchCtx, Provider, ProviderError};
use ow_providers::{Battery, System};
use serde_json::{Map, Value, json};
use std::fs;
use std::path::Path;

const CAP: usize = 48 * 1024;

/// topic -> sampleData exactly as the widgets ship it (only the types are compared).
fn samples() -> Vec<(&'static str, Value)> {
    vec![
        (
            "system.time",
            json!({"hm": "10:09", "hms": "10:09:42", "date": "Mon 19 Oct", "tz": "NPT"}),
        ),
        (
            "battery.status",
            json!({"percent": 72, "state": "Discharging", "timeLeft": "3h 20m"}),
        ),
        ("system.temp", json!({"celsius": 54})),
    ]
}

fn put(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

fn fixture() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "class/hwmon/hwmon0/name", "coretemp\n");
    put(d.path(), "class/hwmon/hwmon0/temp1_input", "54000\n");
    let b = "class/power_supply/BAT0/";
    for (f, c) in [
        ("type", "Battery"),
        ("status", "Discharging"),
        ("capacity", "72"),
        ("energy_now", "36000000"),
        ("energy_full", "50000000"),
        ("power_now", "10800000"),
    ] {
        put(d.path(), &format!("{b}{f}"), c);
    }
    d
}

fn fetch_all(d: &Path) -> Vec<(&'static str, Map<String, Value>)> {
    let sys = System::with_roots("/proc".into(), d.to_owned());
    let bat = Battery::with_root(d.to_owned());
    let ctx = FetchCtx::none();
    let none = Map::new();
    vec![
        (
            "system.time",
            sys.fetch("system.time", &none, &ctx).unwrap(),
        ),
        (
            "system.temp",
            sys.fetch("system.temp", &none, &ctx).unwrap(),
        ),
        (
            "battery.status",
            bat.fetch("battery.status", &none, &ctx).unwrap(),
        ),
    ]
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::String(_) => "string",
        Value::Number(_) => "number",
        _ => "other",
    }
}

#[test]
fn fields_and_json_types_match_widget_sample_data() {
    let d = fixture();
    let got = fetch_all(d.path());
    for (topic, sample) in samples() {
        let (_, out) = got.iter().find(|(t, _)| *t == topic).unwrap();
        let sample = sample.as_object().unwrap();
        let mut a: Vec<_> = out.keys().collect();
        let mut b: Vec<_> = sample.keys().collect();
        a.sort();
        b.sort();
        assert_eq!(a, b, "{topic}: field names");
        for (k, v) in sample {
            assert_eq!(kind(&out[k]), kind(v), "{topic}.{k}");
        }
    }
}

#[test]
fn declared_fields_equal_the_catalog() {
    let want: [(&str, &[&str]); 3] = [
        ("system.time", &["hm", "hms", "date", "tz"]),
        ("battery.status", &["percent", "state", "timeLeft"]),
        ("system.temp", &["celsius"]),
    ];
    let providers: [Box<dyn Provider>; 2] = [Box::new(System::new()), Box::new(Battery::new())];
    for (topic, fields) in want {
        let spec = providers
            .iter()
            .flat_map(|p| p.topics())
            .find(|t| t.name == topic)
            .unwrap_or_else(|| panic!("{topic} not served"));
        assert_eq!(spec.fields, fields, "{topic}");
        assert!(spec.params.is_empty(), "{topic} takes no parameters");
        assert_eq!(spec.sensitivity, ow_core::Sensitivity::Low);
    }
}

#[test]
fn time_has_the_widget_shapes() {
    let d = fixture();
    let sys = System::with_roots("/proc".into(), d.path().to_owned());
    let t = sys
        .fetch("system.time", &Map::new(), &FetchCtx::none())
        .unwrap();
    let s = |k: &str| t[k].as_str().unwrap().to_owned();
    let b = |x: &str| x.as_bytes().to_vec();
    let (hm, hms, date) = (b(&s("hm")), b(&s("hms")), s("date"));
    assert!(hm.len() == 5 && hm[2] == b':', "{hm:?}");
    assert!(hms.len() == 8 && hms[2] == b':' && hms[5] == b':');
    assert!(hms.starts_with(&hm));
    assert_eq!(date.split(' ').count(), 3, "{date}");
    assert!(!s("tz").is_empty());
}

#[test]
fn no_battery_and_no_sensor_are_unavailable_not_panics() {
    let d = tempfile::tempdir().unwrap();
    let ctx = FetchCtx::none();
    assert_eq!(
        Battery::with_root(d.path().to_owned()).fetch("battery.status", &Map::new(), &ctx),
        Err(ProviderError::Unavailable)
    );
    let sys = System::with_roots("/proc".into(), d.path().to_owned());
    assert_eq!(
        sys.fetch("system.temp", &Map::new(), &ctx),
        Err(ProviderError::Unavailable)
    );
}

#[test]
fn params_are_rejected_and_output_fits_the_cap() {
    let d = fixture();
    for (topic, out) in fetch_all(d.path()) {
        let n = serde_json::to_string(&out).unwrap().len();
        assert!(n < CAP, "{topic}: {n} bytes");
    }
    let sys = System::new();
    let spec = sys
        .topics()
        .iter()
        .find(|t| t.name == "system.time")
        .unwrap();
    let mut p = Map::new();
    p.insert("x".into(), json!(1));
    assert_eq!(spec.check_params(&p), Err(ProviderError::BadParams));
    // hostile files cannot inflate output: a huge status string is not echoed
    let b = "class/power_supply/BAT0/status";
    put(d.path(), b, &"A".repeat(10 * CAP));
    let out = Battery::with_root(d.path().to_owned())
        .fetch("battery.status", &Map::new(), &FetchCtx::none())
        .unwrap();
    assert!(serde_json::to_string(&out).unwrap().len() < 200);
    assert_eq!(out["state"], "Unknown");
}
