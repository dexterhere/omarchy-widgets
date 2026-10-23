//! `battery`: `battery.status {percent, state, timeLeft}` from /sys/class/power_supply, read only.
//!
//! Uses the first battery by name order (`BAT0` before `BAT1`) whose `type` is `Battery` (or has
//! no `type` file). No battery at all (desktop PC) is `Unavailable`; a battery whose files are
//! unusable is `Format`. State is the kernel string when it is one of Charging, Discharging,
//! Full, Not charging, else "Unknown". `timeLeft` is "3h 20m" or "45m" while charging or
//! discharging and a rate is known, otherwise "" (never a made-up value).
use ow_core::{FetchCtx, Provider, ProviderError, Sensitivity, TopicSpec};
use serde_json::{Map, Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

static TOPICS: [TopicSpec; 1] = [TopicSpec::new(
    "battery.status",
    Sensitivity::Low,
    Duration::from_secs(10),
    &["percent", "state", "timeLeft"],
)];

pub struct Battery {
    sys_root: PathBuf,
}

impl Battery {
    pub fn new() -> Self {
        Self::with_root("/sys".into())
    }

    /// Read from another directory laid out like /sys (tests).
    pub fn with_root(sys_root: PathBuf) -> Self {
        Self { sys_root }
    }
}

impl Default for Battery {
    fn default() -> Self {
        Self::new()
    }
}

fn int(dir: &Path, file: &str) -> Option<i64> {
    fs::read_to_string(dir.join(file)).ok()?.trim().parse().ok()
}

fn num(dir: &Path, file: &str) -> Option<u64> {
    int(dir, file)?.try_into().ok()
}

fn first_battery(root: &Path) -> Option<PathBuf> {
    let mut v: Vec<_> = fs::read_dir(root.join("class/power_supply"))
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("BAT"))
        .map(|e| e.path())
        .filter(|p| fs::read_to_string(p.join("type")).map_or(true, |t| t.trim() == "Battery"))
        .collect();
    v.sort();
    v.into_iter().next()
}

/// (now, full, rate) in energy (uWh, uW) or charge (uAh, uA); same units within a triple.
/// Some drivers report the rate as negative while discharging, hence `unsigned_abs`.
fn gauge(d: &Path) -> Option<(u64, u64, u64)> {
    for (n, f, r) in [
        ("energy_now", "energy_full", "power_now"),
        ("charge_now", "charge_full", "current_now"),
    ] {
        if let (Some(now), Some(full)) = (num(d, n), num(d, f)) {
            return Some((now, full, int(d, r).map_or(0, i64::unsigned_abs)));
        }
    }
    None
}

fn percent(d: &Path) -> Option<u64> {
    if let Some(c) = num(d, "capacity").filter(|c| *c <= 100) {
        return Some(c);
    }
    let (now, full, _) = gauge(d)?;
    (full > 0).then(|| ((now as f64 / full as f64 * 100.0).round() as u64).min(100))
}

fn state(d: &Path) -> &'static str {
    let s = fs::read_to_string(d.join("status")).unwrap_or_default();
    ["Charging", "Discharging", "Full", "Not charging"]
        .into_iter()
        .find(|k| *k == s.trim())
        .unwrap_or("Unknown")
}

fn time_left(d: &Path, state: &str) -> String {
    let Some((now, full, rate)) = gauge(d) else {
        return String::new();
    };
    let remaining = match state {
        "Discharging" => now,
        "Charging" => full.saturating_sub(now),
        _ => return String::new(),
    };
    if rate == 0 {
        return String::new(); // no division by zero, no guess
    }
    let mins = u128::from(remaining) * 60 / u128::from(rate);
    // Beyond 99 h the rate is noise (a just-plugged-in charger reading), say nothing.
    if mins == 0 || mins >= 99 * 60 {
        return String::new();
    }
    match (mins / 60, mins % 60) {
        (0, m) => format!("{m}m"),
        (h, m) => format!("{h}h {m}m"),
    }
}

