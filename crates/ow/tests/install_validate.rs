//! Hostile and real inputs for the pure validators: the seven real widget files must pass, and
//! every way a third party could bend a file must be refused. No I/O.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod install_common;
use install_common::*;
use ow::install::catalog;
use ow::install::index::{Listing, select_listing};
use ow::install::validate::{MAX_FILE, MAX_LAYOUT, Validated, check_files};
use serde_json::{Value, json};

const WIDGETS: [&str; 7] = [
    "ai-usage",
    "battery",
    "clock",
    "docker",
    "github-activity",
    "system-monitor",
    "tasks",
];

fn drop_key(e: &mut Value, k: &str) {
    e.as_object_mut().unwrap().remove(k);
}

fn listing(widget: &str, edit: impl FnOnce(&mut Value)) -> Listing {
    let w = world(widget, SHA_A, "alpha", "community", edit);
    select_listing(&w.0.index, &id_of(widget)).unwrap()
}

type Files = (Vec<u8>, Vec<u8>, Vec<u8>);

fn files(widget: &str) -> Files {
    (
        fixture(widget, "manifest.json"),
        fixture(widget, "widget.json"),
        fixture(widget, "layout.json"),
    )
}

fn run(l: &Listing, f: &Files) -> Result<Validated, String> {
    check_files(l, &catalog::snapshot().unwrap(), &f.0, &f.1, &f.2)
}

/// Mutate one parsed file of the clock widget and expect a refusal mentioning `why`.
fn refused(which: usize, why: &str, m: impl FnOnce(&mut Value)) {
    let mut f = files("clock");
    let bytes = match which {
        0 => &mut f.0,
        1 => &mut f.1,
        _ => &mut f.2,
    };
    let mut v: Value = serde_json::from_slice(bytes).unwrap();
    m(&mut v);
    *bytes = serde_json::to_vec(&v).unwrap();
    let e = run(&listing("clock", |_| {}), &f).unwrap_err();
    assert!(e.contains(why), "wanted {why:?}, got {e:?}");
}

fn refused_raw(which: usize, why: &str, bytes: Vec<u8>) {
    let mut f = files("clock");
    match which {
        0 => f.0 = bytes,
        1 => f.1 = bytes,
        _ => f.2 = bytes,
    }
    let e = run(&listing("clock", |_| {}), &f).unwrap_err();
    assert!(e.contains(why), "wanted {why:?}, got {e:?}");
}

#[test]
fn all_seven_real_widgets_pass_against_the_real_index_entries() {
    for w in WIDGETS {
        let l = listing(w, |_| {});
        // The shipped index carries providers, capabilities and layout: they must equal the files.
        assert!(
            l.providers.is_some() && l.capabilities.is_some() && l.layout.is_some(),
            "{w}"
        );
        let v = run(&l, &files(w)).unwrap_or_else(|e| panic!("{w}: {e}"));
        assert_eq!(v.hashes.len(), 3);
        assert!(!v.widget.providers.is_empty());
    }
}

#[test]
fn matching_hashes_pass_and_any_difference_is_refused() {
    let f = files("clock");
    let h = |b: &[u8]| ow::install::hash::sha256(b);
    let good = json!({"manifest.json": h(&f.0), "widget.json": h(&f.1), "layout.json": h(&f.2)});
    assert!(run(&listing("clock", |e| e["sha256"] = good.clone()), &f).is_ok());
    for file in ["manifest.json", "widget.json", "layout.json"] {
        let mut bad = good.clone();
        bad[file] = json!(h(b"something else"));
        let e = run(&listing("clock", |e| e["sha256"] = bad), &f).unwrap_err();
        assert!(e.contains(file) && e.contains("hash"), "{e}");
        let mut partial = good.clone();
        partial.as_object_mut().unwrap().remove(file);
        let e = run(&listing("clock", |e| e["sha256"] = partial), &f).unwrap_err();
        assert!(e.contains("none for"), "{e}");
    }
    // One flipped byte in the served file is caught by the hash even though the JSON is valid.
    let mut flipped = f.clone();
    flipped.1.extend_from_slice(b"\n");
    assert!(
        run(&listing("clock", |e| e["sha256"] = good), &flipped)
            .unwrap_err()
            .contains("hash")
    );
}

