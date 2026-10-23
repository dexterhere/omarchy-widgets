//! Validation of the three fetched files: a Rust re-implementation of
//! `registry/schema/widget.schema.json` (draft-0), `src/lib/registry/layout.ts` (layout v0) and
//! the policy gates (`check-policy.ts`, `check-providers.ts`) for the part that applies to spec
//! widgets. Pure functions over bytes. The first error found is returned; messages quote only
//! sanitized, length-capped text. Because this is a re-implementation it can drift from the
//! website's schema; the tests run it over real widget files.
use super::catalog::{Auth, Catalog};
use super::hash::sha256;
use super::index::{FILES, Listing};
use super::json;
use super::text::{dotted, field_name, plain, schema_id, show, show_value, slug};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The registry build's cap per file, and the layout's own cap.
pub const MAX_FILE: usize = 256 * 1024;
pub const MAX_LAYOUT: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub license: String,
}

#[derive(Debug, Clone)]
pub struct ProviderUse {
    pub name: String,
    pub topics: Vec<String>,
    pub fields: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Widget {
    pub providers: Vec<ProviderUse>,
    /// Interactive regions as (name, action).
    pub regions: Vec<(String, Option<String>)>,
}

#[derive(Debug, Clone)]
pub struct Validated {
    pub manifest: Manifest,
    pub widget: Widget,
    /// sha256 of the bytes as fetched, by file name.
    pub hashes: BTreeMap<String, String>,
}

type R<T> = Result<T, String>;

fn obj<'a>(v: &'a Value, at: &str) -> R<&'a Map<String, Value>> {
    v.as_object()
        .ok_or_else(|| format!("{at} must be an object"))
}

fn arr<'a>(v: &'a Value, at: &str, max: usize) -> R<&'a Vec<Value>> {
    let a = v
        .as_array()
        .ok_or_else(|| format!("{at} must be an array"))?;
    if a.len() > max {
        return Err(format!("{at} has more than {max} items"));
    }
    Ok(a)
}

fn string<'a>(v: &'a Value, at: &str) -> R<&'a str> {
    v.as_str().ok_or_else(|| format!("{at} must be a string"))
}

fn only(m: &Map<String, Value>, allowed: &[&str], at: &str) -> R<()> {
    match m.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(format!("{at}: unknown property {}", show_value(k))),
        None => Ok(()),
    }
}

fn need<'a>(m: &'a Map<String, Value>, k: &str, at: &str) -> R<&'a Value> {
    m.get(k).ok_or_else(|| format!("{at}: missing {k}"))
}

fn strs(v: &Value, at: &str, max: usize, ok: impl Fn(&str) -> bool) -> R<Vec<String>> {
    arr(v, at, max)?
        .iter()
        .map(|x| {
            let s = string(x, at)?;
            if ok(s) {
                Ok(s.to_owned())
            } else {
                Err(format!("{at}: {} is not allowed here", show_value(s)))
            }
        })
        .collect()
}

fn unique(v: &[String], at: &str) -> R<()> {
    let set: BTreeSet<&String> = v.iter().collect();
    if set.len() == v.len() {
        Ok(())
    } else {
        Err(format!("{at} repeats an item"))
    }
}

pub fn manifest(bytes: &[u8], id: &str) -> R<Manifest> {
    let v = json::strict(bytes, json::MAX_DEPTH)?;
    let m = obj(&v, "manifest.json")?;
    // Other keys are allowed: this file also belongs to Omarchy's own plugin format.
    if string(need(m, "id", "manifest.json")?, "manifest.json id")? != id {
        return Err("manifest.json id does not match the registry id".into());
    }
    let name = string(need(m, "name", "manifest.json")?, "manifest.json name")?;
    let version = string(
        need(m, "version", "manifest.json")?,
        "manifest.json version",
    )?;
    let license = string(
        need(m, "license", "manifest.json")?,
        "manifest.json license",
    )?;
    let desc = string(
        need(m, "description", "manifest.json")?,
        "manifest.json description",
    )?;
    let ver_ok = !version.is_empty()
        && version.len() <= 40
        && version
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'+' | b'-'));
    if !(plain(name) && name.chars().count() <= 80) {
        return Err("manifest.json name must be plain text up to 80 characters".into());
    }
    if !ver_ok {
        return Err("manifest.json version must match [0-9A-Za-z.+-]{1,40}".into());
    }
    if !(plain(license) && license.len() <= 40) {
        return Err("manifest.json license must be plain text up to 40 characters".into());
    }
    if !(plain(desc) && desc.chars().count() <= 2000) {
        return Err("manifest.json description must be plain text up to 2000 characters".into());
    }
    Ok(Manifest {
        name: name.into(),
        version: version.into(),
        license: license.into(),
    })
}

