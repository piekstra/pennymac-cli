//! Portal-specific date handling.
//!
//! Pennymac's API returns timestamps as ISO-8601 datetimes at midnight —
//! `"2026-09-01T00:00:00"`, sometimes with a fractional part. The CLI speaks
//! plain ISO `YYYY-MM-DD` at its boundary (SPEC v1), so everything is trimmed
//! to the date. A `null`, an empty string, or the epoch sentinel reads as
//! absent rather than as a real 1970 date.

use pk_cli_core::dates::{fmt_iso, parse_iso, Civil};

/// Read a portal timestamp (`YYYY-MM-DDT…`) as an ISO date, or `None` if it is
/// absent or a placeholder.
pub fn iso_date(raw: &str) -> Option<String> {
    let date = raw.split('T').next().unwrap_or(raw).trim();
    if date.is_empty() {
        return None;
    }
    let civil = parse_iso(date).ok()?;
    if is_placeholder(civil) {
        return None;
    }
    Some(fmt_iso(civil))
}

/// Convenience for a `serde_json` value that may be a string or null.
pub fn iso_date_opt(v: Option<&serde_json::Value>) -> Option<String> {
    v.and_then(|v| v.as_str()).and_then(iso_date)
}

/// `0001-01-01` (.NET `DateTime.MinValue`) and `1900-01-01` are the portal's
/// "never" markers; neither is a real date to surface.
fn is_placeholder(c: Civil) -> bool {
    c == (1, 1, 1) || c == (1900, 1, 1) || c.0 <= 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_the_time_component() {
        assert_eq!(
            iso_date("2026-09-01T00:00:00").as_deref(),
            Some("2026-09-01")
        );
        assert_eq!(
            iso_date("2026-08-05T20:34:47.04").as_deref(),
            Some("2026-08-05")
        );
        assert_eq!(iso_date("2026-09-01").as_deref(), Some("2026-09-01"));
    }

    #[test]
    fn absent_and_placeholder_dates_read_as_none() {
        assert_eq!(iso_date(""), None);
        assert_eq!(iso_date("   "), None);
        assert_eq!(iso_date("0001-01-01T00:00:00"), None);
        assert_eq!(iso_date("1900-01-01"), None);
        assert_eq!(iso_date("not-a-date"), None);
    }

    #[test]
    fn iso_date_opt_handles_null_and_strings() {
        use serde_json::json;
        assert_eq!(
            iso_date_opt(Some(&json!("2026-09-01T00:00:00"))).as_deref(),
            Some("2026-09-01")
        );
        assert_eq!(iso_date_opt(Some(&json!(null))), None);
        assert_eq!(iso_date_opt(None), None);
    }
}
