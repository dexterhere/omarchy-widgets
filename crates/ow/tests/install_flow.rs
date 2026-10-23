//! `ow install|list|remove` end to end, in-process against a throwaway tree, with the real
//! widget files behind a fake byte source. Covers consent, status gates, staging and rollback,
//! grants editing, id fuzzing, and the daemon reload against a real in-process `owd`.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod install_common;
use install_common::*;
use ow_core::Paths;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

const CLOCK: &str = "io.github.dexterhere.clock";

fn mode(p: &std::path::Path) -> u32 {
    std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o777
}

fn alpha_clock() -> Fake {
    world("clock", SHA_A, "alpha", "community", |_| {})
}

fn hash_from(out: &str) -> String {
    out.lines()
        .find_map(|l| l.strip_prefix("Access hash (for --accept-access): "))
        .unwrap()
        .to_owned()
}

#[test]
fn install_writes_private_files_a_record_and_a_grant_then_list_and_remove_undo_it() {
    let (_d, paths) = tree();
    write_grants(
        &paths,
        "# my grants\n\n[[grant]]\nwidget = \"other\"\ntopics = [\"system.cpu\"]\n",
        0o600,
    );
    let src = alpha_clock();
    let r = install(&paths, &src, &[CLOCK], &mut Script::says(&["yes"]));
    r.result.unwrap();
    assert!(
        r.out.contains("system.time") && r.out.contains("JSON only"),
        "{}",
        r.out
    );
    assert!(
        r.out
            .contains("The daemon is not running; the grants apply when it starts."),
        "{}",
        r.out
    );

    let dir = paths.widgets().join(CLOCK);
    for f in [
        "manifest.json",
        "widget.json",
        "layout.json",
        "installed.json",
    ] {
        assert_eq!(mode(&dir.join(f)), 0o600, "{f}");
    }
    assert_eq!((mode(&dir), mode(&paths.widgets())), (0o700, 0o700));
    // The bytes on disk are exactly the bytes served, and the record holds their hashes.
    assert_eq!(
        std::fs::read(dir.join("widget.json")).unwrap(),
        fixture("clock", "widget.json")
    );
    let rec = ow::install::store::read_installed(&paths, CLOCK)
        .unwrap()
        .unwrap();
    assert_eq!(rec.commit, SHA_A);
    assert_eq!(
        rec.files["layout.json"],
        ow::install::hash::sha256(&fixture("clock", "layout.json"))
    );
    assert_eq!(rec.granted[0].topics, ["system.time"]);
    // Grants: appended, comments and the other entry kept, file still 0600.
    let g = std::fs::read_to_string(paths.grants()).unwrap();
    assert!(
        g.contains("# my grants")
            && g.contains("widget = \"other\"")
            && g.contains(&format!("widget = \"{CLOCK}\""))
    );
    assert_eq!(mode(&paths.grants()), 0o600);
    let parsed = ow_core::Grants::load(&paths.grants()).unwrap();
    assert!(
        parsed.find(CLOCK, "system.time").is_some() && parsed.find("other", "system.cpu").is_some()
    );
    // The only network traffic: one index, three files at the pinned commit.
    assert_eq!(src.log().len(), 4);
    assert!(
        src.log()[1..]
            .iter()
            .all(|p| p.starts_with(&format!("/dexterhere/omarchy-widget-clock/{SHA_A}/")))
    );

    let l = list(&paths);
    assert!(
        l.contains(CLOCK) && l.contains("0.1.0") && l.contains(&SHA_A[..12]) && l.contains("alpha"),
        "{l}"
    );
    assert!(!l.contains('!'), "{l}");

    let r = remove(&paths, &[CLOCK]);
    r.result.unwrap();
    assert!(!dir.exists());
    let g = std::fs::read_to_string(paths.grants()).unwrap();
    assert!(g.contains("# my grants") && g.contains("widget = \"other\"") && !g.contains(CLOCK));
    assert_eq!(mode(&paths.grants()), 0o600);
    assert!(list(&paths).contains("No widgets installed"));
    // Idempotent.
    let again = remove(&paths, &[CLOCK]);
    again.result.unwrap();
    assert!(again.out.contains("not installed"), "{}", again.out);
    // No stage or lock debris besides the lock file.
    let names: Vec<_> = std::fs::read_dir(paths.widgets())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, [".lock"]);
}