const CATEGORIES: [&str; 10] = [
    "system",
    "time-calendar",
    "productivity",
    "dev-git",
    "ai-agents",
    "media",
    "communication",
    "network-containers",
    "ambient-fun",
    "utilities",
];

/// widget.json against the draft-0 schema plus the spec-widget policy. Returns the parsed
/// widget and the raw value (for comparing with the index copy).
pub fn widget(bytes: &[u8], id: &str, cat: &Catalog) -> R<(Widget, Value)> {
    let v = json::strict(bytes, json::MAX_DEPTH)?;
    let m = obj(&v, "widget.json")?;
    only(
        m,
        &[
            "schemaVersion",
            "id",
            "tier",
            "category",
            "surfaces",
            "sizeClasses",
            "variants",
            "providers",
            "capabilities",
            "sampleData",
            "interactiveRegions",
            "settings",
            "compat",
        ],
        "widget.json",
    )?;
    for k in [
        "schemaVersion",
        "id",
        "tier",
        "category",
        "surfaces",
        "sizeClasses",
        "variants",
        "providers",
        "capabilities",
        "interactiveRegions",
        "settings",
        "compat",
    ] {
        need(m, k, "widget.json")?;
    }
    if m["schemaVersion"] != 0 {
        return Err("widget.json schemaVersion must be 0".into());
    }
    let wid = string(&m["id"], "widget.json id")?;
    if !schema_id(wid) {
        return Err("widget.json id is not a valid widget id".into());
    }
    if wid != id {
        return Err("widget.json id does not match the registry id".into());
    }
    match string(&m["tier"], "widget.json tier")? {
        "spec" => {}
        "code" => return Err(
            "this widget declares tier \"code\": ow installs spec widgets (JSON only), never code"
                .into(),
        ),
        _ => return Err("widget.json tier must be \"spec\" or \"code\"".into()),
    }
    if !CATEGORIES.contains(&string(&m["category"], "widget.json category")?) {
        return Err("widget.json category is not a known category".into());
    }
    let surfaces = strs(&m["surfaces"], "surfaces", 2, |s| {
        matches!(s, "desktop" | "bar")
    })?;
    if surfaces.is_empty() {
        return Err("surfaces must not be empty".into());
    }
    unique(&surfaces, "surfaces")?;
    size_classes(&m["sizeClasses"])?;
    let variants = strs(&m["variants"], "variants", 16, |s| slug(s, 24))?;
    unique(&variants, "variants")?;
    let providers = providers(&m["providers"], cat)?;
    capabilities(&m["capabilities"])?;
    if let Some(s) = m.get("sampleData")
        && obj(s, "sampleData")?.len() > 16
    {
        return Err("sampleData has more than 16 properties".into());
    }
    let regions = regions(&m["interactiveRegions"])?;
    settings(&m["settings"])?;
    let c = obj(&m["compat"], "compat")?;
    only(c, &["omarchy", "widgetsKit"], "compat")?;
    for k in ["omarchy", "widgetsKit"] {
        let s = string(need(c, k, "compat")?, "compat")?;
        if s.len() > 40 || !plain(s) {
            return Err("compat values must be plain text up to 40 characters".into());
        }
    }
    Ok((Widget { providers, regions }, v))
}

