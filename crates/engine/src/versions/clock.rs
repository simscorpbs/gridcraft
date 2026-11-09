//! Time for the version history: UTC seconds and the `YYYY-MM-DDTHH:MM:SSZ` text versions keep,
//! and the computer's local clock for what the history shows. All calendar work is `chrono`'s.

use chrono::{DateTime, Local, Offset, TimeZone, Utc};

/// Seconds since the Unix epoch now (0 if the clock is before it).
pub(crate) fn now_unix() -> u64 {
    u64::try_from(Utc::now().timestamp()).unwrap_or(0)
}

/// `secs` as `2026-11-08T10:30:00Z`.
pub(crate) fn iso_from_unix_secs(secs: u64) -> String {
    i64::try_from(secs)
        .ok()
        .and_then(|s| DateTime::<Utc>::from_timestamp(s, 0))
        .map_or_else(|| "1970-01-01T00:00:00Z".to_owned(), |t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

/// The current time as ISO text.
pub(crate) fn now_iso() -> String {
    iso_from_unix_secs(now_unix())
}

/// The seconds of a stored `YYYY-MM-DDTHH:MM:SSZ` timestamp; `None` for anything else and for
/// times before 1970.
pub(crate) fn parse_iso_timestamp(value: &str) -> Option<u64> {
    if value.len() != 20 || !value.is_ascii() {
        return None;
    }
    let t = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%SZ").ok()?;
    u64::try_from(t.and_utc().timestamp()).ok()
}

/// The local time zone's offset from UTC at `secs`, in seconds (0 when it cannot be told).
pub(crate) fn local_offset(secs: u64) -> i32 {
    let Ok(secs) = i64::try_from(secs) else { return 0 };
    Local.timestamp_opt(secs, 0).single().map_or(0, |t| t.offset().fix().local_minus_utc())
}

/// Format a stored UTC timestamp for the history pane. Version metadata stays UTC on disk so
/// pruning and clients have a stable value; the pane shows the computer's local clock
/// (`YYYY-MM-DD HH:MM`). An unreadable timestamp comes back as it is.
pub fn local_timestamp(saved_at: &str) -> String {
    let Some(secs) = parse_iso_timestamp(saved_at) else { return saved_at.to_owned() };
    i64::try_from(secs)
        .ok()
        .and_then(|s| Local.timestamp_opt(s, 0).single())
        .map_or_else(|| saved_at.to_owned(), |t| t.format("%Y-%m-%d %H:%M").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_round_trip() {
        assert_eq!(iso_from_unix_secs(1_800_000_000), "2027-01-15T08:00:00Z");
        assert_eq!(parse_iso_timestamp("2027-01-15T08:00:00Z"), Some(1_800_000_000));
        for bad in ["", "2027-01-15 08:00:00Z", "2027-13-15T08:00:00Z", "1969-12-31T23:59:59Z", "２０２７-01-15T08:00:00Z"] {
            assert_eq!(parse_iso_timestamp(bad), None, "{bad}");
        }
        assert_eq!(local_timestamp("not a time"), "not a time");
        assert_eq!(local_timestamp("2027-01-15T08:00:00Z").len(), 16);
    }
}