#[test]
fn consent_is_required_and_anything_but_yes_installs_nothing() {
    for answer in [
        "", "y", "Yes", "YES", "yes ", " yes", "no", "yes\u{0}", "yess",
    ] {
        let (_d, paths) = tree();
        let r = install(
            &paths,
            &alpha_clock(),
            &[CLOCK],
            &mut Script::says(&[answer]),
        );
        r.result.unwrap();
        assert!(r.out.contains("Not installed."), "{answer:?}");
        assert!(
            !paths.data.exists() && !paths.config.exists(),
            "{answer:?}: wrote something"
        );
    }
    // No answer at all (EOF) is an error and also writes nothing.
    let (_d, paths) = tree();
    let r = install(&paths, &alpha_clock(), &[CLOCK], &mut Script::says(&[]));
    assert!(r.result.is_err());
    assert!(!paths.data.exists());
}

#[test]
fn no_terminal_means_no_install_and_no_network() {
    let (_d, paths) = tree();
    let src = alpha_clock();
    let e = install(&paths, &src, &[CLOCK], &mut Script::no_terminal())
        .result
        .unwrap_err();
    assert!(e.contains("needs a terminal"), "{e}");
    assert!(src.log().is_empty(), "contacted the source before refusing");
    assert!(!paths.data.exists());
}

#[test]
fn dry_run_prints_the_access_and_writes_nothing() {
    let (_d, paths) = tree();
    let mut ui = Script::no_terminal();
    let r = install(&paths, &alpha_clock(), &[CLOCK, "--dry-run"], &mut ui);
    r.result.unwrap();
    assert!(r.out.contains("Dry run: nothing was written") && ui.asked.is_empty());
    assert!(
        r.out
            .contains("topics system.time; fields hm, hms, date, tz"),
        "{}",
        r.out
    );
    assert_eq!(hash_from(&r.out).len(), 64);
    assert!(!paths.data.exists() && !paths.config.exists());
}

#[test]
fn yes_needs_the_exact_access_hash_and_nothing_else_bypasses_consent() {
    let (_d, paths) = tree();
    let src = alpha_clock();
    let dry = install(
        &paths,
        &src,
        &[CLOCK, "--dry-run"],
        &mut Script::no_terminal(),
    );
    let h = hash_from(&dry.out);
    for bad in [
        &[CLOCK, "--yes"][..],
        &[CLOCK, "--accept-access", &h],
        &[CLOCK, "--yes", "--accept-access", "abc"],
        &[CLOCK, "--yes", "--accept-access", &h.to_uppercase()],
        &[CLOCK, "--yes", "--accept-access", &h, "--dry-run"],
    ] {
        assert!(
            install(&paths, &src, bad, &mut Script::no_terminal())
                .result
                .is_err(),
            "{bad:?}"
        );
    }
    // A hash from another commit (different access text) is refused after the fetch, nothing written.
    let other = world("clock", SHA_B, "alpha", "community", |_| {});
    let h_b = hash_from(
        &install(
            &paths,
            &other,
            &[CLOCK, "--dry-run"],
            &mut Script::no_terminal(),
        )
        .out,
    );
    assert_ne!(h, h_b, "the commit is part of the hash");
    let e = install(
        &paths,
        &src,
        &[CLOCK, "--yes", "--accept-access", &h_b],
        &mut Script::no_terminal(),
    )
    .result
    .unwrap_err();
    assert!(e.contains("access hash"), "{e}");
    assert!(!paths.data.exists());
    // The right hash installs without a terminal.
    install(
        &paths,
        &src,
        &[CLOCK, "--yes", "--accept-access", &h],
        &mut Script::no_terminal(),
    )
    .result
    .unwrap();
    assert!(paths.widgets().join(CLOCK).join("widget.json").exists());
}