fn size_classes(v: &Value) -> R<()> {
    let a = arr(v, "sizeClasses", 8)?;
    if a.is_empty() {
        return Err("sizeClasses must not be empty".into());
    }
    for (i, c) in a.iter().enumerate() {
        let at = format!("sizeClasses[{i}]");
        let m = obj(c, &at)?;
        only(m, &["name", "min", "max", "aspectLock"], &at)?;
        if !slug(string(need(m, "name", &at)?, &at)?, 24) {
            return Err(format!("{at}.name is not allowed"));
        }
        for k in ["min", "max"] {
            let p = arr(need(m, k, &at)?, &format!("{at}.{k}"), 2)?;
            if p.len() != 2
                || !p
                    .iter()
                    .all(|n| n.as_u64().is_some_and(|n| (1..=8192).contains(&n)))
            {
                return Err(format!("{at}.{k} must be two integers from 1 to 8192"));
            }
        }
        match m.get("aspectLock") {
            None | Some(Value::Null) => {}
            Some(n) if n.as_f64().is_some_and(|f| f > 0.0 && f.is_finite()) => {}
            Some(_) => return Err(format!("{at}.aspectLock must be null or a positive number")),
        }
    }
    Ok(())
}

fn providers(v: &Value, cat: &Catalog) -> R<Vec<ProviderUse>> {
    let a = arr(v, "providers", 16)?;
    let mut out = Vec::new();
    let mut names = BTreeSet::new();
    let mut all_topics = BTreeSet::new();
    for (i, p) in a.iter().enumerate() {
        let at = format!("providers[{i}]");
        let m = obj(p, &at)?;
        only(m, &["name", "topics", "fields", "auth"], &at)?;
        let name = string(need(m, "name", &at)?, &at)?;
        if !slug(name, 32) {
            return Err(format!("{at}.name is not allowed"));
        }
        let topics = strs(need(m, "topics", &at)?, &format!("{at}.topics"), 16, |s| {
            dotted(s, 100)
        })?;
        let fields = strs(
            need(m, "fields", &at)?,
            &format!("{at}.fields"),
            32,
            field_name,
        )?;
        if topics.is_empty() {
            return Err(format!("{at}.topics must not be empty"));
        }
        unique(&topics, &format!("{at}.topics"))?;
        unique(&fields, &format!("{at}.fields"))?;
        let declared_auth = match m.get("auth") {
            None => None,
            Some(Value::String(s)) if s == "none" => Some(Auth::None),
            Some(Value::String(s)) if s == "vault" => Some(Auth::Vault),
            Some(_) => return Err(format!("{at}.auth must be \"none\" or \"vault\"")),
        };
        // Policy gate: first-party providers only, with the catalog's auth, topics and fields.
        let cp = cat.providers.get(name).ok_or_else(|| {
            format!("provider {} is not in this ow's catalog (only first-party providers exist; ow may be older than the website)", show_value(name))
        })?;
        if declared_auth.is_some_and(|a| a != cp.auth) {
            return Err(format!(
                "provider {}: declared auth differs from the catalog",
                show_value(name)
            ));
        }
        if cp.class == "secret-mail" {
            return Err("mail providers are not available in v1".into());
        }
        let mut known = BTreeSet::new();
        for t in &topics {
            let fs = cp.topics.get(t).ok_or_else(|| {
                format!(
                    "provider {}: unknown topic {}",
                    show_value(name),
                    show_value(t)
                )
            })?;
            known.extend(fs.iter().map(String::as_str));
            // The daemon rejects a grants file in which one (widget, topic) pair appears twice,
            // and that would deny every widget: a topic may be declared once per widget.
            if !all_topics.insert(t.clone()) {
                return Err(format!(
                    "topic {} is declared more than once",
                    show_value(t)
                ));
            }
        }
        if let Some(f) = fields.iter().find(|f| !known.contains(f.as_str())) {
            return Err(format!(
                "provider {}: field {} is not offered by its topics",
                show_value(name),
                show_value(f)
            ));
        }
        if !names.insert(name.to_owned()) {
            return Err(format!(
                "provider {} is listed more than once",
                show_value(name)
            ));
        }
        out.push(ProviderUse {
            name: name.into(),
            topics,
            fields,
        });
    }
    Ok(out)
}