#[test]
fn a_spec_widget_declaring_network_files_or_spawn_is_refused() {
    for (k, v) in [
        ("network", "evil.example.com"),
        ("files", "~/.ssh"),
        ("spawn", "date"),
    ] {
        refused(1, "no network, files or spawn", |w| {
            w["capabilities"][k] = json!([v])
        });
    }
    // Index shows empty capabilities; file differs -> also refused (by the policy first).
    refused(1, "no network", |w| {
        w["capabilities"]["network"] = json!(["api.github.com"])
    });
}

#[test]
fn code_tier_and_unknown_tier_in_widget_json_are_refused() {
    refused(1, "never code", |w| w["tier"] = json!("code"));
    for t in ["Spec", "core", "", "spec "] {
        refused(1, "tier must be", |w| w["tier"] = json!(t));
    }
}

#[test]
fn ids_must_agree_everywhere() {
    refused(1, "widget.json id does not match", |w| {
        w["id"] = json!("io.github.dexterhere.other")
    });
    refused(0, "manifest.json id does not match", |m| {
        m["id"] = json!("io.github.dexterhere.other")
    });
    refused(1, "not a valid widget id", |w| {
        w["id"] = json!("../../etc/passwd")
    });
    refused(1, "not a valid widget id", |w| {
        w["id"] = json!("omarchy.clock")
    });
    refused(0, "id", |m| {
        m.as_object_mut().unwrap().remove("id");
    });
}

#[test]
fn providers_must_exist_in_the_catalog_with_matching_auth() {
    refused(1, "not in this ow's catalog", |w| {
        w["providers"][0]["name"] = json!("mail")
    });
    refused(1, "unknown topic", |w| {
        w["providers"][0]["topics"] = json!(["system.nope"])
    });
    refused(1, "not offered by its topics", |w| {
        w["providers"][0]["fields"] = json!(["hm", "secret"])
    });
    refused(1, "differs from the catalog", |w| {
        w["providers"][0]["auth"] = json!("vault")
    });
    // The other direction: a vault provider claiming no credential.
    let mut f = files("github-activity");
    let mut w: Value = serde_json::from_slice(&f.1).unwrap();
    w["providers"][0]["auth"] = json!("none");
    f.1 = serde_json::to_vec(&w).unwrap();
    assert!(
        run(&listing("github-activity", |_| {}), &f)
            .unwrap_err()
            .contains("differs from the catalog")
    );
}

#[test]
fn repeated_topics_or_providers_are_refused_because_they_would_break_the_grants_file() {
    refused(1, "declared more than once", |w| {
        let p = w["providers"][0].clone();
        w["providers"] = json!([p.clone(), p]);
    });
    refused(1, "repeats an item", |w| {
        w["providers"][0]["topics"] = json!(["system.time", "system.time"])
    });
    refused(1, "listed more than once", |w| {
        let p = w["providers"][0].clone();
        let mut q = p.clone();
        q["topics"] = json!(["system.cpu"]);
        q["fields"] = json!(["usage_pct"]);
        w["providers"] = json!([p, q]);
    });
}

#[test]
fn unknown_properties_and_wrong_types_are_refused_at_every_level() {
    refused(1, "unknown property", |w| w["extra"] = json!(1));
    refused(1, "unknown property", |w| w["providers"][0]["x"] = json!(1));
    refused(1, "unknown property", |w| {
        w["capabilities"]["x"] = json!([])
    });
    refused(1, "unknown property", |w| {
        w["sizeClasses"][0]["x"] = json!(1)
    });
    refused(1, "unknown property", |w| w["compat"]["x"] = json!("1"));
    refused(1, "schemaVersion", |w| w["schemaVersion"] = json!(1));
    refused(1, "missing", |w| {
        w.as_object_mut().unwrap().remove("compat");
    });
    refused(1, "must be an array", |w| w["variants"] = json!("a"));
    refused(1, "must be an object", |w| *w = json!([]));
    refused(1, "category", |w| w["category"] = json!("hacking"));
    refused(1, "not allowed", |w| w["variants"] = json!(["Bad Name"]));
    refused(1, "repeats", |w| w["variants"] = json!(["a", "a"]));
    refused(1, "more than 16", |w| {
        w["variants"] = json!((0..17).map(|i| format!("v{i}")).collect::<Vec<_>>())
    });
    refused(1, "two integers", |w| {
        w["sizeClasses"][0]["min"] = json!([0, 5])
    });
    refused(1, "two integers", |w| {
        w["sizeClasses"][0]["max"] = json!([9000, 5])
    });
    refused(1, "aspectLock", |w| {
        w["sizeClasses"][0]["aspectLock"] = json!(-1)
    });
    refused(1, "more than 8", |w| {
        w["sizeClasses"] = json!(
            (0..9)
                .map(|_| w["sizeClasses"][0].clone())
                .collect::<Vec<_>>()
        )
    });
    refused(1, "sampleData", |w| {
        w["sampleData"] = json!(
            (0..17)
                .map(|i| (format!("t{i}"), json!({})))
                .collect::<serde_json::Map<_, _>>()
        )
    });
    refused(1, "not allowed", |w| {
        w["interactiveRegions"] = json!([{"name": "Bad Name"}])
    });
    refused(1, "type", |w| {
        w["settings"] = json!([{"key": "a", "type": "script", "default": 1}])
    });
    refused(1, "plain", |w| w["compat"]["omarchy"] = json!("<script>"));
}