#[test]
fn status_gate_planned_needs_the_flag_removed_never_unknown_refused() {
    let (_d, paths) = tree();
    let planned = world("clock", SHA_A, "planned", "concept", |_| {});
    let e = install(
        &paths,
        &planned,
        &[CLOCK, "--dry-run"],
        &mut Script::no_terminal(),
    )
    .result
    .unwrap_err();
    assert!(
        e.contains("--allow-planned") && e.contains("will not show data"),
        "{e}"
    );
    let ok = install(
        &paths,
        &planned,
        &[CLOCK, "--allow-planned", "--dry-run"],
        &mut Script::no_terminal(),
    );
    ok.result.unwrap();
    assert!(
        ok.out.contains("only planned")
            && ok
                .out
                .contains("will not show data until the needed provider exists"),
        "{}",
        ok.out
    );
    // system.time is not served by the daemon yet: the summary says so.
    assert!(
        ok.out.contains("does not serve yet: system.time.date"),
        "{}",
        ok.out
    );
    for (st, tier) in [
        ("removed", "community"),
        ("beta", "community"),
        ("alpha", "official"),
        ("", "spec"),
    ] {
        let w = world("clock", SHA_A, st, tier, |_| {});
        assert!(
            install(
                &paths,
                &w,
                &[CLOCK, "--allow-planned", "--dry-run"],
                &mut Script::no_terminal()
            )
            .result
            .is_err(),
            "{st}/{tier}"
        );
    }
    assert!(!paths.data.exists());
    // The flag does not change what is written for a real install of a planned widget.
    let r = install(
        &paths,
        &planned,
        &[CLOCK, "--allow-planned"],
        &mut Script::says(&["yes"]),
    );
    r.result.unwrap();
    assert!(
        r.out.contains("Reminder: this widget is only planned"),
        "{}",
        r.out
    );
    assert!(list(&paths).contains("planned"));
}

#[test]
fn every_refusal_leaves_the_tree_untouched() {
    let cases: Vec<(&str, Fake)> = vec![
        (
            "hash",
            world("clock", SHA_A, "alpha", "spec", |e| {
                e["sha256"] = serde_json::json!({"manifest.json": "0".repeat(64), "widget.json": "0".repeat(64), "layout.json": "0".repeat(64)});
            }),
        ),
        (
            "short commit",
            world("clock", "0123456", "alpha", "spec", |_| {}),
        ),
        (
            "placeholder commit",
            world("clock", &"0".repeat(40), "alpha", "spec", |_| {}),
        ),
        ("404", {
            let w = alpha_clock();
            Fake(Arc::new(Inner {
                index: w.0.index.clone(),
                files: Default::default(),
                log: Default::default(),
            }))
        }),
        (
            "network declared",
            with_file(&alpha_clock(), "clock", SHA_A, "widget.json", {
                let mut w: serde_json::Value =
                    serde_json::from_slice(&fixture("clock", "widget.json")).unwrap();
                w["capabilities"]["network"] = serde_json::json!(["evil.example.com"]);
                serde_json::to_vec(&w).unwrap()
            }),
        ),
        (
            "id mismatch",
            with_file(&alpha_clock(), "clock", SHA_A, "manifest.json", {
                let mut w: serde_json::Value =
                    serde_json::from_slice(&fixture("clock", "manifest.json")).unwrap();
                w["id"] = serde_json::json!("io.github.dexterhere.battery");
                serde_json::to_vec(&w).unwrap()
            }),
        ),
        (
            "oversize",
            with_file(
                &alpha_clock(),
                "clock",
                SHA_A,
                "layout.json",
                vec![b' '; 300 * 1024],
            ),
        ),
        (
            "duplicate key",
            with_file(&alpha_clock(), "clock", SHA_A, "widget.json", {
                String::from_utf8(fixture("clock", "widget.json"))
                    .unwrap()
                    .replacen('{', "{\"tier\":\"spec\",", 1)
                    .into_bytes()
            }),
        ),
    ];
    for (name, src) in cases {
        let (_d, paths) = tree();
        write_grants(
            &paths,
            "[[grant]]\nwidget = \"other\"\ntopics = [\"system.cpu\"]\n",
            0o600,
        );
        let before = snapshot(_d.path());
        let r = install(&paths, &src, &[CLOCK], &mut Script::says(&["yes"]));
        assert!(r.result.is_err(), "{name} was accepted");
        assert_eq!(before, snapshot(_d.path()), "{name} changed the tree");
    }
}