/// Spec widgets declare no network, files or spawn.
fn capabilities(v: &Value) -> R<()> {
    let m = obj(v, "capabilities")?;
    only(m, &["network", "files", "spawn"], "capabilities")?;
    for k in ["network", "files", "spawn"] {
        if !arr(
            need(m, k, "capabilities")?,
            &format!("capabilities.{k}"),
            16,
        )?
        .is_empty()
        {
            return Err("spec widgets declare no network, files or spawn".into());
        }
    }
    Ok(())
}

fn regions(v: &Value) -> R<Vec<(String, Option<String>)>> {
    let mut out = Vec::new();
    for (i, r) in arr(v, "interactiveRegions", 16)?.iter().enumerate() {
        let at = format!("interactiveRegions[{i}]");
        let m = obj(r, &at)?;
        only(m, &["name", "action"], &at)?;
        let name = string(need(m, "name", &at)?, &at)?;
        if !slug(name, 32) {
            return Err(format!("{at}.name is not allowed"));
        }
        let action = match m.get("action") {
            None => None,
            Some(a) => {
                let a = string(a, &at)?;
                let ok = !a.is_empty()
                    && a.len() <= 64
                    && a.as_bytes()[0].is_ascii_lowercase()
                    && a.bytes().all(|c| {
                        c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'.' || c == b'-'
                    });
                if !ok {
                    return Err(format!("{at}.action is not allowed"));
                }
                Some(a.to_owned())
            }
        };
        out.push((name.to_owned(), action));
    }
    Ok(out)
}

fn settings(v: &Value) -> R<()> {
    for (i, s) in arr(v, "settings", 32)?.iter().enumerate() {
        let at = format!("settings[{i}]");
        let m = obj(s, &at)?;
        only(
            m,
            &["key", "type", "label", "default", "min", "max", "options"],
            &at,
        )?;
        let key = string(need(m, "key", &at)?, &at)?;
        let key_ok = !key.is_empty()
            && key.len() <= 32
            && key.as_bytes()[0].is_ascii_lowercase()
            && key
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-');
        if !key_ok {
            return Err(format!("{at}.key is not allowed"));
        }
        if !matches!(
            string(need(m, "type", &at)?, &at)?,
            "int" | "number" | "bool" | "string" | "enum"
        ) {
            return Err(format!("{at}.type is not allowed"));
        }
        match need(m, "default", &at)? {
            Value::Bool(_) | Value::Number(_) => {}
            Value::String(d) if d.chars().count() <= 200 && plain_or_empty(d) => {}
            _ => {
                return Err(format!(
                    "{at}.default must be a scalar of at most 200 plain characters"
                ));
            }
        }
        if let Some(l) = m.get("label") {
            let l = string(l, &at)?;
            if l.chars().count() > 80 || !plain_or_empty(l) {
                return Err(format!("{at}.label must be plain text up to 80 characters"));
            }
        }
        for k in ["min", "max"] {
            if m.get(k).is_some_and(|n| !n.is_number()) {
                return Err(format!("{at}.{k} must be a number"));
            }
        }
        if let Some(o) = m.get("options") {
            for x in arr(o, &at, 32)? {
                let x = string(x, &at)?;
                if x.chars().count() > 40 || !plain_or_empty(x) {
                    return Err(format!(
                        "{at}.options must be plain text up to 40 characters"
                    ));
                }
            }
        }
    }
    Ok(())
}

fn plain_or_empty(s: &str) -> bool {
    s.is_empty() || plain(s)
}

// ---- layout.json (layout v0) -------------------------------------------------------------

const ROLES: [&str; 4] = ["accent", "foreground", "secondary", "muted"];
const SIZES: [&str; 4] = ["sm", "md", "lg", "xl"];
const GAPS: [&str; 3] = ["sm", "md", "lg"];
const ICONS: [&str; 12] = [
    "cpu",
    "memory",
    "battery",
    "clock",
    "list",
    "git",
    "container",
    "disk",
    "network",
    "bolt",
    "calendar",
    "check",
];
const DEPTH: usize = 8;
const NODES: usize = 200;
const CHILDREN: usize = 20;
const TEXT: usize = 120;

