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
            | 0xfff9..=0xfffb | 0xe0000..=0xe007f
            // Cf characters outside the blocks above (parity with the registry's SPOOF class).
            | 0x0600..=0x0605 | 0x06dd | 0x070f | 0x0890..=0x0891 | 0x08e2 | 0x110bd | 0x110cd
            | 0x13430..=0x1343f | 0x1bca0..=0x1bca3 | 0x1d173..=0x1d17a)
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

/// Length in UTF-16 code units, which is what the website's JavaScript counts.
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Typographic punctuation allowed besides ASCII.
const TYPOGRAPHIC: &str = "\u{2018}\u{2019}\u{201c}\u{201d}\u{2013}\u{2014}\u{2026}\u{00b7}\u{2022}\u{00a9}\u{00ae}\u{2122}\u{00b0}\u{20ac}\u{00a3}\u{00a5}\u{00a0}";

/// Combining marks and signs of scripts that cannot be written without them (no Unicode
/// category table in std, so by block; may include unassigned points inside these blocks).
fn is_mark_block(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036f | 0x0900..=0x0dff | 0x0e31..=0x0e3a | 0x0e47..=0x0e4e | 0x3099..=0x309a)
}

/// Strict allow-list for names and descriptions shown at the consent prompt: `plain_chars`,
/// and every character a letter or digit (`char::is_alphanumeric`), ASCII printable, one of a
/// few typographic marks, or a combining mark of an Indic or similar script. This also rejects
/// unassigned code points outside those blocks without a Unicode table; emoji and other
/// symbols are refused (the website accepts them).
pub fn printable(s: &str) -> bool {
    plain(s)
        && s.chars().all(|c| {
            c.is_alphanumeric()
                || (' '..='~').contains(&c)
                || TYPOGRAPHIC.contains(c)
                || is_mark_block(c)
        })
}

/// Text safe to print to a terminal: plain, otherwise replaced by a fixed notice.
pub fn show(s: &str) -> String {
    if printable(s) {
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
        // Cf characters outside the common blocks.
        for c in [
            '\u{0600}',
            '\u{06dd}',
            '\u{070f}',
            '\u{0890}',
            '\u{08e2}',
            '\u{110bd}',
            '\u{13430}',
            '\u{1bca0}',
            '\u{1d173}',
            '\u{e0001}',
        ] {
            assert!(!plain(&format!("a{c}b")), "{c:?}");
        }
        assert!(!printable("Clock\u{0600}\u{06dd}\u{0378}\u{1d17}3x"));
        assert!(!printable("a\u{0378}b") && !printable("smile \u{1f600}"));
        for ok in [
            "Clock",
            "\u{6642}\u{8a08}\u{30a6}\u{30a3}\u{30b8}\u{30a7}\u{30c3}\u{30c8}",
            "\u{0918}\u{0921}\u{0940} \u{0935}\u{093f}\u{091c}\u{0947}\u{091f} \u{0915}\u{094d}\u{0932}\u{0915}",
            "Caf\u{e9} \u{2014} it\u{2019}s 9\u{b0}",
        ] {
            assert!(printable(ok), "{ok}");
        }
        assert_eq!(utf16_len("a\u{1f600}"), 3);
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
