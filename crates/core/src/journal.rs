//! Журнал событий приложения: общие типы для GUI и CLI.
//!
//! Журнал персистится в SQLite (таблица `journal`, крейт mdwf-storage);
//! этот модуль — единый источник словаря ИСТОЧНИКОВ события (origin) и
//! человекочитаемых описаний периода (используются в записях журнала
//! обоими бинарниками — текст всегда одинаковый).

use std::fmt;

const MONTHS_NOM: [&str; 12] = [
    "январь", "февраль", "март", "апрель", "май", "июнь",
    "июль", "август", "сентябрь", "октябрь", "ноябрь", "декабрь",
];

const MONTHS_GEN: [&str; 12] = [
    "января", "февраля", "марта", "апреля", "мая", "июня",
    "июля", "августа", "сентября", "октября", "ноября", "декабря",
];

/// Источник события журнала: ПОЧЕМУ оно произошло (вручную / CLI /
/// расписание — и как именно запущено расписание). Хранится в БД как
/// строка `Display`; старые записи (до появления origin) — пустая строка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogOrigin {
    /// Действие пользователя в окне приложения (кнопки, диалоги).
    ManualGui,
    /// Разовая команда командной строки (`mdwf …`), запущенная человеком.
    Cli,
    /// Расписание, сработавшее фоновым циклом планировщика открытого GUI.
    ScheduleGuiLoop(String),
    /// Расписание, запущенное кнопкой «▶ Выполнить сейчас» в GUI.
    ScheduleManualRun(String),
    /// Расписание, исполненное фоновой задачей Windows Task Scheduler
    /// (`mdwf schedule run --by-task`, работает без открытого GUI).
    ScheduleWinTask(String),
    /// Расписание, исполненное командой `mdwf schedule run` из терминала.
    ScheduleCliRun(String),
}

impl LogOrigin {
    /// Строка для колонки `journal.origin`.
    #[must_use]
    pub fn as_str(&self) -> String {
        match self {
            Self::ManualGui => "вручную (GUI)".into(),
            Self::Cli => "CLI".into(),
            Self::ScheduleGuiLoop(n) => format!("расписание «{n}», автозапуск"),
            Self::ScheduleManualRun(n) => format!("расписание «{n}», запуск вручную"),
            Self::ScheduleWinTask(n) => format!("расписание «{n}», задача Windows"),
            Self::ScheduleCliRun(n) => format!("расписание «{n}», запуск из CLI"),
        }
    }
}

impl fmt::Display for LogOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_str())
    }
}

/// Человекочитаемое описание диапазона дат — ЧИСТАЯ функция от [from, to],
/// независимо от того, как даты заданы (виджет интервала, ручной ввод, restore):
/// ровно год → «2024 год»; ровно полугодие → «первое/второе полугодие 2024»;
/// ровно квартал → «3 квартал 2025»; ровно месяц → «январь 2025»;
/// один день (from == to) → «23 января 2026»; прочее → «с 04.03.2025 по 06.03.2025».
/// None — одна из дат не задана/не парсится.
#[must_use]
pub fn describe_range(
    from: Option<chrono::NaiveDate>,
    to: Option<chrono::NaiveDate>,
) -> Option<String> {
    use chrono::Datelike;
    let f = from?;
    let t = to?;

    // Один день.
    if f == t {
        return Some(format!(
            "{} {} {}",
            f.day(),
            MONTHS_GEN[f.month0() as usize],
            f.year()
        ));
    }
    // Стандартные интервалы проверяем только внутри одного года (границы
    // календарных периодов не пересекают годы).
    if f.year() == t.year() {
        let y = f.year();
        // Последний день месяца t (для проверок «ровно …»).
        let last_of = |m: u32| {
            chrono::NaiveDate::from_ymd_opt(y, m + 1, 1)
                .and_then(|d| d.pred_opt())
                .or_else(|| chrono::NaiveDate::from_ymd_opt(y, 12, 31))
        };
        // Ровно год: 1 января .. 31 декабря.
        if (f.month(), f.day()) == (1, 1) && (t.month(), t.day()) == (12, 31) {
            return Some(format!("{y} год"));
        }
        // Ровно полугодие: 01.01–30.06 или 01.07–31.12.
        if (f.month(), f.day()) == (1, 1) && (t.month(), t.day()) == (6, 30) {
            return Some(format!("первое полугодие {y}"));
        }
        if (f.month(), f.day()) == (7, 1) && (t.month(), t.day()) == (12, 31) {
            return Some(format!("второе полугодие {y}"));
        }
        // Ровно месяц: первое число .. последнее число того же месяца.
        if f.day() == 1 && t.month() == f.month() && last_of(f.month()) == Some(t) {
            return Some(format!("{} {}", MONTHS_NOM[f.month0() as usize], y));
        }
        // Ровно квартал: первый месяц квартала (1/4/7/10), день 1, до конца
        // третьего месяца. (Проверка после месяца — диапазоны не пересекаются.)
        if f.day() == 1 && matches!(f.month(), 1 | 4 | 7 | 10) && t.month() == f.month() + 2 {
            if let Some(last) = last_of(t.month()) {
                if last == t {
                    let q = (f.month() - 1) / 3 + 1;
                    return Some(format!("{q} квартал {y}"));
                }
            }
        }
    }
    // Произвольный диапазон.
    Some(format!(
        "с {} по {}",
        f.format("%d.%m.%Y"),
        t.format("%d.%m.%Y")
    ))
}

