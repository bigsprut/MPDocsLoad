//! Парсинг cron-выражений (спец. §2.8.1).

use chrono::{DateTime, Local, TimeZone, Utc};
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
///
/// Cron-выражение трактуется в **локальном таймзоне** пользователя: GUI задаёт
/// время «как на часах» (напр. 02:00 — это 02:00 по вашему времени). Но
/// возвращается **UTC-инстант**: расписание хранится в БД как RFC3339-UTC, и
/// `claim_schedule` сравнивает такие строки лексикографически (= хронологически).
/// Так «02:00 локально» срабатывает именно в 02:00 локально, а не в 02:00 UTC
/// (что для MSK было бы 05:00). Scheduler всегда работает в сессии/задаче
/// залогиненного юзера, поэтому его Local = таймзона пользователя.
pub fn next_run(expr: &str, from: DateTime<Utc>) -> CoreResult<DateTime<Utc>> {
    let next_local = next_run_in(expr, from.with_timezone(&Local))?;
    Ok(next_local.with_timezone(&Utc))
}

/// Следующий запуск cron в заданном таймзоне `Z`. Обобщение `next_run`:
/// используется с `chrono::Local` в продакшене и с фиксированным смещением
/// (`FixedOffset`) в тестах, чтобы не зависеть от таймзоны машины.
fn next_run_in<Z: TimeZone>(expr: &str, from: DateTime<Z>) -> CoreResult<DateTime<Z>> {
    let sched = parse(expr)?;
    sched
        .after(&from)
        .next()
        .ok_or_else(|| CoreError::Internal(format!("no upcoming run for cron '{expr}'")))
}

/// Форматирует сохранённый UTC-инстант (RFC3339, как пишет `next_run`) как
/// **локальное** время «YYYY-MM-DD HH:MM» для показа пользователю. Расписания
/// хранятся в UTC (для лексикографического сравнения в `claim_schedule`), но
/// показывать юзеру UTC — путать (он задавал время по своим часам). При ошибке
/// парсинга возвращает исходную строку (не падаем на битых данных).
#[must_use]
pub fn fmt_local(rfc: &str) -> String {
    match rfc.parse::<DateTime<Utc>>() {
        Ok(dt) => dt
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        Err(_) => rfc.to_string(),
    }
}

/// Период (YYYY-MM) для запуска расписания: текущий месяц + `offset`
/// (0 = текущий, -1 = прошлый, -2 = позапрошлый). Месяц считается по
/// ЛОКАЛЬНОМУ времени (расписание и период — понятия пользователя, не UTC).
/// Единый источник для GUI и CLI-исполнителей расписаний.
#[must_use]
pub fn period_for_offset(offset: i32) -> String {
    let now = Local::now();
    let months = chrono::Months::new(offset.unsigned_abs());
    let date = if offset >= 0 {
        now.checked_add_months(months)
    } else {
        now.checked_sub_months(months)
    }
    .unwrap_or(now);
    date.format("%Y-%m").to_string()
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
    use chrono::{Datelike, FixedOffset, TimeZone, Timelike};

    /// MSK (+03:00) — фиксированное смещение для детерминированных тестов
    /// (независимо от таймзоны машины, на которой гоняют тесты).
    fn msk() -> FixedOffset {
        FixedOffset::east_opt(3 * 3600).expect("valid offset")
    }

    #[test]
    fn next_run_in_monthly() {
        // В MSK cron «0 2 1 * *»: следующее 1-е число в 02:00 после 15 июля.
        let from = msk().with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let next = next_run_in("0 2 1 * *", from).unwrap();
        assert_eq!(next.month(), 8);
        assert_eq!(next.day(), 1);
        assert_eq!(next.hour(), 2);
    }

    #[test]
    fn next_run_in_daily() {
        let from = msk().with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let next = next_run_in("0 9 * * *", from).unwrap();
        // Сегодняшний 09:00 уже прошёл → завтра 09:00.
        assert_eq!(next.day(), 16);
        assert_eq!(next.hour(), 9);
    }

    #[test]
    fn next_run_in_5_fields_normalized() {
        // 5 полей (unix cron) → нормализуется до 6.
        let from = msk().with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let next = next_run_in("0 2 1 * *", from).unwrap();
        assert_eq!(next.day(), 1);
    }

    #[test]
    fn next_run_in_quarterly() {
        let from = msk().with_ymd_and_hms(2026, 5, 15, 0, 0, 0).unwrap();
        let next = next_run_in("0 2 1 1,4,7,10 *", from).unwrap();
        assert_eq!(next.month(), 7);
        assert_eq!(next.day(), 1);
    }

    #[test]
    fn local_cron_yields_utc_instant() {
        // 02:00 MSK = 23:00 UTC предыдущих суток. Доказательство, что локальное
        // время cron корректно приводится к UTC-инстанту для хранения в БД
        // (claim_schedule сравнивает RFC3339-UTC лексикографически).
        let from = msk().with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let next_local = next_run_in("0 2 1 * *", from).unwrap();
        let next_utc = next_local.with_timezone(&Utc);
        assert_eq!(
            next_utc.format("%Y-%m-%d %H:%M").to_string(),
            "2026-07-31 23:00"
        );
    }

    #[test]
    fn next_run_returns_future_utc() {
        // next_run (Local) возвращает UTC-инстант строго позже `from`.
        let from = Utc::now();
        let next = next_run("0 2 1 * *", from).unwrap();
        assert!(next > from);
    }

    #[test]
    fn invalid_cron_errors() {
        assert!(parse("not a cron").is_err());
        assert!(parse("99 99 99 99 99 99").is_err());
    }
}