#[test]
fn an_unreadable_or_loose_grants_file_stops_the_install_before_any_write() {
    for (text, mode_bits) in [
        ("not toml [", 0o600),
        ("[[grant]]\nwidget = \"o\"\ntopics = [\"a.b\"]\n", 0o644),
        ("[[grant]]\nwidget = \"o\"\ntopics = [\"a.b\"]\n", 0o666),
    ] {
        let (_d, paths) = tree();
        write_grants(&paths, text, mode_bits);
        let before = snapshot(_d.path());
        let r = install(
            &paths,
            &alpha_clock(),
            &[CLOCK],
            &mut Script::says(&["yes"]),
        );
        assert!(r.result.is_err(), "{mode_bits:o}");
        assert_eq!(before, snapshot(_d.path()), "{mode_bits:o}");
        assert_eq!(
            mode(&paths.grants()),
            mode_bits,
            "the grants file mode must not be 'fixed' silently"
        );
    }
    // A symlinked grants file is refused as the daemon refuses it.
    let (_d, paths) = tree();
    write_grants(&paths, "", 0o600);
    std::fs::rename(paths.grants(), paths.config.join("real.toml")).unwrap();
    std::os::unix::fs::symlink(paths.config.join("real.toml"), paths.grants()).unwrap();
    assert!(
        install(
            &paths,
            &alpha_clock(),
            &[CLOCK],
            &mut Script::says(&["yes"])
        )
        .result
        .is_err()
    );
    assert!(!paths.widgets().join(CLOCK).exists());
}

