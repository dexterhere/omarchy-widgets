//! Small string rules shared by the validators. Hand-written instead of a regex engine: every
//! pattern here mirrors one in `registry/schema/widget.schema.json` or the registry build.

/// Characters that are invisible or reorder text: control, format (bidi, zero width), line and
/// paragraph separators. Rust's std has no Unicode category table, so the format characters are
/// listed by range; this is a floor, not a full `\p{Cf}`.
pub fn is_spoofing(c: char) -> bool {
    c.is_control()
        || matches!(c as u32,
            0x00ad | 0x034f | 0x061c | 0x115f | 0x1160 | 0x17b4 | 0x17b5
            | 0x180b..=0x180f | 0x200b..=0x200f | 0x2028..=0x202e | 0x2060..=0x206f
            | 0x2800 | 0x3164 | 0xfe00..=0xfe0e | 0xfeff | 0xffa0
            | 0xfff9..=0xfffb | 0xe0000..=0xe007f)
        // Private use.
        || matches!(c as u32, 0xe000..=0xf8ff | 0xf0000..=0xffffd | 0x100000..=0x10fffd)
}

/// Looks like markup: `<` then optional spaces, optional `/`, then a letter, `!` or `?`.
pub fn htmlish(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'<' {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if b.get(j) == Some(&b'/') {
                j += 1;
                while j < b.len() && b[j].is_ascii_whitespace() {
                    j += 1;
                }
            }
            if b.get(j)
                .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'!' || *c == b'?')
            {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Plain text a person can see: `plain_chars` and at least one non-space character.
pub fn plain(s: &str) -> bool {
    plain_chars(s) && s.chars().any(|c| !c.is_whitespace())
}

/// No spoofing characters, nothing markup-like, no run of four combining marks (the empty
/// string passes). NFC is not checked: no normalisation tables without a new dependency.
pub fn plain_chars(s: &str) -> bool {
    let mut marks = 0;
    for c in s.chars() {
        if is_spoofing(c) {
            return false;
        }
        marks = if matches!(c as u32, 0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff)
        {
            marks + 1
        } else {
            0
        };
        if marks >= 4 {
            return false;
        }
    }
    !htmlish(s)
}

/// Text safe to print to a terminal: plain, otherwise replaced by a fixed notice.
pub fn show(s: &str) -> String {
    if plain(s) {
        s.chars().take(120).collect()
    } else {
        "(unprintable text)".into()
    }
}

/// `show` for text that is not meant to be valid (an unknown value from the index): quoted and
/// cut short, with anything unusual replaced.
pub fn show_value(s: &str) -> String {
    let t: String = s
        .chars()
        .take(24)
        .map(|c| {
            if is_spoofing(c) || c == '"' || c == '\\' {
                '?'
            } else {
                c
            }
        })
        .collect();
    format!("\"{t}\"")
}

pub fn is_lower_alnum_dash(s: &str) -> bool {
    s.bytes()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

/// `^[a-z][a-z0-9-]*$` with a length cap.
pub fn slug(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.as_bytes()[0].is_ascii_lowercase()
        && is_lower_alnum_dash(s)
}

/// Dotted name: first segment `[a-z][a-z0-9-]*`, later ones `[a-z0-9][a-z0-9-]*`, at least two.
pub fn dotted(s: &str, max: usize) -> bool {
    if s.is_empty() || s.len() > max {
        return false;
    }
    let mut n = 0;
    for (i, seg) in s.split('.').enumerate() {
        let ok = match seg.as_bytes().first() {
            None => false,
            Some(c) if i == 0 => c.is_ascii_lowercase(),
            Some(c) => c.is_ascii_lowercase() || c.is_ascii_digit(),
        } && is_lower_alnum_dash(seg);
        if !ok {
            return false;
        }
        n += 1;
    }
    n >= 2
}

/// Widget id as the schema writes it: dotted, at most 100 characters, not under `omarchy.`.
pub fn schema_id(s: &str) -> bool {
    dotted(s, 100) && !s.starts_with("omarchy.")
}

/// Directory name for an installed widget: `^[a-z0-9.-]{1,100}$`, and also no leading dot or
/// dash and no `..`, so it can never be a hidden name, an option-looking name or a path step.
/// Used on every path join, including for ids typed by the user to `ow remove`.
pub fn store_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'.' || c == b'-')
        && !s.starts_with(['.', '-'])
        && !s.contains("..")
        && !s.ends_with('.')
}

/// `^[a-zA-Z][a-zA-Z0-9_.-]{0,39}$`.
pub fn field_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 40
        && s.as_bytes()[0].is_ascii_alphabetic()
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-'))
}

/// 40 lowercase hex digits.
pub fn full_sha(s: &str) -> bool {
    s.len() == 40
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// 64 lowercase hex digits.
pub fn sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_id_refuses_every_path_trick() {
        for ok in [
            "a",
            "io.github.dexterhere.clock",
            "a-b.c1",
            &"a".repeat(100),
        ] {
            assert!(store_id(ok), "{ok}");
        }
        let long = "a".repeat(101);
        for bad in [
            "",
            ".",
            "..",
            "../x",
            "a/b",
            "/abs",
            "a\\b",
            ".hidden",
            "-opt",
            "a..b",
            "a.",
            "A",
            "a b",
            "a\0b",
            "a\nb",
            "é",
            "a:b",
            "~",
            "a%2e",
            "a/../b",
            &long,
            "a\u{202e}b",
            ".stage-1",
            ".old-1",
            "installed.json/",
        ] {
            assert!(!store_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn schema_id_matches_the_json_schema_pattern() {
        assert!(schema_id("io.github.dexterhere.clock"));
        assert!(schema_id("a.b"));
        for bad in [
            "omarchy.x",
            "single",
            "a..b",
            "a.",
            ".a",
            "A.b",
            "1a.b",
            "a.-b",
            "a.b-",
        ] {
            // "a.b-" is fine for the schema ([a-z0-9-]* allows a trailing dash).
            if bad == "a.b-" {
                assert!(schema_id(bad));
            } else {
                assert!(!schema_id(bad), "{bad}");
            }
        }
        assert!(!schema_id(&format!("a.{}", "b".repeat(100))));
    }

    #[test]
    fn plain_text_rules() {
        assert!(plain("Clock 0.1"));
        for bad in [
            "",
            " ",
            "a\u{1b}[31m",
            "a\u{202e}b",
            "a\u{200b}b",
            "<b>x</b>",
            "a < /b",
            "x\ny",
            "a\u{0301}\u{0301}\u{0301}\u{0301}",
            "\u{e000}",
            "<!--",
        ] {
            assert!(!plain(bad), "{bad:?}");
        }
        assert!(plain("1 < 2"));
        assert_eq!(show("a\u{1b}[31m"), "(unprintable text)");
        assert!(!show_value("a\u{202e}\"b\\c").contains(['\u{202e}', '\\']));
    }

    #[test]
    fn hex_rules() {
        assert!(full_sha(&"a".repeat(40)));
        for bad in [
            &"a".repeat(39),
            &"A".repeat(40),
            &"a".repeat(41),
            &"g".repeat(40),
            "abc123",
            "",
        ] {
            assert!(!full_sha(bad), "{bad}");
        }
        assert!(sha256_hex(&"0".repeat(64)) && !sha256_hex(&"F".repeat(64)));
    }
}
