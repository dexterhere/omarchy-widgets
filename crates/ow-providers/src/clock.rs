//! `system.time`: local wall-clock strings for the clock widget. 24 hour, English, no locale.
//! The zone comes from `TZ` or `/etc/localtime` (jiff reads them in safe code; DECISIONS DEP-TIME).
use jiff::Zoned;
use serde_json::{Map, Value, json};

/// hm "10:09", hms "10:09:42", date "Mon 19 Oct", tz the zone abbreviation ("CEST", "UTC", or
/// "+0545" where tzdata has no letters for the zone, as `date +%Z` prints).
pub(crate) fn fields(now: &Zoned) -> Map<String, Value> {
    let f = |fmt: &str| now.strftime(fmt).to_string();
    json!({
        "hm": f("%H:%M"),
        "hms": f("%H:%M:%S"),
        "date": f("%a %-d %b"),
        "tz": f("%Z"),
    })
    .as_object()
    .cloned()
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use jiff::civil::date;
    use jiff::tz::TimeZone;

    #[test]
    fn shapes_match_the_widget_sample() {
        let z = date(2026, 10, 19)
            .at(10, 9, 42, 0)
            .to_zoned(TimeZone::UTC)
            .unwrap();
        let m = fields(&z);
        assert_eq!(m["hm"], "10:09");
        assert_eq!(m["hms"], "10:09:42");
        assert_eq!(m["date"], "Mon 19 Oct");
        assert_eq!(m["tz"], "UTC");
    }

    #[test]
    fn afternoon_is_24h_and_day_is_not_padded() {
        let z = date(2026, 10, 5)
            .at(0, 5, 3, 0)
            .to_zoned(TimeZone::UTC)
            .unwrap();
        let m = fields(&z);
        assert_eq!(
            (m["hm"].as_str(), m["date"].as_str()),
            (Some("00:05"), Some("Mon 5 Oct"))
        );
        let z = date(2026, 10, 5)
            .at(23, 59, 59, 0)
            .to_zoned(TimeZone::UTC)
            .unwrap();
        assert_eq!(fields(&z)["hms"], "23:59:59");
    }
}