#[test]
fn a_failed_grants_write_rolls_back_to_the_previous_install() {
    if rustix::process::geteuid().is_root() {
        return; // read-only directories do not stop root
    }
    let (_d, paths) = tree();
    install(
        &paths,
        &alpha_clock(),
        &[CLOCK],
        &mut Script::says(&["yes"]),
    )
    .result
    .unwrap();
    let before = snapshot(_d.path());
    // The config directory becomes read-only: the new grants cannot be written.
    std::fs::set_permissions(&paths.config, std::fs::Permissions::from_mode(0o500)).unwrap();
    let next = world("clock", SHA_B, "alpha", "community", |_| {});
    let r = install(&paths, &next, &[CLOCK], &mut Script::says(&["yes"]));
    std::fs::set_permissions(&paths.config, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(r.result.is_err());
    let after = snapshot(_d.path());
    assert_eq!(
        before
            .iter()
            .filter(|(k, _)| !k.starts_with("config"))
            .count(),
        after
            .iter()
            .filter(|(k, _)| !k.starts_with("config"))
            .count()
    );
    assert_eq!(
        before, after,
        "rollback must restore the old install and grants exactly"
    );
    assert_eq!(
        ow::install::store::read_installed(&paths, CLOCK)
            .unwrap()
            .unwrap()
            .commit,
        SHA_A
    );
}

#[test]
fn a_first_install_that_fails_at_the_grants_step_leaves_no_widget_directory() {
    if rustix::process::geteuid().is_root() {
        return;
    }
    let (_d, paths) = tree();
    write_grants(&paths, "", 0o600);
    std::fs::set_permissions(&paths.config, std::fs::Permissions::from_mode(0o500)).unwrap();
    let r = install(
        &paths,
        &alpha_clock(),
        &[CLOCK],
        &mut Script::says(&["yes"]),
    );
    std::fs::set_permissions(&paths.config, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(r.result.is_err());
    let names: Vec<_> = std::fs::read_dir(paths.widgets())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, [".lock"], "partial install left behind");
}

#[test]
fn updating_to_a_new_commit_replaces_files_and_the_single_grant_entry() {
    let (_d, paths) = tree();
    install(
        &paths,
        &alpha_clock(),
        &[CLOCK],
        &mut Script::says(&["yes"]),
    )
    .result
    .unwrap();
    // The new commit also reads a battery topic.
    let mut widget: serde_json::Value =
        serde_json::from_slice(&fixture("clock", "widget.json")).unwrap();
    let extra = serde_json::json!({"name": "battery", "topics": ["battery.status"], "fields": ["percent"], "auth": "none"});
    widget["providers"]
        .as_array_mut()
        .unwrap()
        .push(extra.clone());
    let next = world("clock", SHA_B, "alpha", "community", |e| {
        e["providers"] = widget["providers"].clone()
    });
    let next = with_file(
        &next,
        "clock",
        SHA_B,
        "widget.json",
        serde_json::to_vec(&widget).unwrap(),
    );
    let mut ui = Script::says(&["yes"]);
    let r = install(&paths, &next, &[CLOCK], &mut ui);
    r.result.unwrap();
    assert!(
        r.out.contains(&format!(
            "UPDATE of an existing install: commit {SHA_A} -> {SHA_B}"
        )),
        "{}",
        r.out
    );
    assert!(
        r.out
            .contains("The update asks for NEW topics: battery.status"),
        "{}",
        r.out
    );
    let rec = ow::install::store::read_installed(&paths, CLOCK)
        .unwrap()
        .unwrap();
    assert_eq!(rec.commit, SHA_B);
    let g = ow_core::Grants::load(&paths.grants()).unwrap();
    assert_eq!(
        g.entries().iter().filter(|e| e.widget == CLOCK).count(),
        2,
        "one entry per provider, none left over from the old commit"
    );
    assert!(g.find(CLOCK, "system.time").is_some() && g.find(CLOCK, "battery.status").is_some());
    // Same commit again is a reinstall, said so.
    let r = install(&paths, &next, &[CLOCK], &mut Script::says(&["yes"]));
    assert!(r.out.contains("REINSTALL"), "{}", r.out);
}

#[test]
fn a_changed_file_after_install_is_flagged_by_list_and_a_missing_grant_too() {
    let (_d, paths) = tree();
    install(
        &paths,
        &alpha_clock(),
        &[CLOCK],
        &mut Script::says(&["yes"]),
    )
    .result
    .unwrap();
    let w = paths.widgets().join(CLOCK).join("widget.json");
    std::fs::write(&w, b"{}").unwrap();
    std::fs::write(paths.grants(), "").unwrap();
    let l = list(&paths);
    assert!(
        l.contains("changed or missing since install: widget.json"),
        "{l}"
    );
    assert!(l.contains("no grant"), "{l}");
    std::fs::remove_file(paths.widgets().join(CLOCK).join("installed.json")).unwrap();
    assert!(list(&paths).contains("install record missing or unreadable"));
}

#[test]
fn list_and_remove_survive_hostile_debris_in_the_widgets_dir() {
    let (_d, paths) = tree();
    install(
        &paths,
        &alpha_clock(),
        &[CLOCK],
        &mut Script::says(&["yes"]),
    )
    .result
    .unwrap();
    let w = paths.widgets();
    std::fs::write(w.join("stray-file"), "x").unwrap();
    std::fs::create_dir(w.join("UPPER")).unwrap();
    std::fs::create_dir(w.join("..hidden")).unwrap_or(());
    std::fs::create_dir(w.join(".stage-leftover")).unwrap();
    std::fs::write(w.join(".stage-leftover").join("x"), "x").unwrap();
    let l = list(&paths);
    assert!(
        l.contains(CLOCK)
            && !l.contains("stray")
            && !l.contains("UPPER")
            && !l.contains("leftover"),
        "{l}"
    );
    // A record with terminal escapes is printed harmlessly.
    let rec = w.join(CLOCK).join("installed.json");
    let t = std::fs::read_to_string(&rec)
        .unwrap()
        .replace("\"alpha\"", "\"\\u001b[31mALERT\"");
    std::fs::write(&rec, t).unwrap();
    assert!(!list(&paths).contains('\u{1b}'));
    // The next install or remove sweeps the leftover stage.
    remove(&paths, &["io.github.nobody.x"]).result.unwrap();
    assert!(!w.join(".stage-leftover").exists());
}

#[test]
fn hostile_ids_never_touch_anything_outside_the_widget_dir() {
    let (d, paths) = tree();
    install(
        &paths,
        &alpha_clock(),
        &[CLOCK],
        &mut Script::says(&["yes"]),
    )
    .result
    .unwrap();
    // Canaries beside and above the widgets dir.
    let canary_dir = paths.data.join("victim");
    std::fs::create_dir(&canary_dir).unwrap();
    std::fs::write(canary_dir.join("keep"), "keep").unwrap();
    std::fs::write(paths.data.join("vault.owv"), "vault").unwrap();
    std::fs::write(d.path().join("outside"), "outside").unwrap();
    let before = snapshot(d.path());
    let hostile = [
        "..",
        ".",
        "../victim",
        "../../victim",
        "../vault.owv",
        "/etc/passwd",
        "../../../outside",
        "victim/..",
        "a/../../victim",
        "",
        " ",
        "\n",
        "..\\victim",
        "~",
        "$HOME",
        "io.github.dexterhere.clock/../../victim",
        "io.github.dexterhere.clock/",
        "%2e%2e/victim",
        "..%2fvictim",
        ".lock",
        ".stage-1",
        ".old-1",
        "-rf",
        "--yes",
        "installed.json",
        "A",
        "a\u{0}b",
        "\u{202e}",
        &"a".repeat(101),
    ];
    // Deterministic pseudo-random strings over the characters that matter.
    let alphabet: Vec<char> = "ab.-/\\%~ \n\u{0}".chars().collect();
    let mut x = 0x2545_f491_4f6c_dd1du64;
    let mut random = Vec::new();
    for _ in 0..600 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let len = 1 + (x % 12) as usize;
        let mut s = String::new();
        let mut y = x;
        for _ in 0..len {
            y = y
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            s.push(alphabet[(y >> 33) as usize % alphabet.len()]);
        }
        random.push(s);
    }
    for id in hostile.iter().map(|s| (*s).to_owned()).chain(random) {
        let valid = ow::install::text::store_id(&id);
        let r = remove(&paths, &[&id]);
        if !valid {
            assert!(r.result.is_err(), "{id:?} should be refused");
        }
        let i = install(
            &paths,
            &alpha_clock(),
            &[&id, "--dry-run"],
            &mut Script::no_terminal(),
        );
        if !valid {
            assert!(i.result.is_err(), "{id:?} should be refused by install");
        }
    }
    // Everything outside widgets/<id> is exactly as before; the real widget was never removed
    // by a near-miss id.
    let after = snapshot(d.path());
    for (k, v) in &before {
        if !k.starts_with("data/owd/widgets") {
            assert_eq!(after.get(k), Some(v), "{k:?} changed");
        }
    }
    assert!(paths.widgets().join(CLOCK).join("widget.json").exists());
}

