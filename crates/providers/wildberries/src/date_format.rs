//! Форматирование дат для WB API (спец. §2.10.2 — RFC3339, Москва UTC+3).

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Europe::Moscow;

/// RFC3339 в часовом поясе Москвы (UTC+3): `2026-07-03T00:00:00+03:00`.
#[must_use]
pub fn format_moscow_rfc3339(dt: DateTime<Utc>) -> String {
    dt.with_timezone(&Moscow).to_rfc3339()
}

/// RFC3339 для даты (начало дня по Москве): `2026-07-03T00:00:00+03:00`.
#[must_use]
pub fn format_date_moscow(date: NaiveDate) -> String {
    let dt = date
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(Moscow)
        .unwrap();
    dt.to_rfc3339()
}

/// Парсинг даты `YYYY-MM-DD`.
pub fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moscow_rfc3339_has_plus03_offset() {
        let dt = DateTime::parse_from_rfc3339("2026-07-03T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        let s = format_moscow_rfc3339(dt);
        assert!(s.ends_with("+03:00"), "got {s}");
        assert!(s.starts_with("2026-07-03T03:00:00"), "got {s}");
    }

    #[test]
    fn date_moscow_format() {
        let d = NaiveDate::from_ymd_opt(2026, 7, 3).unwrap();
        let s = format_date_moscow(d);
        assert_eq!(s, "2026-07-03T00:00:00+03:00");
    }
}
