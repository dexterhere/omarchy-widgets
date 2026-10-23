//! Validation of everything a caller can influence. All checks are allow-list style: anything
//! not explicitly permitted is refused. Pure functions, no I/O.

const MAX_PATH: usize = 2048;
const MAX_HEADER_VALUE: usize = 256;
const MAX_HEADER_NAME: usize = 64;

/// Request headers a caller may never set: the wrapper owns them.
const RESERVED_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "host",
    "cookie",
    "user-agent",
    "content-type",
    "content-length",
    "transfer-encoding",
    "connection",
    "accept-encoding",
    "content-encoding",
    "upgrade",
    "te",
    "trailer",
    "expect",
    "forwarded",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-proto",
];

/// A bare lowercase ASCII DNS name with at least two labels, as written in an allow-list.
/// Refuses IP literals (the last label must start with a letter, which also rules out inet_aton
/// shorthand such as `127.1` and `0x7f.1`), ports, userinfo, brackets, a trailing dot, uppercase,
/// non-ASCII and punycode (`xn--`, or any `--` in positions 3 and 4).
pub(crate) fn valid_host(host: &str) -> bool {
    let bytes = host.as_bytes();
    if bytes.is_empty() || bytes.len() > 253 {
        return false;
    }
    let mut labels = 0;
    let mut last = "";
    for label in host.split('.') {
        let l = label.as_bytes();
        if l.is_empty()
            || l.len() > 63
            || l[0] == b'-'
            || l[l.len() - 1] == b'-'
            || label.get(2..4) == Some("--")
            || !l
                .iter()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        {
            return false;
        }
        labels += 1;
        last = label;
    }
    labels >= 2 && last.as_bytes()[0].is_ascii_lowercase()
}

