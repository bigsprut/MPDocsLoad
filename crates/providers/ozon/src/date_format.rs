//! Форматирование дат для Ozon API (спец. §2.9.2).
//!
//! Три формата:
//! 1. ISO 8601 UTC с миллисекундами и Z (v3 endpoints): `2026-07-03T00:00:00.000Z`
//! 2. Только месяц (realization): `2026-06`
//! 3. Только дата (compensation, decompensation, accrual/by-day): `2026-07-03`

use chrono::{DateTime, NaiveDate, Utc};

/// ISO 8601 UTC с миллисекундами и Z: `2026-07-03T00:00:00.000Z`.
#[must_use]
pub fn format_iso8601_ms_z(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// Только месяц: `2026-06`.
#[must_use]
pub fn format_year_month(year: i32, month: u32) -> String {
    format!("{year:04}-{month:02}")
}

/// Только дата: `2026-07-03`.
#[must_use]
pub fn format_date_only(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Разбор даты `YYYY-MM-DD`.
pub fn parse_date_only(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// Разбор года/месяца `YYYY-MM` -> (year, month).
pub fn parse_year_month(s: &str) -> Option<(i32, u32)> {
    let (y, m) = s.split_once('-')?;
    Some((y.parse().ok()?, m.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso8601_ms_z_format() {
        let dt = DateTime::parse_from_rfc3339("2026-07-03T00:00:00Z").unwrap().with_timezone(&Utc);
        assert_eq!(format_iso8601_ms_z(dt), "2026-07-03T00:00:00.000Z");
    }

    #[test]
    fn year_month_format() {
        assert_eq!(format_year_month(2026, 6), "2026-06");
        assert_eq!(format_year_month(2026, 12), "2026-12");
    }

    #[test]
    fn date_only_format() {
        let d = NaiveDate::from_ymd_opt(2026, 7, 3).unwrap();
        assert_eq!(format_date_only(d), "2026-07-03");
    }

    #[test]
    fn parse_roundtrips() {
        let d = NaiveDate::from_ymd_opt(2026, 7, 3).unwrap();
        assert_eq!(parse_date_only("2026-07-03"), Some(d));
        assert_eq!(parse_year_month("2026-06"), Some((2026, 6)));
        assert_eq!(parse_year_month("bad"), None);
    }
}
