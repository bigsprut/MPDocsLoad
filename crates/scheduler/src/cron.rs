//! Парсинг cron-выражений (спец. §2.8.1).

use chrono::{DateTime, Utc};
use cron::Schedule as CronSchedule;

use mdwf_core::{CoreError, CoreResult};

/// Шаблоны cron из спец. §2.8.1 (unix-формат, 5 полей: min hour day month dow).
/// Нормализуются до 6 полей (с секундами) в `parse`.
pub const MONTHLY: &str = "0 2 1 * *"; // 1-е число месяца в 02:00
pub const WEEKLY: &str = "0 9 * * 1"; // каждый понедельник в 09:00
pub const DAILY: &str = "0 9 * * *"; // каждый день в 09:00
pub const QUARTERLY: &str = "0 2 1 1,4,7,10"; // квартально (янв/апр/июл/окт)

/// Парсит cron-выражение (с секундами, 6 полей) и возвращает расписание.
pub fn parse(expr: &str) -> CoreResult<CronSchedule> {
    // Крейт cron использует 6-7 полей (с секундами). Если пользователь дал 5 полей
    // (классический unix cron), добавляем секунды "0 " в начало.
    let normalized = normalize(expr);
    normalized
        .parse::<CronSchedule>()
        .map_err(|e| CoreError::InvalidParameter(format!("cron '{expr}': {e}")))
}

/// Вычисляет следующий запуск от `from`.
pub fn next_run(expr: &str, from: DateTime<Utc>) -> CoreResult<DateTime<Utc>> {
    let sched = parse(expr)?;
    sched
        .after(&from)
        .next()
        .ok_or_else(|| CoreError::Internal(format!("no upcoming run for cron '{expr}'")))
}

/// Нормализует выражение: дополняет до 6 полей (с секундами).
fn normalize(expr: &str) -> String {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    match fields.len() {
        5 => format!("0 {}", expr), // добавляем секунды
        6 | 7 => expr.to_string(),
        _ => expr.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike, TimeZone};

    #[test]
    fn parse_monthly() {
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let next = next_run("0 2 1 * *", now).unwrap();
        // Следующее 1-е число в 02:00 — 2026-08-01 02:00.
        assert_eq!(next.month(), 8);
        assert_eq!(next.day(), 1);
        assert_eq!(next.hour(), 2);
    }

    #[test]
    fn parse_daily() {
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let next = next_run("0 9 * * *", now).unwrap();
        // Сегодняшний 09:00 уже прошёл → завтра 09:00.
        assert_eq!(next.day(), 16);
        assert_eq!(next.hour(), 9);
    }

    #[test]
    fn parse_5_fields_normalized() {
        // 5 полей (unix cron) → нормализуется до 6.
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let next = next_run("0 2 1 * *", now).unwrap();
        assert_eq!(next.day(), 1);
    }

    #[test]
    fn invalid_cron_errors() {
        assert!(parse("not a cron").is_err());
        assert!(parse("99 99 99 99 99 99").is_err());
    }

    #[test]
    fn quarterly_jan_apr_jul_oct() {
        let now = Utc.with_ymd_and_hms(2026, 5, 15, 0, 0, 0).unwrap();
        let next = next_run("0 2 1 1,4,7,10 *", now).unwrap();
        assert_eq!(next.month(), 7);
        assert_eq!(next.day(), 1);
    }
}