impl Provider for Battery {
    fn id(&self) -> &'static str {
        "battery"
    }

    fn topics(&self) -> &'static [TopicSpec] {
        &TOPICS
    }

    fn fetch(
        &self,
        topic: &str,
        _params: &Map<String, Value>,
        _ctx: &FetchCtx<'_>,
    ) -> Result<Map<String, Value>, ProviderError> {
        let spec = TOPICS
            .iter()
            .find(|t| t.name == topic)
            .ok_or(ProviderError::UnknownTopic)?;
        let d = first_battery(&self.sys_root).ok_or(ProviderError::Unavailable)?;
        let percent = percent(&d).ok_or(ProviderError::Format)?;
        let state = state(&d);
        let data = json!({ "percent": percent, "state": state, "timeLeft": time_left(&d, state) });
        Ok(spec.sanitize(data.as_object().cloned().unwrap_or_default()))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn bat(files: &[(&str, &str)]) -> (tempfile::TempDir, Battery) {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "BAT0", files);
        let b = Battery::with_root(d.path().to_owned());
        (d, b)
    }

    fn put(root: &Path, name: &str, files: &[(&str, &str)]) {
        let dir = root.join("class/power_supply").join(name);
        fs::create_dir_all(&dir).unwrap();
        for (n, c) in files {
            fs::write(dir.join(n), c).unwrap();
        }
    }

    fn get(b: &Battery) -> Result<Map<String, Value>, ProviderError> {
        b.fetch("battery.status", &Map::new(), &FetchCtx::none())
    }

    #[test]
    fn discharging_with_capacity_and_energy() {
        let (_d, b) = bat(&[
            ("type", "Battery\n"),
            ("status", "Discharging\n"),
            ("capacity", "72\n"),
            ("energy_now", "36000000\n"),
            ("energy_full", "50000000\n"),
            ("power_now", "10800000\n"),
        ]);
        let m = get(&b).unwrap();
        assert_eq!(m["percent"], 72);
        assert_eq!(m["state"], "Discharging");
        assert_eq!(m["timeLeft"], "3h 20m");
    }

    #[test]
    fn charging_uses_remaining_to_full_and_charge_units() {
        let (_d, b) = bat(&[
            ("status", "Charging\n"),
            ("capacity", "50"),
            ("charge_now", "2000000"),
            ("charge_full", "4000000"),
            ("current_now", "-2000000"),
        ]);
        assert_eq!(get(&b).unwrap()["timeLeft"], "1h 0m");
    }

    #[test]
    fn no_battery_is_unavailable() {
        let d = tempfile::tempdir().unwrap();
        let b = Battery::with_root(d.path().to_owned());
        assert_eq!(get(&b), Err(ProviderError::Unavailable));
        // AC adapter only
        put(d.path(), "AC", &[("type", "Mains\n"), ("online", "1\n")]);
        assert_eq!(get(&b), Err(ProviderError::Unavailable));
        // a BAT-named supply that is not a battery
        put(d.path(), "BAT9", &[("type", "UPS\n"), ("capacity", "5")]);
        assert_eq!(get(&b), Err(ProviderError::Unavailable));
    }

    #[test]
    fn first_of_several_batteries_wins() {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "BAT1", &[("capacity", "11")]);
        put(d.path(), "BAT0", &[("capacity", "22")]);
        let b = Battery::with_root(d.path().to_owned());
        assert_eq!(get(&b).unwrap()["percent"], 22);
    }

    #[test]
    fn zero_power_means_empty_time_not_a_panic() {
        let (_d, b) = bat(&[
            ("status", "Discharging"),
            ("capacity", "40"),
            ("energy_now", "1000"),
            ("energy_full", "2000"),
            ("power_now", "0"),
        ]);
        assert_eq!(get(&b).unwrap()["timeLeft"], "");
    }

    #[test]
    fn energy_fallback_when_capacity_missing_or_garbage() {
        for cap in [None, Some("abc"), Some("250"), Some("-3")] {
            let mut f = vec![
                ("status", "Full"),
                ("energy_now", "49000000"),
                ("energy_full", "50000000"),
            ];
            if let Some(c) = cap {
                f.push(("capacity", c));
            }
            let (_d, b) = bat(&f);
            let m = get(&b).unwrap();
            assert_eq!(m["percent"], 98, "{cap:?}");
            assert_eq!(m["timeLeft"], "");
        }
    }

    #[test]
    fn unusable_files_are_format_errors() {
        let (_d, b) = bat(&[("status", "Discharging"), ("capacity", "lots")]);
        assert_eq!(get(&b), Err(ProviderError::Format));
        let (_d, b) = bat(&[("energy_now", "5"), ("energy_full", "0")]);
        assert_eq!(get(&b), Err(ProviderError::Format));
        let (_d, b) = bat(&[]);
        assert_eq!(get(&b), Err(ProviderError::Format));
    }

    #[test]
    fn unknown_status_and_missing_status() {
        let (_d, b) = bat(&[("capacity", "10"), ("status", "<script>\n")]);
        assert_eq!(get(&b).unwrap()["state"], "Unknown");
        let (_d, b) = bat(&[("capacity", "10")]);
        assert_eq!(get(&b).unwrap()["state"], "Unknown");
        let (_d, b) = bat(&[("capacity", "100"), ("status", "Not charging\n")]);
        assert_eq!(get(&b).unwrap()["state"], "Not charging");
    }

    #[test]
    fn absurd_times_and_huge_numbers_are_blank() {
        let (_d, b) = bat(&[
            ("status", "Discharging"),
            ("capacity", "40"),
            ("energy_now", "9223372036854775807"),
            ("energy_full", "9223372036854775807"),
            ("power_now", "1"),
        ]);
        assert_eq!(get(&b).unwrap()["timeLeft"], "");
        let (_d, b) = bat(&[
            ("status", "Discharging"),
            ("capacity", "40"),
            ("energy_now", "30000000"),
            ("energy_full", "50000000"),
            ("power_now", "40000000"),
        ]);
        assert_eq!(get(&b).unwrap()["timeLeft"], "45m");
    }

    #[test]
    fn unknown_topic() {
        let (_d, b) = bat(&[("capacity", "1")]);
        assert_eq!(
            b.fetch("battery.x", &Map::new(), &FetchCtx::none()),
            Err(ProviderError::UnknownTopic)
        );
    }
}