#[test]
fn manifest_rules() {
    refused(0, "missing license", |m| {
        m.as_object_mut().unwrap().remove("license");
    });
    refused(0, "version", |m| m["version"] = json!("1.0 /../"));
    refused(0, "name", |m| m["name"] = json!("Clock\u{1b}[31m"));
    refused(0, "name", |m| m["name"] = json!("<b>Clock</b>"));
    refused(0, "name", |m| m["name"] = json!("x".repeat(81)));
    refused(0, "license", |m| m["license"] = json!("MIT\u{202e}"));
    refused(0, "must be a string", |m| m["description"] = json!(5));
    refused(0, "must be an object", |m| *m = json!("x"));
    // Extra keys are fine (Omarchy's own plugin keys live in this file).
    let mut f = files("clock");
    let mut m: Value = serde_json::from_slice(&f.0).unwrap();
    m["entry"] = json!("Main.qml");
    f.0 = serde_json::to_vec(&m).unwrap();
    assert!(run(&listing("clock", |_| {}), &f).is_ok());
    // The index's name/version must agree with the manifest.
    let e = run(
        &listing("clock", |e| e["version"] = json!("9.9.9")),
        &files("clock"),
    )
    .unwrap_err();
    assert!(e.contains("differs from the registry index"), "{e}");
}

#[test]
fn size_caps_apply_before_parsing() {
    let pad = |mut b: Vec<u8>, n: usize| {
        b.resize(n, b' ');
        b
    };
    refused_raw(
        1,
        "larger than 256 KB",
        pad(fixture("clock", "widget.json"), MAX_FILE + 1),
    );
    refused_raw(
        0,
        "larger than 256 KB",
        pad(fixture("clock", "manifest.json"), MAX_FILE + 1),
    );
    refused_raw(
        2,
        "larger than 64 KB",
        pad(fixture("clock", "layout.json"), MAX_LAYOUT + 1),
    );
    // At the cap it is still parsed (trailing spaces are valid JSON).
    let mut f = files("clock");
    f.2 = pad(f.2, MAX_LAYOUT);
    assert!(run(&listing("clock", |_| {}), &f).is_ok());
}

#[test]
fn duplicate_keys_and_deep_nesting_are_refused_in_every_file() {
    let dup = |s: &str| s.replacen('{', "{\"id\":\"x\",", 1);
    for (i, name) in ["manifest.json", "widget.json", "layout.json"]
        .into_iter()
        .enumerate()
    {
        let text = String::from_utf8(fixture("clock", name)).unwrap();
        // Layout has no "id": duplicate a key it does have.
        let bad = if i == 2 {
            text.replacen('{', "{\"schemaVersion\":0,", 1)
        } else {
            dup(&text)
        };
        refused_raw(i, "duplicate object key", bad.into_bytes());
    }
    let deep = format!("{}1{}", "[".repeat(40), "]".repeat(40));
    refused(1, "nested too deeply", |w| {
        w["sampleData"] = serde_json::from_str(&format!("{{\"a\":{deep}}}")).unwrap()
    });
    refused_raw(1, "not valid JSON", b"\xef\xbb\xbf{}".to_vec());
    refused_raw(2, "not valid JSON", b"{} trailing".to_vec());
    refused_raw(0, "not valid JSON", vec![0xff, 0xfe]);
    refused_raw(1, "not valid JSON", Vec::new());
}

