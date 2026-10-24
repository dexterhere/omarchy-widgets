//! The provider catalog snapshot compiled into the binary (`data/providers.json`, a copy of the
//! website's `registry/providers.json`). A widget may use only providers, topics and fields that
//! exist here. A newer website catalog than this binary knows fails closed ("not in the catalog").
use super::json;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const SNAPSHOT: &str = include_str!("../../data/providers.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    None,
    Vault,
}

#[derive(Debug, Clone)]
pub struct Provider {
    pub class: String,
    pub auth: Auth,
    /// Hosts the daemon (never the widget) may contact for this provider.
    pub egress: Vec<String>,
    /// topic -> fields it offers.
    pub topics: BTreeMap<String, Vec<String>>,
    /// `topic.field` pairs the shipped daemon serves today.
    pub serving: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct Catalog {
    pub providers: BTreeMap<String, Provider>,
}

fn strings(v: &Value, at: &str) -> Result<Vec<String>, String> {
    v.as_array()
        .ok_or_else(|| format!("catalog: {at} must be an array"))?
        .iter()
        .map(|x| {
            x.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("catalog: {at} must hold strings"))
        })
        .collect()
}

pub fn parse(bytes: &[u8]) -> Result<Catalog, String> {
    let v = json::strict(bytes, json::MAX_DEPTH)?;
    if v["schemaVersion"] != 0 {
        return Err("catalog: unsupported schemaVersion".into());
    }
    let mut providers = BTreeMap::new();
    for (name, p) in v["providers"].as_object().ok_or("catalog: no providers")? {
        let auth = match p["auth"].as_str() {
            Some("none") => Auth::None,
            Some("vault") => Auth::Vault,
            _ => return Err(format!("catalog: {name}: unknown auth")),
        };
        let mut topics = BTreeMap::new();
        for (t, f) in p["topics"]
            .as_object()
            .ok_or("catalog: topics must be an object")?
        {
            topics.insert(t.clone(), strings(f, "topic fields")?);
        }
        providers.insert(
            name.clone(),
            Provider {
                class: p["class"]
                    .as_str()
                    .ok_or("catalog: class missing")?
                    .to_owned(),
                auth,
                egress: strings(&p["egress"], "egress")?,
                topics,
                serving: p
                    .get("serving")
                    .map_or(Ok(vec![]), |s| strings(s, "serving"))?
                    .into_iter()
                    .collect(),
            },
        );
    }
    Ok(Catalog { providers })
}

/// The compiled-in catalog. A parse failure here is a build defect (a test parses it).
pub fn snapshot() -> Result<Catalog, String> {
    parse(SNAPSHOT.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_snapshot_parses_and_is_self_consistent() {
        let c = snapshot().unwrap();
        assert!(c.providers.contains_key("system") && c.providers.contains_key("github"));
        assert_eq!(c.providers["github"].auth, Auth::Vault);
        assert_eq!(c.providers["github"].egress, ["api.github.com"]);
        for (name, p) in &c.providers {
            // Every served pair names a real topic and field, and egress exists only with a vault.
            for s in &p.serving {
                assert!(
                    p.topics
                        .iter()
                        .any(|(t, fs)| fs.iter().any(|f| format!("{t}.{f}") == *s)),
                    "{name}: {s}"
                );
            }
        }
    }

    /// The catalog's `serving` list must equal what the shipped daemon providers really offer;
    /// otherwise the "will not show data" statements would be false.
    #[test]
    fn serving_matches_the_shipped_providers_in_this_workspace() {
        fn served(p: &dyn ow_core::Provider) -> BTreeSet<String> {
            let mut actual = BTreeSet::new();
            for t in p.topics() {
                for f in t.fields {
                    actual.insert(format!("{}.{}", t.name, f));
                }
            }
            actual
        }
        let catalog = snapshot().unwrap().providers;
        assert_eq!(
            catalog["system"].serving,
            served(&ow_providers::System::new())
        );
        assert_eq!(
            catalog["battery"].serving,
            served(&ow_providers::Battery::new())
        );
        for (n, p) in catalog {
            if n != "system" && n != "battery" {
                assert!(p.serving.is_empty(), "{n} claims to be served");
            }
        }
    }

    #[test]
    fn hostile_catalogs_are_refused() {
        for s in [
            r#"{"schemaVersion":1,"providers":{}}"#,
            r#"{"schemaVersion":0,"providers":{"a":{"class":"x","auth":"maybe","egress":[],"topics":{}}}}"#,
            r#"{"schemaVersion":0,"providers":{"a":{"class":"x","auth":"none","egress":[],"topics":{}},"a":{}}}"#,
            r#"{"schemaVersion":0}"#,
            "[]",
        ] {
            assert!(parse(s.as_bytes()).is_err(), "{s}");
        }
    }
}
