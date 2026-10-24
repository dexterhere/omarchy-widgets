//! `system.temp`: hottest relevant sensor from sysfs, read only.
//!
//! Choice: if any hwmon device is a known CPU sensor driver (`coretemp`, `k10temp`, `zenpower`,
//! `cpu_thermal`, `x86_pkg_temp`), report the max over its `temp*_input` files (package and core
//! sensors). Otherwise the max over every hwmon `temp*_input`, then over
//! `/sys/class/thermal/thermal_zone*/temp`. Readings outside -50..=150 C are ignored.
use crate::attr::{SYSFS_MAX, read};
use ow_core::ProviderError;
use std::fs;
use std::path::Path;

const CPU_DRIVERS: [&str; 5] = [
    "coretemp",
    "k10temp",
    "zenpower",
    "cpu_thermal",
    "x86_pkg_temp",
];

fn milli(p: &Path) -> Option<f64> {
    let m: i64 = read(p, SYSFS_MAX).ok()?.trim().parse().ok()?;
    let c = m as f64 / 1000.0;
    (-50.0..=150.0).contains(&c).then_some(c)
}

fn max(it: impl Iterator<Item = f64>) -> Option<f64> {
    it.fold(None, |a, x| Some(a.map_or(x, |a: f64| a.max(x))))
}

fn dirs(p: &Path, prefix: &str) -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = fs::read_dir(p)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

fn hwmon_max(dir: &Path) -> Option<f64> {
    max(fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n.starts_with("temp") && n.ends_with("_input")
        })
        .filter_map(|e| milli(&e.path())))
}

pub(crate) fn celsius(sys: &Path) -> Result<f64, ProviderError> {
    let hw = dirs(&sys.join("class/hwmon"), "hwmon");
    let is_cpu = |d: &&std::path::PathBuf| {
        read(&d.join("name"), SYSFS_MAX).is_ok_and(|n| CPU_DRIVERS.contains(&n.trim()))
    };
    let cpu = max(hw.iter().filter(is_cpu).filter_map(|d| hwmon_max(d)));
    let any = || max(hw.iter().filter_map(|d| hwmon_max(d)));
    let zones = || {
        max(dirs(&sys.join("class/thermal"), "thermal_zone")
            .iter()
            .filter_map(|z| milli(&z.join("temp"))))
    };
    cpu.or_else(any)
        .or_else(zones)
        .map(|c| (c * 10.0).round() / 10.0)
        .ok_or(ProviderError::Unavailable)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn put(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    #[test]
    fn cpu_driver_wins_over_hotter_other_sensor() {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "class/hwmon/hwmon0/name", "nvme\n");
        put(d.path(), "class/hwmon/hwmon0/temp1_input", "80000\n");
        put(d.path(), "class/hwmon/hwmon1/name", "coretemp\n");
        put(d.path(), "class/hwmon/hwmon1/temp1_input", "54000\n");
        put(d.path(), "class/hwmon/hwmon1/temp2_input", "61500\n");
        assert_eq!(celsius(d.path()), Ok(61.5));
    }

    #[test]
    fn falls_back_to_all_hwmon_then_thermal_zones() {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "class/thermal/thermal_zone0/temp", "47000\n");
        assert_eq!(celsius(d.path()), Ok(47.0));
        put(d.path(), "class/hwmon/hwmon0/name", "nvme\n");
        put(d.path(), "class/hwmon/hwmon0/temp1_input", "39000\n");
        assert_eq!(celsius(d.path()), Ok(39.0));
    }

    #[test]
    fn insane_garbage_and_missing_are_ignored_or_unavailable() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(celsius(d.path()), Err(ProviderError::Unavailable));
        put(d.path(), "class/hwmon/hwmon0/name", "coretemp\n");
        for bad in ["200000", "-60000", "abc", "", "99999999999999999999"] {
            put(d.path(), "class/hwmon/hwmon0/temp1_input", bad);
            assert_eq!(celsius(d.path()), Err(ProviderError::Unavailable), "{bad}");
        }
        // a sane sensor next to a broken one still reports
        put(d.path(), "class/hwmon/hwmon0/temp2_input", "50000");
        assert_eq!(celsius(d.path()), Ok(50.0));
        // insane CPU readings do not hide a sane non-CPU sensor
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "class/hwmon/hwmon0/name", "coretemp");
        put(d.path(), "class/hwmon/hwmon0/temp1_input", "999000");
        put(d.path(), "class/hwmon/hwmon1/temp1_input", "42000");
        assert_eq!(celsius(d.path()), Ok(42.0));
    }

    #[test]
    fn hostile_sensor_files_never_hang() {
        use crate::attr::tests::{hostile, within_2s};
        for f in ["name", "temp1_input"] {
            let d = tempfile::tempdir().unwrap();
            put(d.path(), "class/hwmon/hwmon0/name", "coretemp");
            put(d.path(), "class/hwmon/hwmon0/temp1_input", "50000");
            let root = d.path().to_owned();
            hostile(&root.join("class/hwmon/hwmon0").join(f), || {
                let r = root.clone();
                let _ = within_2s(move || celsius(&r));
            });
        }
    }
}
