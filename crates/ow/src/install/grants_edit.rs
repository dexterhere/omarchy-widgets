//! Editing `grants.toml` as text so comments and other entries survive, then proving the result
//! with the daemon's own parser. If the text edit does not produce exactly the intended entries,
//! the file is regenerated from the parsed entries (comments lost, nothing else). Pure functions.
use ow_core::{Grant, Grants};

/// One `[[grant]]` entry for the widget: a provider's topics and the fields it may read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub topics: Vec<String>,
    pub fields: Vec<String>,
}

/// TOML basic string: quotes, backslash and all control characters escaped.
pub fn quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if c.is_control() => o.push_str(&format!("\\u{:04X}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn list(items: &[String]) -> String {
    format!(
        "[{}]",
        items
            .iter()
            .map(|s| quote(s))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn block(g: &Grant, note: Option<&str>) -> String {
    let mut s = String::new();
    if let Some(n) = note {
        s.push_str(&format!("# {n}\n"));
    }
    s.push_str(&format!(
        "[[grant]]\nwidget = {}\ntopics = {}\n",
        quote(&g.widget),
        list(&g.topics)
    ));
    if let Some(f) = &g.fields {
        s.push_str(&format!("fields = {}\n", list(f)));
    }
    if let Some(a) = &g.approved_at {
        s.push_str(&format!("approved_at = {}\n", quote(a)));
    }
    s
}

fn marker(id: &str) -> String {
    format!("added by `ow install` for {id}; `ow remove {id}` takes it out")
}

fn is_header(line: &str) -> bool {
    line.trim_start().starts_with('[')
}

fn is_grant_header(line: &str) -> bool {
    let t = line.trim();
    t.strip_prefix("[[grant]]")
        .is_some_and(|r| r.trim().is_empty() || r.trim_start().starts_with('#'))
}

fn is_comment_or_blank(line: &str) -> bool {
    let t = line.trim();
    t.is_empty() || t.starts_with('#')
}

/// Remove every `[[grant]]` block whose widget is `id`, with the comment lines directly above it.
fn remove_blocks(old: &str, id: &str) -> String {
    let lines: Vec<&str> = old.split_inclusive('\n').collect();
    let mut drop = vec![false; lines.len()];
    let mut i = 0;
    while i < lines.len() {
        if !is_grant_header(lines[i]) {
            i += 1;
            continue;
        }
        let mut end = i + 1;
        while end < lines.len() && !is_header(lines[end]) {
            end += 1;
        }
        // Trailing comments and blanks belong to whatever comes next.
        let mut body_end = end;
        while body_end > i + 1 && is_comment_or_blank(lines[body_end - 1]) {
            body_end -= 1;
        }
        let text: String = lines[i..body_end].concat();
        let ours = Grants::parse(&text).is_ok_and(|g| g.entries().iter().any(|e| e.widget == id));
        if ours {
            let mut start = i;
            while start > 0 && lines[start - 1].trim_start().starts_with('#') {
                start -= 1;
            }
            drop[start..body_end].fill(true);
        }
        i = end;
    }
    lines
        .iter()
        .zip(drop)
        .filter(|(_, d)| !d)
        .map(|(l, _)| *l)
        .collect()
}

fn sorted(mut v: Vec<Grant>) -> Vec<Grant> {
    v.sort_by(|a, b| (&a.widget, &a.topics).cmp(&(&b.widget, &b.topics)));
    v
}

/// New file text with `id`'s entries replaced by `add` (empty `add` = removal). `old` must
/// already parse. Errors only if `old` is invalid or the final text cannot be proven right.
pub fn edit(old: &str, id: &str, add: &[Entry], approved_at: &str) -> Result<String, String> {
    let before = Grants::parse(old)
        .map_err(|_| "the existing grants file is not valid; fix or remove it first")?;
    let new_grants: Vec<Grant> = add
        .iter()
        .map(|e| Grant {
            widget: id.to_owned(),
            topics: e.topics.clone(),
            fields: Some(e.fields.clone()),
            approved_at: Some(approved_at.to_owned()),
        })
        .collect();
    let want = sorted(
        before
            .entries()
            .iter()
            .filter(|g| g.widget != id)
            .cloned()
            .chain(new_grants.iter().cloned())
            .collect(),
    );

    let mut text = remove_blocks(old, id);
    if !new_grants.is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        for (n, g) in new_grants.iter().enumerate() {
            if !text.is_empty() && !text.ends_with("\n\n") {
                text.push('\n');
            }
            text.push_str(&block(g, (n == 0).then(|| marker(id)).as_deref()));
        }
    }
    let proven = Grants::parse(&text).is_ok_and(|g| sorted(g.entries().to_vec()) == want);
    if proven {
        return Ok(text);
    }
    // The textual edit did not give exactly the intended entries (odd formatting in the
    // existing file): regenerate from parsed data, which is right by construction.
    let mut out =
        String::from("# Rewritten by `ow`; comments from the previous file were not kept.\n");
    for g in &want {
        out.push('\n');
        out.push_str(&block(g, None));
    }
    if Grants::parse(&out).is_ok_and(|g| sorted(g.entries().to_vec()) == want) {
        Ok(out)
    } else {
        Err(
            "could not produce a grants file that the daemon's parser reads back identically"
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: &str = "2026-10-23T12:00:00Z";
    const ID: &str = "io.github.a.clock";

    fn e(t: &[&str], f: &[&str]) -> Entry {
        Entry {
            topics: t.iter().map(|s| (*s).into()).collect(),
            fields: f.iter().map(|s| (*s).into()).collect(),
        }
    }

    fn widgets(text: &str) -> Vec<(String, Vec<String>)> {
        Grants::parse(text)
            .unwrap()
            .entries()
            .iter()
            .map(|g| (g.widget.clone(), g.topics.clone()))
            .collect()
    }

    const HAND: &str = "# my grants, keep this comment\n\n# the cpu widget\n[[grant]]\nwidget = \"other\"\ntopics = [\"system.cpu\"]\nfields = [\"usage_pct\"]\n# trailing note\n";

    #[test]
    fn adds_to_an_empty_or_missing_file() {
        let t = edit("", ID, &[e(&["system.time"], &["hm", "date"])], AT).unwrap();
        assert!(
            t.contains("[[grant]]") && t.contains(&format!("widget = \"{ID}\"")) && t.contains(AT)
        );
        let g = Grants::parse(&t).unwrap();
        assert_eq!(
            g.find(ID, "system.time").unwrap().fields.as_deref(),
            Some(&["hm".to_owned(), "date".to_owned()][..])
        );
    }

    #[test]
    fn keeps_other_entries_and_comments() {
        let t = edit(HAND, ID, &[e(&["system.time"], &["hm"])], AT).unwrap();
        for keep in [
            "# my grants, keep this comment",
            "# the cpu widget",
            "# trailing note",
            "widget = \"other\"",
        ] {
            assert!(t.contains(keep), "{keep}\n{t}");
        }
        assert_eq!(widgets(&t).len(), 2);
    }

    #[test]
    fn replaces_only_this_widgets_entries_and_is_idempotent() {
        let once = edit(HAND, ID, &[e(&["system.time"], &["hm"])], AT).unwrap();
        let again = edit(&once, ID, &[e(&["system.time"], &["hm"])], AT).unwrap();
        assert_eq!(once, again);
        let changed = edit(&once, ID, &[e(&["system.cpu"], &["usage_pct"])], AT).unwrap();
        assert!(
            changed.contains("# my grants, keep this comment") && !changed.contains("system.time")
        );
        assert_eq!(widgets(&changed).iter().filter(|(w, _)| w == ID).count(), 1);
        // The marker comment of the removed block went with it.
        assert_eq!(changed.matches("added by `ow install`").count(), 1);
    }

    #[test]
    fn removal_restores_the_original_text_for_a_clean_file() {
        let added = edit(HAND, ID, &[e(&["system.time"], &["hm"])], AT).unwrap();
        let removed = edit(&added, ID, &[], AT).unwrap();
        assert_eq!(removed.trim_end(), HAND.trim_end());
        // Removing something that is not there changes nothing semantically.
        assert_eq!(
            widgets(&edit(HAND, "nobody", &[], AT).unwrap()),
            widgets(HAND)
        );
    }

    #[test]
    fn several_provider_entries_for_one_widget() {
        let t = edit(
            "",
            ID,
            &[e(&["system.time"], &["hm"]), e(&["github.prs"], &["open"])],
            AT,
        )
        .unwrap();
        assert_eq!(widgets(&t).len(), 2);
        let none = edit(&t, ID, &[], AT).unwrap();
        assert!(widgets(&none).is_empty());
    }

    #[test]
    fn another_widgets_entry_with_a_similar_id_is_never_touched() {
        let other = format!("[[grant]]\nwidget = \"{ID}x\"\ntopics = [\"system.cpu\"]\n");
        let t = edit(&other, ID, &[e(&["system.time"], &["hm"])], AT).unwrap();
        assert!(t.contains(&format!("widget = \"{ID}x\"")));
        let r = edit(&t, ID, &[], AT).unwrap();
        assert_eq!(
            widgets(&r),
            vec![(format!("{ID}x"), vec!["system.cpu".to_owned()])]
        );
    }

    #[test]
    fn odd_formatting_falls_back_to_a_full_rewrite_without_losing_entries() {
        // Inline tables, no header spacing, a multi-line array: the text splitter may not cope,
        // the proof step must still hold.
        let odd = "grant = [ { widget = \"o1\", topics = [\"system.cpu\"] }, { widget = \"o2\", topics = [\"system.load\"], fields = [\"load1\"] } ]\n";
        let t = edit(odd, ID, &[e(&["system.time"], &["hm"])], AT).unwrap();
        let w = widgets(&t);
        assert!(
            w.contains(&("o1".into(), vec!["system.cpu".into()]))
                && w.contains(&("o2".into(), vec!["system.load".into()]))
        );
        assert!(w.iter().any(|(x, _)| x == ID));
        let multi = "[[grant]]\nwidget = \"o\"\ntopics = [\n  \"system.cpu\",\n  \"system.load\",\n]\n[[grant]]\nwidget = \"ow.x\"\ntopics = [\"a.b\"]\n";
        let t = edit(multi, "ow.x", &[], AT).unwrap();
        assert_eq!(
            widgets(&t),
            vec![("o".into(), vec!["system.cpu".into(), "system.load".into()])]
        );
    }

    #[test]
    fn an_invalid_existing_file_is_never_rewritten() {
        for bad in [
            "not toml [",
            "[[grant]]\nwidget=\"w\"\ntopcs=[\"t\"]\n",
            "[[grant]]\nwidget=\"w\"\ntopics=[\"t\"]\n[[grant]]\nwidget=\"w\"\ntopics=[\"t\"]\n",
        ] {
            assert!(
                edit(bad, ID, &[e(&["a.b"], &[])], AT)
                    .unwrap_err()
                    .contains("not valid")
            );
        }
    }

    #[test]
    fn hostile_strings_cannot_break_out_of_their_quotes() {
        let evil = "x\"\n[[grant]]\nwidget = \"victim\"\ntopics = [\"y.z\"]\n#\\";
        let t = edit("", "ow.x", &[e(&["a.b"], &[evil])], AT).unwrap();
        let g = Grants::parse(&t).unwrap();
        assert_eq!(g.entries().len(), 1, "{t}");
        assert_eq!(g.entries()[0].fields.as_ref().unwrap()[0], evil);
        assert_eq!(quote("a\u{7}\u{1b}"), "\"a\\u0007\\u001B\"");
    }

    #[test]
    fn a_comment_that_looks_like_a_block_is_left_alone() {
        let text = format!(
            "# [[grant]] widget = \"{ID}\"\n[[grant]]\nwidget = \"o\"\ntopics = [\"system.cpu\"]\n"
        );
        let t = edit(&text, ID, &[], AT).unwrap();
        assert!(t.contains("widget = \"o\""));
    }
}
