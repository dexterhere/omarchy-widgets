//! The access summary a person consents to, and the hash that names exactly that access.
//! Built only from the validated widget and the catalog snapshot, never from widget prose.
use super::catalog::{Auth, Catalog};
use super::grants_edit::Entry;
use super::hash::sha256;
use super::index::Listing;
use super::store::Installed;
use super::text::show;
use super::validate::Validated;
use ow_core::Grant;
use std::collections::{BTreeMap, BTreeSet};

/// One grants entry per provider the widget uses, topics and fields sorted.
pub fn entries(v: &Validated) -> Vec<Entry> {
    let mut out: Vec<Entry> = v
        .widget
        .providers
        .iter()
        .map(|p| {
            let (mut topics, mut fields) = (p.topics.clone(), p.fields.clone());
            topics.sort();
            fields.sort();
            Entry { topics, fields }
        })
        .collect();
    out.sort_by(|a, b| a.topics.cmp(&b.topics));
    out
}

/// sha256 over a canonical text of the id, the commit and the exact grants. Printed by the
/// summary and accepted by `--accept-access`, so a scripted install can only ever grant what a
/// person read.
pub fn access_hash(
    id: &str,
    commit: &str,
    registry_host: &str,
    repo_url: &str,
    files: &BTreeMap<String, String>,
    entries: &[Entry],
) -> String {
    let mut t = format!(
        "ow-access-v2\nid={id}\ncommit={commit}\nregistry={registry_host}\nrepo={repo_url}\n"
    );
    // BTreeMap order: manifest.json, layout.json, widget.json, the same every time.
    for (name, h) in files {
        t.push_str(&format!("file {name} sha256={h}\n"));
    }
    for e in entries {
        t.push_str(&format!(
            "grant topics={} fields={}\n",
            e.topics.join(","),
            e.fields.join(",")
        ));
    }
    sha256(t.as_bytes())
}

/// `topic.field` pairs the widget asks for that the daemon does not serve today.
pub fn not_served(v: &Validated, cat: &Catalog) -> Vec<String> {
    let mut out = BTreeSet::new();
    for p in &v.widget.providers {
        let Some(cp) = cat.providers.get(&p.name) else {
            continue;
        };
        for t in &p.topics {
            for f in p
                .fields
                .iter()
                .filter(|f| cp.topics.get(t).is_some_and(|fs| fs.contains(f)))
            {
                let pair = format!("{t}.{f}");
                if !cp.serving.contains(&pair) {
                    out.insert(pair);
                }
            }
        }
    }
    out.into_iter().collect()
}

/// Grant entries for the id that `ow` did not write, or that differ from its install record.
pub fn hand_written(existing: &[Grant], recorded: Option<&Installed>) -> bool {
    if existing.is_empty() {
        return false;
    }
    let Some(r) = recorded else { return true };
    let key = |t: &[String], f: &[String]| {
        let (mut t, mut f) = (t.to_vec(), f.to_vec());
        t.sort();
        f.sort();
        (t, f)
    };
    let mut have: Vec<_> = existing
        .iter()
        .map(|g| key(&g.topics, g.fields.as_deref().unwrap_or(&[])))
        .collect();
    let mut want: Vec<_> = r
        .granted
        .iter()
        .map(|e| key(&e.topics, &e.fields))
        .collect();
    have.sort();
    want.sort();
    have != want
}

/// The topics of the entries, for the replace/remove notices.
pub fn topics_of(existing: &[Grant]) -> String {
    let t: BTreeSet<&str> = existing
        .iter()
        .flat_map(|g| g.topics.iter().map(String::as_str))
        .collect();
    t.into_iter().map(show).collect::<Vec<_>>().join(", ")
}

pub struct Facts<'a> {
    pub listing: &'a Listing,
    pub validated: &'a Validated,
    pub catalog: &'a Catalog,
    pub planned_override: bool,
    pub previous: Option<&'a Installed>,
    /// Entries already in grants.toml for this widget id.
    pub existing_grants: &'a [Grant],
    pub hashes_verified: bool,
    pub registry_host: &'a str,
    pub default_registry: bool,
    pub grants_path: &'a str,
    pub widget_dir: &'a str,
}