fn node_keys(t: &str) -> Option<&'static [&'static str]> {
    Some(match t {
        "stack" | "row" => &["type", "gap", "children"],
        "grid" => &["type", "columns", "gap", "children"],
        "text" | "badge" => &["type", "text", "bind", "size", "color"],
        "value" => &["type", "bind", "unit", "size", "color"],
        "bar" | "ring" | "sparkline" => &["type", "bind", "color"],
        "list" => &["type", "bind", "max", "size", "color"],
        "icon" => &["type", "name", "size", "color"],
        "spacer" | "divider" => &["type"],
        _ => return None,
    })
}

struct Walk {
    nodes: usize,
    binds: Vec<(String, String)>,
}

impl Walk {
    fn node(&mut self, n: &Value, depth: usize, at: &str) -> R<()> {
        self.nodes += 1;
        if self.nodes > NODES {
            return Err(format!("layout has more than {NODES} nodes"));
        }
        if depth > DEPTH {
            return Err(format!("{at}: deeper than {DEPTH}"));
        }
        let m = obj(n, at)?;
        let t = m
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{at}.type is not a known node"))?;
        let keys = node_keys(t).ok_or_else(|| format!("{at}.type is not a known node"))?;
        only(m, keys, at)?;
        let one_of = |k: &str, set: &[&str]| -> R<()> {
            match m.get(k) {
                None => Ok(()),
                Some(Value::String(s)) if set.contains(&s.as_str()) => Ok(()),
                Some(_) => Err(format!("{at}.{k} is not an allowed value")),
            }
        };
        one_of("color", &ROLES)?;
        one_of("size", &SIZES)?;
        one_of("gap", &GAPS)?;
        if matches!(t, "stack" | "row" | "grid") {
            let ch = m
                .get("children")
                .and_then(Value::as_array)
                .filter(|c| (1..=CHILDREN).contains(&c.len()))
                .ok_or_else(|| format!("{at}.children must hold 1 to {CHILDREN} nodes"))?;
            if t == "grid"
                && !m
                    .get("columns")
                    .and_then(Value::as_u64)
                    .is_some_and(|c| (2..=4).contains(&c))
            {
                return Err(format!("{at}.columns must be 2, 3 or 4"));
            }
            for (i, c) in ch.iter().enumerate() {
                self.node(c, depth + 1, &format!("{at}.children[{i}]"))?;
            }
            return Ok(());
        }
        let bind = m.get("bind");
        if matches!(t, "text" | "badge") {
            let text = m.get("text");
            if text.is_some() == bind.is_some() {
                return Err(format!("{at} needs exactly one of text or bind"));
            }
            if let Some(tx) = text {
                match tx.as_str() {
                    Some(s) if s.chars().count() <= TEXT && plain(s) => {}
                    _ => {
                        return Err(format!(
                            "{at}.text must be plain text up to {TEXT} characters"
                        ));
                    }
                }
            }
        } else if matches!(t, "value" | "bar" | "ring" | "sparkline" | "list") && bind.is_none() {
            return Err(format!("{at} needs a bind"));
        }
        if let Some(b) = bind {
            let bm = obj(b, &format!("{at}.bind"))?;
            let (topic, field) = (
                bm.get("topic").and_then(Value::as_str),
                bm.get("field").and_then(Value::as_str),
            );
            match (bm.len(), topic, field) {
                (2, Some(tp), Some(f)) if dotted(tp, 100) && field_name(f) => {
                    self.binds.push((tp.into(), f.into()))
                }
                _ => {
                    return Err(format!(
                        "{at}.bind must be {{topic, field}} with valid names"
                    ));
                }
            }
        }
        if t == "value"
            && let Some(u) = m.get("unit")
        {
            match u.as_str() {
                Some(s) if s.chars().count() <= 4 && plain(s) => {}
                _ => return Err(format!("{at}.unit must be plain text up to 4 characters")),
            }
        }
        if t == "list"
            && m.get("max")
                .is_some_and(|x| !x.as_u64().is_some_and(|n| (1..=50).contains(&n)))
        {
            return Err(format!("{at}.max must be 1 to 50"));
        }
        if t == "icon"
            && !m
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|n| ICONS.contains(&n))
        {
            return Err(format!("{at}.name is not a known icon"));
        }
        Ok(())
    }
}

