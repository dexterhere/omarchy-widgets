//! `system`: CPU, memory and load from /proc, temperature from /sys, local time. No secrets, no
//! network.
use crate::temp;
use ow_core::{FetchCtx, Provider, ProviderError, Sensitivity, TopicSpec};
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

static TOPICS: [TopicSpec; 4] = [
    TopicSpec::new(
        "system.cpu",
        Sensitivity::Low,
        Duration::from_secs(2),
        &["usage_pct"],
    ),
    TopicSpec::new(
        "system.memory",
        Sensitivity::Low,
        Duration::from_secs(5),
        &["total_mib", "used_mib", "used_pct"],
    ),
    TopicSpec::new(
        "system.load",
        Sensitivity::Low,
        Duration::from_secs(5),
        &["load1", "load5", "load15"],
    ),
    TopicSpec::new(
        "system.temp",
        Sensitivity::Low,
        Duration::from_secs(5),
        &["celsius"],
    ),
];

#[derive(Clone, Copy)]
struct CpuSample {
    idle: u64,
    iowait: u64,
    total: u64,
    pct: Option<f64>,
}

pub struct System {
    proc_root: PathBuf,
    sys_root: PathBuf,
    last_cpu: Mutex<Option<CpuSample>>,
}

impl System {
    pub fn new() -> Self {
        Self::with_roots("/proc".into(), "/sys".into())
    }

    /// Read from another directory laid out like /proc (tests); /sys stays the real one.
    pub fn with_root(proc_root: PathBuf) -> Self {
        Self::with_roots(proc_root, "/sys".into())
    }

    /// Both roots injectable (tests): /proc-like and /sys-like.
    pub fn with_roots(proc_root: PathBuf, sys_root: PathBuf) -> Self {
        let s = Self {
            proc_root,
            sys_root,
            last_cpu: Mutex::new(None),
        };
        // Baseline now, so the first real fetch already has a delta to work with.
        let _ = s.cpu_usage();
        s
    }

    fn read(&self, file: &str) -> Result<String, ProviderError> {
        std::fs::read_to_string(self.proc_root.join(file)).map_err(|_| ProviderError::Unavailable)
    }
}

impl System {
    /// Busy percentage since the previous sample. A counter reset or regression re-baselines
    /// and reports `Unavailable` instead of a made-up number; if no tick elapsed since the
    /// last sample the last computed value is returned.
    fn cpu_usage(&self) -> Result<f64, ProviderError> {
        let (idle, iowait, total) = parse_cpu(&self.read("stat")?).ok_or(ProviderError::Format)?;
        let mut last = self
            .last_cpu
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let prev = *last;
        let fresh = CpuSample {
            idle,
            iowait,
            total,
            pct: None,
        };
        let Some(p) = prev else {
            *last = Some(fresh);
            return Err(ProviderError::Unavailable);
        };
        // `idle` and `total` are monotonic; a drop means a reset. `iowait` is documented as
        // unreliable and can go backwards, so a drop there counts as zero idle time gained.
        if total < p.total || idle < p.idle {
            *last = Some(fresh);
            return Err(ProviderError::Unavailable);
        }
        if total == p.total {
            return p.pct.ok_or(ProviderError::Unavailable);
        }
        let idle_gain = (idle - p.idle) + iowait.saturating_sub(p.iowait);
        let busy = (100.0 * (1.0 - idle_gain as f64 / (total - p.total) as f64)).clamp(0.0, 100.0);
        let pct = round1(busy);
        *last = Some(CpuSample {
            idle,
            iowait,
            total,
            pct: Some(pct),
        });
        Ok(pct)
    }
}

impl Default for System {
    fn default() -> Self {
        Self::new()
    }
}

/// `cpu  user nice system idle iowait irq softirq steal ...` -> (idle, iowait, total).
pub(crate) fn parse_cpu(stat: &str) -> Option<(u64, u64, u64)> {
    let line = stat.lines().find(|l| l.starts_with("cpu "))?;
    let v: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    if v.len() < 5 {
        return None;
    }
    // guest/guest_nice (indices 8, 9) are already counted inside user/nice.
    let total = v.iter().take(8).try_fold(0u64, |a, &x| a.checked_add(x))?;
    Some((v[3], v[4], total))
}