#[test]
fn a_symlink_in_place_of_the_widget_dir_is_unlinked_never_followed() {
    let (d, paths) = tree();
    std::fs::create_dir_all(&paths.data).unwrap();
    std::fs::set_permissions(&paths.data, std::fs::Permissions::from_mode(0o700)).unwrap();
    let outside = d.path().join("precious");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("file"), "precious").unwrap();
    std::fs::create_dir_all(paths.widgets()).unwrap();
    std::fs::set_permissions(paths.widgets(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::os::unix::fs::symlink(&outside, paths.widgets().join(CLOCK)).unwrap();
    remove(&paths, &[CLOCK]).result.unwrap();
    assert!(!paths.widgets().join(CLOCK).exists());
    assert_eq!(std::fs::read(outside.join("file")).unwrap(), b"precious");
    // Installing over a symlink replaces the link, and writes nothing through it.
    std::os::unix::fs::symlink(&outside, paths.widgets().join(CLOCK)).unwrap();
    install(
        &paths,
        &alpha_clock(),
        &[CLOCK],
        &mut Script::says(&["yes"]),
    )
    .result
    .unwrap();
    assert!(std::fs::read_dir(&outside).unwrap().count() == 1);
    assert!(paths.widgets().join(CLOCK).join("widget.json").is_file());
    // A symlinked widgets directory itself is refused.
    let (d2, p2) = tree();
    std::fs::create_dir_all(&p2.data).unwrap();
    std::fs::set_permissions(&p2.data, std::fs::Permissions::from_mode(0o700)).unwrap();
    let elsewhere = d2.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, p2.widgets()).unwrap();
    assert!(
        install(&p2, &alpha_clock(), &[CLOCK], &mut Script::says(&["yes"]))
            .result
            .is_err()
    );
    assert!(std::fs::read_dir(&elsewhere).unwrap().next().is_none());
}

