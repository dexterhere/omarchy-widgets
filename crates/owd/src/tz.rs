//! Startup check of the `TZ` environment variable. The time library reads whatever file `TZ`
//! names, so `TZ=/dev/zero` would stall `system.time` and a huge file would be read. The crate
//! forbids `unsafe`, so the variable cannot be removed; a bad value stops the daemon instead,
//! before any thread exists. Rule: unset or empty is fine; otherwise at most 64 characters from
//! `A-Za-z0-9_+/:.-`, no leading `/` (after one optional leading `:`), no `..` segment.
//! POSIX rule strings such as `CET-1CEST,M3.5.0` are refused too: use a zone name.
use std::ffi::OsStr;

/// `Err` carries a fixed message that never contains the value.
pub fn check_tz(v: Option<&OsStr>) -> Result<(), &'static str> {
    let Some(v) = v else { return Ok(()) };
    if v.is_empty() {
        return Ok(());
    }
    let s = v.to_str().ok_or("TZ is not valid UTF-8")?;
    if s.len() > 64 {
        return Err("TZ is longer than 64 characters");
    }
    if !s
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"_+/:.-".contains(&b))
    {
        return Err("TZ contains characters outside A-Za-z0-9_+/:.-");
    }
    let name = s.strip_prefix(':').unwrap_or(s);
    if name.starts_with('/') || name.starts_with(':') {
        return Err("TZ must be a zone name, not a path");
    }
    if name.split('/').any(|seg| seg == "..") {
        return Err("TZ must not contain a '..' segment");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ck(s: &str) -> Result<(), &'static str> {
        check_tz(Some(OsStr::new(s)))
    }

    #[test]
    fn accepts_names_and_empty() {
        assert_eq!(check_tz(None), Ok(()));
        for ok in [
            "",
            "UTC",
            "Asia/Kathmandu",
            "America/Argentina/Buenos_Aires",
            "Etc/GMT+5",
            ":Europe/Paris",
            "posix/Europe/Paris",
        ] {
            assert_eq!(ck(ok), Ok(()), "{ok}");
        }
    }

    #[test]
    fn refuses_hostile_values_without_echoing_them() {
        let long = "a".repeat(65);
        for bad in [
            "/dev/zero",
            ":/dev/zero",
            "::/etc/passwd",
            "/etc/shadow",
            "../../etc/passwd",
            "Europe/../../x",
            "a b",
            "UTC\n",
            "<+03>-3",
            "CET-1CEST,M3.5.0",
            "~/x",
            "é",
            &long,
        ] {
            let e = ck(bad).expect_err(bad);
            assert!(!e.contains(bad.trim()) || bad.trim().is_empty(), "{e}");
        }
        use std::os::unix::ffi::OsStrExt;
        assert!(check_tz(Some(OsStr::from_bytes(&[0xff, 0xfe]))).is_err());
    }
}