fn hex(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

/// An absolute path with optional query. Must start with a single `/`. Allowed bytes are ASCII
/// alphanumerics and `-._~!$&'()*+,;=:/?%`. Refuses `@`, `#`, `\`, whitespace, controls, non-ASCII,
/// malformed percent escapes, encoded control bytes, encoded `/` and `\` in the path part, and
/// `.` or `..` segments (also encoded).
pub(crate) fn valid_path(path: &str) -> bool {
    let b = path.as_bytes();
    if b.is_empty() || b.len() > MAX_PATH || b[0] != b'/' || b.get(1) == Some(&b'/') {
        return false;
    }
    if !b
        .iter()
        .all(|c| c.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:/?%".contains(c))
    {
        return false;
    }
    let (path_part, query) = path.split_once('?').unwrap_or((path, ""));
    // Percent escapes: well formed, no controls anywhere, no encoded slash or backslash in the
    // path part (some servers decode them into separators).
    for (part, is_path) in [(path_part, true), (query, false)] {
        let p = part.as_bytes();
        let mut i = 0;
        while i < p.len() {
            if p[i] == b'%' {
                let (Some(h), Some(l)) = (
                    p.get(i + 1).and_then(|c| hex(*c)),
                    p.get(i + 2).and_then(|c| hex(*c)),
                ) else {
                    return false;
                };
                let v = h * 16 + l;
                if v < 0x20 || v == 0x7f || v == b'\\' || (is_path && v == b'/') {
                    return false;
                }
                i += 3;
            } else {
                i += 1;
            }
        }
    }
    !path_part.split('/').any(|seg| {
        let s = seg.to_ascii_lowercase().replace("%2e", ".");
        s == "." || s == ".."
    })
}

/// Lowercase header name, token characters only, not one the wrapper owns.
pub(crate) fn valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_HEADER_NAME
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && !RESERVED_HEADERS.contains(&name)
}

/// Printable ASCII and space only: no CR, LF, NUL, tab, other controls or non-ASCII.
pub(crate) fn valid_header_value(value: &str) -> bool {
    value.len() <= MAX_HEADER_VALUE && value.bytes().all(|c| (0x20..=0x7e).contains(&c))
}

pub(crate) fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

/// Hostile host strings. Every entry must be refused by `valid_host`, and (see `tests.rs`) by
/// `Egress::send` before any network activity.
#[cfg(test)]
pub(crate) const HOSTILE_HOSTS: &[&str] = &[
    "",
    " ",
    "localhost",
    "github",
    "127.0.0.1",
    "127.1",
    "0x7f.1",
    "1.0x7f",
    "2130706433",
    "017700000001",
    "10.0.0.1",
    "169.254.169.254",
    "[::1]",
    "[::ffff:127.0.0.1]",
    "::1",
    "api.github.com.",
    "API.GITHUB.COM",
    "Api.github.com",
    "api.github.com:443",
    "api.github.com:80",
    "api.github.com:",
    "api.github.com@evil.com",
    "evil.com@api.github.com",
    "user:pw@api.github.com",
    "api.github.com#@evil.com",
    "api.github.com\\@evil.com",
    "api.github.com\\.evil.com",
    "api.github.com/",
    "api.github.com/evil",
    "api.github.com?x=1",
    "https://api.github.com",
    "http://api.github.com",
    "//api.github.com",
    "api.github.com%00",
    "api.github.com%2f@evil.com",
    "api.github.com\0",
    "api.github.com\r\nX-Evil: 1",
    "api.github.com\n",
    "api.github.com ",
    " api.github.com",
    "api.github.com\t",
    ".api.github.com",
    "api..github.com",
    "*.github.com",
    "-api.github.com",
    "api-.github.com",
    "xn--pi-ld.github.com",
    "xn--80ak6aa92e.com",
    "\u{430}pi.github.com",
    "api.githu\u{1E07}.com",
    "api.github.com\u{200b}",
    "api\u{3002}github\u{3002}com",
    "api.github\u{FF0E}com",
    "\u{FF41}pi.github.com",
    "api.github.c\u{FF4F}m",
    "ab--cd.github.com",
];

/// Well-formed names that are simply not on an allow-list of `api.github.com`. Refused by the
/// exact-match rule in `Egress::send`, not by `valid_host`.
#[cfg(test)]
pub(crate) const LOOKALIKE_HOSTS: &[&str] = &[
    "api.github.com.evil.com",
    "evilapi.github.com",
    "sub.api.github.com",
    "xapi.github.com",
    "github.com",
    "api.github.co",
    "api.github.com-evil.com",
    "evil.com",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_hosts_are_refused() {
        assert!(HOSTILE_HOSTS.len() >= 25);
        for h in HOSTILE_HOSTS {
            assert!(!valid_host(h), "accepted host {h:?}");
        }
    }

    #[test]
    fn plain_dns_names_are_accepted() {
        for h in ["api.github.com", "a.b", "x1.example-host.org"] {
            assert!(valid_host(h), "refused {h:?}");
        }
    }

    #[test]
    fn hostile_paths_are_refused() {
        let long = format!("/{}", "a".repeat(MAX_PATH));
        for p in [
            "",
            "user",
            "https://evil.com/x",
            "//evil.com/x",
            "/\\evil.com",
            "/a\\b",
            "@evil.com",
            "/@evil.com",
            "/a#frag",
            "/a b",
            "/a\tb",
            "/a\r\nHost: evil",
            "/a\nb",
            "/a\0",
            "/\u{e9}",
            "/../x",
            "/a/..",
            "/a/./b",
            "/%2e%2e/x",
            "/%2E/x",
            "/a%2fb",
            "/a%5cb",
            "/a%00b",
            "/a%0d%0ab",
            "/a%zz",
            "/a%4",
            "/a?q=%0a",
            "/a?q=\n",
            long.as_str(),
        ] {
            assert!(!valid_path(p), "accepted path {p:?}");
        }
    }

    #[test]
    fn normal_paths_are_accepted() {
        for p in [
            "/",
            "/user",
            "/search/issues?q=is%3Apr+is%3Aopen&per_page=1",
            "/repos/a/b/pulls?state=open",
            "/a/b.c/d-e_f~g",
        ] {
            assert!(valid_path(p), "refused {p:?}");
        }
    }

    #[test]
    fn header_rules() {
        assert!(valid_header_name("x-github-api-version"));
        for n in [
            "",
            "Accept",
            "authorization",
            "host",
            "cookie",
            "user-agent",
            "a b",
            "a\r\nb",
            "x:y",
        ] {
            assert!(!valid_header_name(n), "accepted name {n:?}");
        }
        assert!(valid_header_value("2022-11-28"));
        for v in [
            "a\r\nb",
            "a\nb",
            "a\0b",
            "a\tb",
            "\u{e9}",
            &"x".repeat(MAX_HEADER_VALUE + 1),
        ] {
            assert!(!valid_header_value(v), "accepted value {v:?}");
        }
    }
}