#[test]
fn widget_files_are_data_only_hostile_text_is_stored_but_never_run_or_printed_raw() {
    let marker = std::env::temp_dir().join(format!("ow-pwned-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let (_d, paths) = tree();
    let evil = format!("$(touch {m})", m = marker.display());
    let mut m: serde_json::Value =
        serde_json::from_slice(&fixture("clock", "manifest.json")).unwrap();
    m["name"] = serde_json::json!(evil);
    m["entry"] = serde_json::json!(format!("; touch {}", marker.display()));
    let src = with_file(
        &alpha_clock(),
        "clock",
        SHA_A,
        "manifest.json",
        serde_json::to_vec(&m).unwrap(),
    );
    let src = Fake(Arc::new(Inner {
        index: {
            let mut idx: serde_json::Value = serde_json::from_slice(&src.0.index).unwrap();
            for w in idx["widgets"].as_array_mut().unwrap() {
                if w["id"] == CLOCK {
                    w["name"] = serde_json::json!(evil);
                }
            }
            serde_json::to_vec(&idx).unwrap()
        },
        files: src.0.files.clone(),
        log: Default::default(),
    }));
    let r = install(&paths, &src, &[CLOCK], &mut Script::says(&["yes"]));
    r.result.unwrap();
    assert!(!marker.exists(), "something executed widget text");
    assert!(
        std::fs::read_to_string(paths.widgets().join(CLOCK).join("manifest.json"))
            .unwrap()
            .contains("touch")
    );
}

#[test]
fn other_widgets_grants_and_a_lookalike_id_survive_an_install_and_a_remove() {
    let (_d, paths) = tree();
    let existing = format!(
        "[[grant]]\nwidget = \"{CLOCK}x\"\ntopics = [\"system.cpu\"]\n\n[[grant]]\nwidget = \"x{CLOCK}\"\ntopics = [\"system.load\"]\nfields = [\"load1\"]\n"
    );
    write_grants(&paths, &existing, 0o600);
    install(
        &paths,
        &alpha_clock(),
        &[CLOCK],
        &mut Script::says(&["yes"]),
    )
    .result
    .unwrap();
    remove(&paths, &[CLOCK]).result.unwrap();
    assert_eq!(
        std::fs::read_to_string(paths.grants()).unwrap().trim_end(),
        existing.trim_end()
    );
}

#[test]
fn two_installs_cannot_run_at_once() {
    let (_d, paths) = tree();
    let held = ow::install::store::Lock::acquire(&paths).unwrap();
    let e = install(
        &paths,
        &alpha_clock(),
        &[CLOCK],
        &mut Script::says(&["yes"]),
    )
    .result
    .unwrap_err();
    assert!(e.contains("another ow install or remove is running"), "{e}");
    assert!(
        remove(&paths, &[CLOCK])
            .result
            .unwrap_err()
            .contains("another ow")
    );
    drop(held);
    install(
        &paths,
        &alpha_clock(),
        &[CLOCK],
        &mut Script::says(&["yes"]),
    )
    .result
    .unwrap();
}

#[test]
fn a_running_daemon_picks_the_grant_up_and_drops_it_again_without_a_signal() {
    use ow_providers::System;
    let (_d, paths) = tree();
    let server = owd::Server::bind(&paths, vec![Arc::new(System::new())]).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (s2, r2) = (Arc::clone(&stop), Arc::new(AtomicBool::new(false)));
    let th = std::thread::spawn(move || server.run(&s2, &r2));
    let id = "io.github.dexterhere.system-monitor";
    let snap = |topic: &str| {
        ow::daemon::call(&paths, &format!("{{\"v\":0,\"id\":1,\"op\":\"snapshot\",\"widget\":\"{id}\",\"topic\":\"{topic}\"}}\n")).unwrap()
    };
    assert!(snap("system.cpu").contains("denied"));
    let src = world("system-monitor", SHA_A, "alpha", "community", |_| {});
    let r = install(&paths, &src, &[id], &mut Script::says(&["yes"]));
    r.result.unwrap();
    assert!(
        r.out.contains("The running daemon reloaded the grants."),
        "{}",
        r.out
    );
    let reply = snap("system.cpu");
    assert!(
        reply.contains("\"ok\":true") && reply.contains("usage_pct"),
        "{reply}"
    );
    // A topic the widget did not declare stays denied.
    assert!(snap("system.nope").contains("denied"));
    let r = remove(&paths, &[id]);
    r.result.unwrap();
    assert!(
        r.out.contains("The running daemon reloaded the grants."),
        "{}",
        r.out
    );
    assert!(snap("system.cpu").contains("denied"));
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    th.join().unwrap();
}

#[test]
fn the_vault_and_audit_paths_are_never_touched() {
    let (d, paths) = tree();
    std::fs::create_dir_all(&paths.state).unwrap();
    std::fs::write(paths.audit(), "audit").unwrap();
    install(
        &paths,
        &alpha_clock(),
        &[CLOCK],
        &mut Script::says(&["yes"]),
    )
    .result
    .unwrap();
    remove(&paths, &[CLOCK]).result.unwrap();
    assert_eq!(std::fs::read(paths.audit()).unwrap(), b"audit");
    assert!(!paths.vault().exists());
    let _ = Paths::under(d.path());
}

#[test]
fn the_summary_says_where_the_credential_goes_and_what_the_widget_never_gets() {
    let (_d, paths) = tree();
    let id = "io.github.dexterhere.github-activity";
    let src = world("github-activity", SHA_A, "alpha", "community", |_| {});
    let r = install(&paths, &src, &[id, "--dry-run"], &mut Script::no_terminal());
    r.result.unwrap();
    for want in [
        "provider github: topics github.prs; fields open, review",
        "uses a credential from your vault",
        "The daemon, not the widget, contacts api.github.com",
        "The widget never receives the credential",
        "claims",
        "not a review of the widget by a person",
        "registry index carries no file hashes",
    ] {
        assert!(r.out.contains(want), "{want}\n{}", r.out);
    }
    // A widget without a vault provider says no credential is involved.
    let clock = install(
        &paths,
        &alpha_clock(),
        &[CLOCK, "--dry-run"],
        &mut Script::no_terminal(),
    );
    assert!(
        clock.out.contains("Credentials: none involved."),
        "{}",
        clock.out
    );
    // A non-default registry is called out as moving the trust root.
    let mut out = Vec::new();
    let s = alpha_clock();
    ow::install::install(
        &paths,
        &args(&[
            CLOCK,
            "--dry-run",
            "--registry",
            "https://registry.example.org",
        ]),
        &mut out,
        &move |host| {
            assert_eq!(host, "registry.example.org");
            Box::new(s.clone())
        },
        &mut Script::no_terminal(),
    )
    .unwrap();
    assert!(
        String::from_utf8(out)
            .unwrap()
            .contains("NOT the default registry")
    );
}