#[test]
fn layout_rules() {
    let node = |t: &str| json!({"type": t});
    refused(2, "not a known node", |l| l["root"] = node("script"));
    refused(2, "not a known node", |l| l["root"] = node("__proto__"));
    refused(2, "schemaVersion", |l| l["schemaVersion"] = json!(1));
    refused(2, "unknown property", |l| l["extra"] = json!(1));
    refused(2, "unknown property", |l| l["root"]["onclick"] = json!("x"));
    refused(2, "color", |l| {
        l["root"]["children"][0]["color"] = json!("#ff0000")
    });
    refused(2, "size", |l| {
        l["root"]["children"][0]["size"] = json!("huge")
    });
    refused(2, "gap", |l| l["root"]["gap"] = json!("1px"));
    refused(2, "exactly one of text or bind", |l| {
        l["root"]["children"][1]["text"] = json!("x")
    });
    refused(2, "plain text", |l| {
        l["root"]["children"][1]
            .as_object_mut()
            .unwrap()
            .remove("bind");
        l["root"]["children"][1]["text"] = json!("<img src=x onerror=1>");
    });
    refused(2, "plain text", |l| {
        l["root"]["children"][1]
            .as_object_mut()
            .unwrap()
            .remove("bind");
        l["root"]["children"][1]["text"] = json!("a".repeat(121));
    });
    refused(2, "does not declare", |l| {
        l["root"]["children"][0]["bind"] = json!({"topic": "github.prs", "field": "open"})
    });
    refused(2, "does not declare", |l| {
        l["root"]["children"][0]["bind"]["field"] = json!("nope")
    });
    refused(2, "bind must be", |l| {
        l["root"]["children"][0]["bind"]["extra"] = json!(1)
    });
    refused(2, "bind must be", |l| {
        l["root"]["children"][0]["bind"] = json!("system.time.hm")
    });
    refused(2, "not declared in widget.json", |l| {
        l["variants"]["evil"] = l["root"].clone()
    });
    refused(2, "children must hold", |l| {
        l["root"]["children"] = json!([])
    });
    refused(2, "children must hold", |l| {
        l["root"]["children"] = json!((0..21).map(|_| node("spacer")).collect::<Vec<_>>())
    });
    refused(2, "columns", |l| {
        l["root"]["type"] = json!("grid");
        l["root"]["columns"] = json!(5);
    });
    refused(2, "icon", |l| {
        l["root"]["children"][0] = json!({"type": "icon", "name": "../../x"})
    });
    refused(2, "needs a bind", |l| {
        l["root"]["children"][0] = node("value")
    });
    refused(
        2,
        "max",
        |l| {
            l["root"]["children"][0] =
                json!({"type": "list", "max": 51, "bind": {"topic": "system.time", "field": "hm"}})
        },
    );
    refused(2, "unit", |l| {
        l["root"]["children"][0]["unit"] = json!("12345")
    });
}

#[test]
fn layout_depth_and_node_limits() {
    let nest = |d: usize| {
        let mut n = json!({"type": "spacer"});
        for _ in 0..d {
            n = json!({"type": "stack", "children": [n]});
        }
        n
    };
    // Depth counts nodes from the root as 1; the limit is 8.
    let mut f = files("clock");
    let put = |f: &mut Files, root: Value| {
        f.2 = serde_json::to_vec(&json!({"schemaVersion": 0, "root": root})).unwrap()
    };
    put(&mut f, nest(7));
    run(&listing("clock", |e| drop_key(e, "layout")), &f).unwrap();
    put(&mut f, nest(8));
    assert!(
        run(&listing("clock", |e| drop_key(e, "layout")), &f)
            .unwrap_err()
            .contains("deeper than 8")
    );
    // 201 nodes: 10 stacks of 20 children plus the root and the stacks themselves.
    let leaf = json!({"type": "spacer"});
    let stack = json!({"type": "stack", "children": vec![leaf; 20]});
    put(
        &mut f,
        json!({"type": "stack", "children": vec![stack; 10]}),
    );
    assert!(
        run(&listing("clock", |e| drop_key(e, "layout")), &f)
            .unwrap_err()
            .contains("200 nodes")
    );
}

#[test]
fn what_the_index_showed_must_equal_what_installs() {
    let l = listing("clock", |e| {
        e["capabilities"]["network"] = json!(["api.github.com"])
    });
    assert!(
        run(&l, &files("clock"))
            .unwrap_err()
            .contains("capabilities differ")
    );
    let l = listing("clock", |e| e["providers"][0]["fields"] = json!(["hm"]));
    assert!(
        run(&l, &files("clock"))
            .unwrap_err()
            .contains("providers differ")
    );
    let l = listing("clock", |e| {
        e["layout"]["root"]["children"] = json!([{"type": "spacer"}])
    });
    assert!(
        run(&l, &files("clock"))
            .unwrap_err()
            .contains("layout differ")
    );
    // An index without the inline copies is accepted (they are optional).
    let l = listing("clock", |e| {
        let o = e.as_object_mut().unwrap();
        for k in ["providers", "capabilities", "layout"] {
            o.remove(k);
        }
    });
    assert!(run(&l, &files("clock")).is_ok());
}