pub fn summary(f: &Facts<'_>) -> String {
    let (l, v) = (f.listing, f.validated);
    let mut s = String::new();
    let mut line = |t: String| {
        s.push_str(&t);
        s.push('\n');
    };
    line(format!(
        "{} {} ({})",
        show(&v.manifest.name),
        show(&v.manifest.version),
        l.id
    ));
    line(format!(
        "  licence {}, status {}, review tier {}",
        show(&v.manifest.license),
        l.status.name(),
        l.tier.name()
    ));
    line(format!("  source {} at commit {}", l.repo.url(), l.commit));
    line(format!(
        "  registry {}{}",
        f.registry_host,
        if f.default_registry {
            ""
        } else {
            "  (NOT the default registry: you chose this one)"
        }
    ));
    if let Some(p) = f.previous {
        if p.commit == l.commit {
            line(format!(
                "  REINSTALL of the commit that is already installed ({})",
                show(&p.commit)
            ));
        } else {
            line(format!(
                "  UPDATE of an existing install: commit {} -> {}",
                show(&p.commit),
                l.commit
            ));
            let old: BTreeSet<&str> = p
                .granted
                .iter()
                .flat_map(|g| g.topics.iter().map(String::as_str))
                .collect();
            let new: BTreeSet<&str> = v
                .widget
                .providers
                .iter()
                .flat_map(|g| g.topics.iter().map(String::as_str))
                .collect();
            let added: Vec<&str> = new.difference(&old).copied().collect();
            let dropped: Vec<&str> = old.difference(&new).copied().collect();
            if !added.is_empty() {
                line(format!(
                    "  The update asks for NEW topics: {}",
                    added.join(", ")
                ));
            }
            if !dropped.is_empty() {
                line(format!(
                    "  The update no longer reads: {}",
                    dropped.join(", ")
                ));
            }
        }
    }
    if hand_written(f.existing_grants, f.previous) {
        line(format!(
            "  NOTE: grants.toml already has {} entr{} for this id that ow did not write (or that differ from its install record). They will be replaced: topics {}. Comments attached to them will be dropped.",
            f.existing_grants.len(),
            if f.existing_grants.len() == 1 {
                "y"
            } else {
                "ies"
            },
            topics_of(f.existing_grants)
        ));
    }
    if f.planned_override {
        line("  NOTE: this widget is only planned. It will not show data until the needed provider exists.".into());
    }
    line(String::new());
    line("This is a spec widget: JSON only. Nothing in it is run. It cannot declare network, file or program access, and this one declares none.".into());
    line(String::new());
    line("It will be allowed to read, through the daemon and field by field:".into());
    let ents = entries(v);
    if v.widget.providers.is_empty() {
        line("  nothing (it uses no provider data)".into());
    }
    for p in &v.widget.providers {
        line(format!(
            "  provider {}: topics {}; fields {}",
            p.name,
            p.topics.join(", "),
            if p.fields.is_empty() {
                "none".to_owned()
            } else {
                p.fields.join(", ")
            }
        ));
    }
    let missing = not_served(v, f.catalog);
    if !missing.is_empty() {
        line(format!(
            "  The daemon does not serve yet: {}. Those parts will show no data.",
            missing.join(", ")
        ));
    }
    line(String::new());
    let vault: Vec<(&str, &Vec<String>)> = v
        .widget
        .providers
        .iter()
        .filter_map(|p| {
            f.catalog
                .providers
                .get(&p.name)
                .filter(|c| c.auth == Auth::Vault)
                .map(|c| (p.name.as_str(), &c.egress))
        })
        .collect();
    if vault.is_empty() {
        line("Credentials: none involved.".into());
    }
    for (name, hosts) in vault {
        line(format!(
            "Credentials: provider {name} uses a credential from your vault (unlock it with `ow vault unlock`). The daemon, not the widget, contacts {}. The widget never receives the credential.",
            if hosts.is_empty() {
                "its service".to_owned()
            } else {
                hosts.join(", ")
            }
        ));
    }
    if !v.widget.regions.is_empty() {
        let r: Vec<String> = v
            .widget
            .regions
            .iter()
            .map(|(n, a)| a.as_ref().map_or(n.clone(), |a| format!("{n} ({a})")))
            .collect();
        line(format!("Interactive regions it declares: {}", r.join(", ")));
    }
    line(String::new());
    line("What this does not mean:".into());
    line("  - The daemon knows a widget only by the id it claims. Another program running as you could claim this id too, so the grant limits honest widgets and keeps credentials out of reach; it is not a barrier against your other programs.".into());
    line(format!(
        "  - ow checked the files against the website's rules and the registry's pinned commit. That is not a review of the widget by a person (tier: {}).",
        l.tier.name()
    ));
    line(if f.hashes_verified {
        "  - File hashes from the registry index matched.".into()
    } else {
        "  - The registry index carries no file hashes, so the files could not be matched to the build that checked them.".into()
    });
    line(String::new());
    line(format!(
        "It will write the widget files to {} and add this to {}:",
        f.widget_dir, f.grants_path
    ));
    for e in &ents {
        line(format!(
            "  widget {}  topics [{}]  fields [{}]",
            l.id,
            e.topics.join(", "),
            e.fields.join(", ")
        ));
    }
    for (name, h) in &v.hashes {
        line(format!("  file {name} sha256 {h}"));
    }
    line(format!(
        "  (the access hash covers these file hashes, the registry host, the repo, the commit and the grants; {})",
        if f.hashes_verified {
            "the registry index gave the file hashes and they matched"
        } else {
            "the index gave no file hashes, so they were computed from the files just fetched"
        }
    ));
    line(format!(
        "Access hash (for --accept-access): {}",
        access_hash(
            &l.id,
            &l.commit,
            f.registry_host,
            &l.repo.url(),
            &v.hashes,
            &ents
        )
    ));
    s
}