/// -> (total_kib, available_kib)
pub(crate) fn parse_meminfo(s: &str) -> Option<(u64, u64)> {
    let get = |key: &str| -> Option<u64> {
        s.lines().find_map(|l| {
            l.strip_prefix(key)?
                .trim()
                .strip_suffix("kB")?
                .trim()
                .parse()
                .ok()
        })
    };
    Some((get("MemTotal:")?, get("MemAvailable:")?))
}

pub(crate) fn parse_load(s: &str) -> Option<[f64; 3]> {
    let mut it = s.split_whitespace().take(3).map(str::parse::<f64>);
    let out = [it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?];
    out.iter()
        .all(|x| x.is_finite() && *x >= 0.0)
        .then_some(out)
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

impl Provider for System {
    fn id(&self) -> &'static str {
        "system"
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
        let data = match topic {
            "system.cpu" => json!({ "usage_pct": self.cpu_usage()? }),
            "system.memory" => {
                let (total, avail) =
                    parse_meminfo(&self.read("meminfo")?).ok_or(ProviderError::Format)?;
                if total == 0 || avail > total {
                    return Err(ProviderError::Format);
                }
                let used = total - avail;
                json!({
                    "total_mib": total / 1024,
                    "used_mib": used / 1024,
                    "used_pct": round1(100.0 * used as f64 / total as f64),
                })
            }
            "system.load" => {
                let [a, b, c] = parse_load(&self.read("loadavg")?).ok_or(ProviderError::Format)?;
                json!({ "load1": a, "load5": b, "load15": c })
            }
            "system.temp" => json!({ "celsius": temp::celsius(&self.sys_root)? }),
            _ => return Err(ProviderError::UnknownTopic),
        };
        Ok(spec.sanitize(obj(data)))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    const STAT1: &str = "cpu  100 0 100 800 0 0 0 0 0 0\ncpu0 1 2 3 4 5 6 7 8 9 10\nintr 1\n";
    const STAT2: &str = "cpu  150 0 150 900 0 0 0 0 0 0\n";
    const MEM: &str = "MemTotal:       16384000 kB\nMemFree: 1 kB\nMemAvailable:    4096000 kB\n";

    fn fixture(files: &[(&str, &str)]) -> (tempfile::TempDir, System) {
        let d = tempfile::tempdir().unwrap();
        for (n, c) in files {
            std::fs::write(d.path().join(n), c).unwrap();
        }
        let s = System::with_root(d.path().to_owned());
        (d, s)
    }

    #[test]
    fn parsers() {
        assert_eq!(parse_cpu(STAT1), Some((800, 0, 1000)));
        assert_eq!(parse_meminfo(MEM), Some((16_384_000, 4_096_000)));
        assert_eq!(
            parse_load("0.50 1.25 2.00 1/100 123\n"),
            Some([0.5, 1.25, 2.0])
        );
        for bad in ["", "cpu 1 2", "garbage"] {
            assert_eq!(parse_cpu(bad), None);
        }
        assert_eq!(parse_meminfo("MemTotal: abc kB"), None);
        assert_eq!(parse_load("x y z"), None);
    }

    #[test]
    fn cpu_usage_is_delta_based_and_honest() {
        let (d, s) = fixture(&[("stat", STAT1)]); // baseline taken at construction
        let p = Map::new();
        let set = |c: &str| std::fs::write(d.path().join("stat"), c).unwrap();
        // no tick since the baseline and no previous value: unavailable, not a fake 0
        assert_eq!(
            s.fetch("system.cpu", &p, &FetchCtx::none()),
            Err(ProviderError::Unavailable)
        );
        set(STAT2); // total +200, idle +100
        assert_eq!(
            s.fetch("system.cpu", &p, &FetchCtx::none()).unwrap()["usage_pct"],
            50.0
        );
        // unchanged counters: last value, no division by zero
        assert_eq!(
            s.fetch("system.cpu", &p, &FetchCtx::none()).unwrap()["usage_pct"],
            50.0
        );
        // idle regresses while total advances: re-baseline, never 100%
        set("cpu  150 0 150 800 0 0 0 0 0 0\n");
        assert_eq!(
            s.fetch("system.cpu", &p, &FetchCtx::none()),
            Err(ProviderError::Unavailable)
        );
        // total regresses (reset): same
        set("cpu  1 0 1 8 0 0 0 0 0 0\n");
        assert_eq!(
            s.fetch("system.cpu", &p, &FetchCtx::none()),
            Err(ProviderError::Unavailable)
        );
    }

    #[test]
    fn iowait_going_backwards_is_not_a_reset() {
        let (d, s) = fixture(&[("stat", "cpu  100 0 100 800 50 0 0 0 0 0\n")]);
        let set = |c: &str| std::fs::write(d.path().join("stat"), c).unwrap();
        // idle +100, iowait -10 (counted as 0), total +190 -> 1 - 100/190
        set("cpu  150 0 150 900 40 0 0 0 0 0\n");
        let v = s
            .fetch("system.cpu", &Map::new(), &FetchCtx::none())
            .unwrap()["usage_pct"]
            .as_f64()
            .unwrap();
        assert!((v - 47.4).abs() < 0.1, "{v}");
    }

    #[test]
    fn guest_columns_are_not_double_counted() {
        assert_eq!(
            parse_cpu("cpu  100 0 100 800 0 0 0 0 50 50\n"),
            Some((800, 0, 1000))
        );
    }

    #[test]
    fn hostile_numbers_are_rejected() {
        let max = u64::MAX;
        assert_eq!(
            parse_cpu(&format!("cpu  {max} {max} 0 0 0 0 0 0 0 0\n")),
            None
        );
        assert_eq!(
            parse_cpu(&format!("cpu  0 0 0 {max} {max} 0 0 0 0 0\n")),
            None
        );
        for bad in ["nan 0 0", "inf 0 0", "1e400 0 0", "-1 0 0"] {
            assert_eq!(parse_load(bad), None, "{bad}");
        }
    }

    #[test]
    fn memory_and_load() {
        let (_d, s) = fixture(&[("meminfo", MEM), ("loadavg", "0.10 0.20 0.30 1/2 3\n")]);
        let m = s
            .fetch("system.memory", &Map::new(), &FetchCtx::none())
            .unwrap();
        assert_eq!(m["total_mib"], 16_000);
        assert_eq!(m["used_pct"], 75.0);
        let l = s
            .fetch("system.load", &Map::new(), &FetchCtx::none())
            .unwrap();
        assert_eq!(l["load5"], 0.2);
    }

    #[test]
    fn memory_guards() {
        for body in [
            "MemTotal: 0 kB\nMemAvailable: 0 kB\n",
            "MemTotal: 1 kB\nMemAvailable: 2 kB\n",
            "MemTotal: 1 kB\n",
        ] {
            let (_d, s) = fixture(&[("meminfo", body)]);
            assert_eq!(
                s.fetch("system.memory", &Map::new(), &FetchCtx::none()),
                Err(ProviderError::Format),
                "{body}"
            );
        }
    }

    #[test]
    fn errors_are_categories_only() {
        let (_d, s) = fixture(&[("meminfo", "MemTotal: 1 kB\nMemAvailable: 2 kB\n")]);
        assert_eq!(
            s.fetch("system.memory", &Map::new(), &FetchCtx::none()),
            Err(ProviderError::Format)
        );
        assert_eq!(
            s.fetch("system.load", &Map::new(), &FetchCtx::none()),
            Err(ProviderError::Unavailable)
        );
        assert_eq!(
            s.fetch("nope", &Map::new(), &FetchCtx::none()),
            Err(ProviderError::UnknownTopic)
        );
    }

    #[test]
    fn real_proc_works_on_this_machine() {
        let s = System::new();
        let p = Map::new();
        let pct = |v: &Map<String, Value>, k: &str| v[k].as_f64().unwrap();
        let m = s.fetch("system.memory", &p, &FetchCtx::none()).unwrap();
        assert!((0.0..=100.0).contains(&pct(&m, "used_pct")));
        assert!(pct(&m, "total_mib") > 0.0);
        assert!(
            !s.fetch("system.load", &p, &FetchCtx::none())
                .unwrap()
                .is_empty()
        );
        std::thread::sleep(Duration::from_millis(60));
        if let Ok(c) = s.fetch("system.cpu", &p, &FetchCtx::none()) {
            assert!((0.0..=100.0).contains(&pct(&c, "usage_pct")));
        }
    }

    #[test]
    fn undeclared_fields_are_dropped() {
        let spec = &TOPICS[0];
        let out = spec.sanitize(
            json!({"usage_pct": 1, "secret": "x"})
                .as_object()
                .cloned()
                .unwrap(),
        );
        assert_eq!(out.len(), 1);
    }
}