#[test]
fn hostile_text_in_names_is_never_printable() {
    for bad in ["a\u{1b}[2J", "a\u{202e}b", "a\u{200b}", "a\nb", "\u{0}"] {
        refused(0, "name", |m| m["name"] = json!(bad));
    }
}

#[test]
fn names_use_a_printable_allow_list_and_legitimate_non_latin_names_pass() {
    // The index entry carries no name here, so only the manifest's own checks can refuse.
    let refuse_name = |bad: &str| {
        let mut f = files("clock");
        let mut m: Value = serde_json::from_slice(&f.0).unwrap();
        m["name"] = json!(bad);
        f.0 = serde_json::to_vec(&m).unwrap();
        let e = run(&listing("clock", |e| drop_key(e, "name")), &f).unwrap_err();
        assert!(
            e.contains("manifest.json name must be plain text"),
            "{bad:?}: {e}"
        );
    };
    // The audit's example: Cf characters, an unassigned code point, a phonetic letter.
    for bad in [
        "Clock\u{0600}\u{06DD}\u{0378}\u{1D17}3x",
        "Clock\u{0378}",
        "Clock \u{1F600}",
        "Clock\u{200D}",
        "Cl\u{070F}ock",
        "Cl\u{1D17}\u{0378}",
    ] {
        refuse_name(bad);
    }
    refused(0, "description", |m| {
        m["description"] = json!("nice \u{1F600}")
    });
    refused(0, "license", |m| m["license"] = json!("MIT\u{2060}"));
    // Length is counted in UTF-16 units like the website: 41 astral characters are 82 units.
    refuse_name(&"\u{1D400}".repeat(41));
    for ok in [
        "\u{6642}\u{8a08}\u{30a6}\u{30a3}\u{30b8}\u{30a7}\u{30c3}\u{30c8}",
        "\u{0918}\u{0921}\u{0940} \u{0935}\u{093f}\u{091c}\u{0947}\u{091f} \u{0915}\u{094d}\u{0932}\u{0915}",
        "Caf\u{e9} \u{2014} it\u{2019}s 9\u{b0}",
    ] {
        let mut f = files("clock");
        let mut m: Value = serde_json::from_slice(&f.0).unwrap();
        m["name"] = json!(ok);
        f.0 = serde_json::to_vec(&m).unwrap();
        run(&listing("clock", |e| e["name"] = json!(ok)), &f)
            .unwrap_or_else(|e| panic!("{ok}: {e}"));
    }
}

#[test]
fn format_characters_hidden_in_any_string_or_key_are_refused() {
    refused(1, "plain", |w| {
        w["settings"] = json!([{"key": "a", "type": "string", "default": "x\u{0600}y"}])
    });
    refused(1, "forbidden key", |w| w["extra\u{202e}"] = json!(1));
    refused(1, "forbidden key", |w| {
        w["sampleData"]["__proto__"] = json!({})
    });
    refused(2, "plain", |l| {
        l["root"]["children"][0]["unit"] = json!("\u{200b}")
    });
}