/// layout.json: node rules, limits, variants declared in widget.json, every bind declared in the
/// widget's providers. Returns the raw value for the index comparison.
pub fn layout(bytes: &[u8], w: &Widget, widget_variants: &[String]) -> R<Value> {
    let v = json::strict(bytes, json::MAX_DEPTH)?;
    let m = obj(&v, "layout.json")?;
    only(m, &["schemaVersion", "root", "variants"], "layout.json")?;
    if m.get("schemaVersion") != Some(&Value::from(0)) {
        return Err("layout.json schemaVersion must be 0".into());
    }
    let mut walk = Walk {
        nodes: 0,
        binds: Vec::new(),
    };
    walk.node(need(m, "root", "layout.json")?, 1, "root")?;
    if let Some(vs) = m.get("variants") {
        for (name, node) in obj(vs, "variants")? {
            if !widget_variants.iter().any(|d| d == name) {
                return Err(format!(
                    "layout variant {} is not declared in widget.json",
                    show_value(name)
                ));
            }
            walk.node(node, 1, &format!("variants.{}", show(name)))?;
        }
    }
    let topics: BTreeSet<&str> = w
        .providers
        .iter()
        .flat_map(|p| p.topics.iter().map(String::as_str))
        .collect();
    let fields: BTreeSet<&str> = w
        .providers
        .iter()
        .flat_map(|p| p.fields.iter().map(String::as_str))
        .collect();
    for (t, f) in &walk.binds {
        if !topics.contains(t.as_str()) || !fields.contains(f.as_str()) {
            return Err(format!(
                "layout binds {}.{} which widget.json does not declare",
                show_value(t),
                show_value(f)
            ));
        }
    }
    Ok(v)
}

/// Everything between the three byte strings and a decision. `index_hashes` come from the
/// listing; the index's inline copies of providers/capabilities/layout must equal the files.
pub fn check_files(
    l: &Listing,
    cat: &Catalog,
    manifest_b: &[u8],
    widget_b: &[u8],
    layout_b: &[u8],
) -> R<Validated> {
    for (name, b, cap) in [
        ("manifest.json", manifest_b, MAX_FILE),
        ("widget.json", widget_b, MAX_FILE),
        ("layout.json", layout_b, MAX_LAYOUT),
    ] {
        if b.len() > cap {
            return Err(format!("{name} is larger than {} KB", cap / 1024));
        }
    }
    let hashes: BTreeMap<String, String> = FILES
        .iter()
        .zip([manifest_b, widget_b, layout_b])
        .map(|(n, b)| ((*n).to_owned(), sha256(b)))
        .collect();
    if let Some(want) = &l.sha256 {
        for f in FILES {
            match want.get(f) {
                None => return Err(format!("the index carries hashes but none for {f}")),
                Some(h) if *h != hashes[f] => {
                    return Err(format!("{f} does not match the hash in the registry index"));
                }
                Some(_) => {}
            }
        }
    }
    let mf = manifest(manifest_b, &l.id)?;
    if l.name.as_ref().is_some_and(|n| *n != mf.name)
        || l.version.as_ref().is_some_and(|v| *v != mf.version)
    {
        return Err("manifest.json name or version differs from the registry index".into());
    }
    let (w, wv) = widget(widget_b, &l.id, cat)?;
    let variants: Vec<String> = wv["variants"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let lv = layout(layout_b, &w, &variants)?;
    for (what, from_index, fetched) in [
        ("providers", &l.providers, &wv["providers"]),
        ("capabilities", &l.capabilities, &wv["capabilities"]),
        ("layout", &l.layout, &lv),
    ] {
        if from_index.as_ref().is_some_and(|i| i != fetched) {
            return Err(format!(
                "the fetched {what} differ from what the registry index shows; refusing"
            ));
        }
    }
    Ok(Validated {
        manifest: mf,
        widget: w,
        hashes,
    })
}