/// Человекочитаемое описание периода выгрузки для журнала:
/// "2026-07" → «июль 2026», "2026-07-01" → «1 июля 2026».
/// None/нечитаемое значение → None (журнал покажет запись без периода).
#[must_use]
pub fn describe_report_period(period: Option<&str>) -> Option<String> {
    let p = period?;
    if p.len() == 7 {
        // YYYY-MM: месяц целиком.
        let (y, m) = p.split_once('-')?;
        let y: i32 = y.parse().ok()?;
        let m: u32 = m.parse().ok()?;
        if !(1..=12).contains(&m) {
            return None;
        }
        let first = chrono::NaiveDate::from_ymd_opt(y, m, 1)?;
        let last = first
            .checked_add_months(chrono::Months::new(1))?
            .pred_opt()?;
        describe_range(Some(first), Some(last))
    } else {
        // YYYY-MM-DD: один день.
        let d = chrono::NaiveDate::parse_from_str(p, "%Y-%m-%d").ok()?;
        describe_range(Some(d), Some(d))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn describe_range_standard_intervals() {
        assert_eq!(
            describe_range(Some(d("2024-01-01")), Some(d("2024-12-31"))),
            Some("2024 год".into())
        );
        assert_eq!(
            describe_range(Some(d("2024-01-01")), Some(d("2024-06-30"))),
            Some("первое полугодие 2024".into())
        );
        assert_eq!(
            describe_range(Some(d("2024-07-01")), Some(d("2024-12-31"))),
            Some("второе полугодие 2024".into())
        );
        assert_eq!(
            describe_range(Some(d("2025-07-01")), Some(d("2025-09-30"))),
            Some("3 квартал 2025".into())
        );
        assert_eq!(
            describe_range(Some(d("2025-01-01")), Some(d("2025-03-31"))),
            Some("1 квартал 2025".into())
        );
        assert_eq!(
            describe_range(Some(d("2025-01-01")), Some(d("2025-01-31"))),
            Some("январь 2025".into())
        );
        assert_eq!(
            describe_range(Some(d("2025-02-01")), Some(d("2025-02-28"))),
            Some("февраль 2025".into())
        );
    }

    #[test]
    fn describe_range_day_and_custom() {
        assert_eq!(
            describe_range(Some(d("2026-01-23")), Some(d("2026-01-23"))),
            Some("23 января 2026".into())
        );
        assert_eq!(
            describe_range(Some(d("2025-01-01")), Some(d("2025-01-30"))),
            Some("с 01.01.2025 по 30.01.2025".into())
        );
        // Межгодовой диапазон.
        assert_eq!(
            describe_range(Some(d("2024-11-01")), Some(d("2025-02-28"))),
            Some("с 01.11.2024 по 28.02.2025".into())
        );
    }

    #[test]
    fn describe_range_missing_dates() {
        assert_eq!(describe_range(None, Some(d("2025-01-01"))), None);
        assert_eq!(describe_range(Some(d("2025-01-01")), None), None);
    }

    #[test]
    fn describe_report_period_month_and_day() {
        assert_eq!(
            describe_report_period(Some("2026-07")),
            Some("июль 2026".into())
        );
        assert_eq!(
            describe_report_period(Some("2026-01")),
            Some("январь 2026".into())
        );
        assert_eq!(
            describe_report_period(Some("2026-12")),
            Some("декабрь 2026".into())
        );
        assert_eq!(
            describe_report_period(Some("2026-07-01")),
            Some("1 июля 2026".into())
        );
    }

    #[test]
    fn describe_report_period_invalid_and_missing() {
        assert_eq!(describe_report_period(None), None);
        assert_eq!(describe_report_period(Some("")), None);
        assert_eq!(describe_report_period(Some("2026-13")), None);
        assert_eq!(describe_report_period(Some("2026-00")), None);
        assert_eq!(describe_report_period(Some("июль")), None);
        assert_eq!(describe_report_period(Some("2026-7")), None);
    }

    #[test]
    fn log_origin_vocabulary() {
        assert_eq!(LogOrigin::ManualGui.as_str(), "вручную (GUI)");
        assert_eq!(LogOrigin::Cli.as_str(), "CLI");
        assert_eq!(
            LogOrigin::ScheduleGuiLoop("fb_smoke".into()).as_str(),
            "расписание «fb_smoke», автозапуск"
        );
        assert_eq!(
            LogOrigin::ScheduleManualRun("fb_smoke".into()).as_str(),
            "расписание «fb_smoke», запуск вручную"
        );
        assert_eq!(
            LogOrigin::ScheduleWinTask("fb_smoke".into()).as_str(),
            "расписание «fb_smoke», задача Windows"
        );
        assert_eq!(
            LogOrigin::ScheduleCliRun("fb_smoke".into()).as_str(),
            "расписание «fb_smoke», запуск из CLI"
        );
    }
}