#[test]
fn sample_data_is_checked_like_the_website_does() {
    let with = |sd: Value| {
        let mut f = files("clock");
        let mut w: Value = serde_json::from_slice(&f.1).unwrap();
        w["sampleData"] = sd;
        f.1 = serde_json::to_vec(&w).unwrap();
        run(&listing("clock", |_| {}), &f)
    };
    let good =
        json!({"system.time": {"hm": "10:09", "hms": "10:09:42", "date": "Mon", "tz": "NPT"}});
    assert!(with(good.clone()).is_ok());
    // Every bind needs sample data.
    let mut missing = good.clone();
    missing["system.time"].as_object_mut().unwrap().remove("tz");
    assert!(with(missing).unwrap_err().contains("no sampleData"));
    assert!(with(json!({})).unwrap_err().contains("no sampleData"));
    for (bad, why) in [
        (json!("x"), "must be an object"),
        (json!({"Bad Topic": {}}), "invalid topic"),
        (json!({"system.time": []}), "invalid topic"),
        (json!({"system.time": {"hm": [1, "a"]}}), "mixed types"),
        (
            json!({"system.time": {"hm": (0..51).collect::<Vec<_>>()}}),
            "too many items",
        ),
        (json!({"system.time": {"hm": "x\u{1b}[31m"}}), "plain text"),
        (
            json!({"system.time": {"hm": "a".repeat(121)}}),
            "plain strings",
        ),
        (json!({"system.time": {"hm": true}}), "plain strings"),
        (json!({"system.time": {"hm": null}}), "plain strings"),
        (json!({"system.time": {"bad field": 1}}), "invalid field"),
    ] {
        let mut sd = good.clone();
        let tgt = bad.clone();
        if bad.is_object()
            && bad.as_object().unwrap().contains_key("system.time")
            && bad["system.time"].is_object()
        {
            for (k, v) in tgt["system.time"].as_object().unwrap() {
                sd["system.time"][k] = v.clone();
            }
        } else {
            sd = bad;
        }
        let e = with(sd).unwrap_err();
        assert!(e.contains(why), "{why}: {e}");
    }
}

#[test]
fn bound_nodes_need_sample_data_of_the_right_shape() {
    // battery: a bar needs a number, a sparkline a series of at least two numbers.
    let l = listing("battery", |_| {});
    let base = files("battery");
    let mut w: Value = serde_json::from_slice(&base.1).unwrap();
    let mut lay: Value = serde_json::from_slice(&base.2).unwrap();
    let topic = w["providers"][0]["topics"][0].as_str().unwrap().to_owned();
    let field = w["providers"][0]["fields"][0].as_str().unwrap().to_owned();
    lay["root"] = json!({"type": "bar", "bind": {"topic": topic, "field": field}});
    lay.as_object_mut().unwrap().remove("variants");
    for (sample, ok) in [
        (json!(55), true),
        (json!("high"), false),
        (json!([1, 2]), false),
    ] {
        w["sampleData"] = json!({ topic.clone(): { field.clone(): sample } });
        let f = (
            base.0.clone(),
            serde_json::to_vec(&w).unwrap(),
            serde_json::to_vec(&lay).unwrap(),
        );
        let mut l2 = l.clone();
        l2.providers = None;
        l2.layout = None;
        assert_eq!(run(&l2, &f).is_ok(), ok);
    }
    lay["root"]["type"] = json!("sparkline");
    for (sample, ok) in [
        (json!([1, 2, 3]), true),
        (json!([1]), false),
        (json!(5), false),
    ] {
        w["sampleData"] = json!({ topic.clone(): { field.clone(): sample } });
        let f = (
            base.0.clone(),
            serde_json::to_vec(&w).unwrap(),
            serde_json::to_vec(&lay).unwrap(),
        );
        let mut l2 = l.clone();
        l2.providers = None;
        l2.layout = None;
        assert_eq!(run(&l2, &f).is_ok(), ok);
    }
}

/// Two different providers offering the same topic would give the widget two grant entries for
/// one (widget, topic) pair, which the daemon rejects for the whole file. The shipped catalog has
/// no such overlap, so the check is tested with a catalog that does.
#[test]
fn a_topic_claimed_by_two_providers_is_refused_with_its_own_message() {
    let cat = catalog::parse(
        br#"{"schemaVersion":0,"providers":{
        "a":{"class":"local-system","auth":"none","egress":[],"topics":{"x.y":["f"]}},
        "b":{"class":"local-system","auth":"none","egress":[],"topics":{"x.y":["f"]}}}}"#,
    )
    .unwrap();
    let mut w: Value = serde_json::from_slice(&fixture("clock", "widget.json")).unwrap();
    w["providers"] = json!([
        {"name": "a", "topics": ["x.y"], "fields": ["f"]},
        {"name": "b", "topics": ["x.y"], "fields": ["f"]}
    ]);
    let e = ow::install::validate::widget(&serde_json::to_vec(&w).unwrap(), &id_of("clock"), &cat)
        .unwrap_err();
    assert!(
        e.contains("is declared more than once") && !e.contains("listed"),
        "{e}"
    );
    // Control: one provider each is fine.
    w["providers"][1]["topics"] = json!([]);
    w["providers"] = json!([w["providers"][0].clone()]);
    assert!(
        ow::install::validate::widget(&serde_json::to_vec(&w).unwrap(), &id_of("clock"), &cat)
            .is_ok()
    );
}
