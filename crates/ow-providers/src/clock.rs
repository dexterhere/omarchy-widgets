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

    /// (zone, UTC instant, hm, hms, date, tz)
    #[test]
    fn explicit_zones_dst_and_leap_day() {
        let cases = [
            // no letter abbreviation in tzdata: numeric offset, as `date +%Z` prints
            (
                "Asia/Kathmandu",
                "2026-10-19T04:24:42Z",
                "10:09",
                "10:09:42",
                "Mon 19 Oct",
                "+0545",
            ),
            // spring forward 2027-03-14: 01:59:59 EST, one second later 03:00:00 EDT
            (
                "America/New_York",
                "2027-03-14T06:59:59Z",
                "01:59",
                "01:59:59",
                "Sun 14 Mar",
                "EST",
            ),
            (
                "America/New_York",
                "2027-03-14T07:00:00Z",
                "03:00",
                "03:00:00",
                "Sun 14 Mar",
                "EDT",
            ),
            // fall back 2026-11-01: 01:30 happens twice
            (
                "America/New_York",
                "2026-11-01T05:30:00Z",
                "01:30",
                "01:30:00",
                "Sun 1 Nov",
                "EDT",
            ),
            (
                "America/New_York",
                "2026-11-01T06:30:00Z",
                "01:30",
                "01:30:00",
                "Sun 1 Nov",
                "EST",
            ),
            // leap day, and a single-digit day without padding
            (
                "Europe/Paris",
                "2028-02-29T23:30:00Z",
                "00:30",
                "00:30:00",
                "Wed 1 Mar",
                "CET",
            ),
            (
                "UTC",
                "2028-02-29T12:00:00Z",
                "12:00",
                "12:00:00",
                "Tue 29 Feb",
                "UTC",
            ),
        ];
        for (zone, utc, hm, hms, date, tz) in cases {
            let z = utc
                .parse::<jiff::Timestamp>()
                .unwrap()
                .to_zoned(TimeZone::get(zone).unwrap());
            let m = fields(&z);
            let got = [&m["hm"], &m["hms"], &m["date"], &m["tz"]].map(|v| v.as_str().unwrap());
            assert_eq!(got, [hm, hms, date, tz], "{zone} {utc}");
        }
    }
}
