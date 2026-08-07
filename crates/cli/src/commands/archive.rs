//! Команда `archive list` — офлайн-просмотр скачанных файлов (аналог вкладки
//! «Архив» GUI). Только чтение из локального каталога; сетевых запросов нет.

use anyhow::Result;
use chrono::NaiveDate;

use mdwf_storage::ArchiveEntry;

use crate::commands::Context;
use crate::exit_code::ExitCode;
use crate::ArchiveCmd;

pub async fn run(ctx: &Context, action: ArchiveCmd) -> Result<ExitCode> {
    match action {
        ArchiveCmd::List {
            profile,
            report,
            period,
        } => list(ctx, profile.as_deref(), report.as_deref(), period.as_deref()).await,
    }
}

/// `archive list [--profile NAME] [--report TYPE] [--period YYYY-MM]`
async fn list(
    ctx: &Context,
    profile: Option<&str>,
    report: Option<&str>,
    period: Option<&str>,
) -> Result<ExitCode> {
    // Резолв profile_name → profile_id (None = все профили).
    let profile_id = match profile {
        Some(name) => {
            let p = ctx
                .catalog
                .get_profile_by_name(name)?
                .ok_or_else(|| anyhow::anyhow!("профиль '{name}' не найден"))?;
            Some(p.id.ok_or_else(|| anyhow::anyhow!("профиль без id"))?)
        }
        None => None,
    };
    // Период YYYY-MM → диапазон дат (пересечение, как в GUI).
    let date_range = period.and_then(period_to_range);

    let entries = ctx
        .catalog
        .list_downloads_filtered(profile_id, report, date_range)?;

    if entries.is_empty() {
        println!("Ничего не найдено по заданным фильтрам.");
    } else {
        print_archive(&entries);
        println!("\nВсего: {}", entries.len());
    }
    Ok(ExitCode::Success)
}

/// Преобразует период фильтра (YYYY-MM) в диапазон `(from, to)` для inclusion-фильтра
/// (пересечение): первое число месяца .. последний день месяца. Копия логики из GUI.
fn period_to_range(period: &str) -> Option<(String, String)> {
    let (year_s, month_s) = period.split_once('-')?;
    let year: i32 = year_s.parse().ok()?;
    let month: u32 = month_s.parse().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    let last = first
        .checked_add_months(chrono::Months::new(1))?
        .pred_opt()?;
    Some((
        first.format("%Y-%m-%d").to_string(),
        last.format("%Y-%m-%d").to_string(),
    ))
}

/// Печатает таблицу архивных записей: Профиль | Отчёт | Период | Формат | Размер |
/// Скачан | Путь.
fn print_archive(entries: &[ArchiveEntry]) {
    println!(
        "{:<16} {:<24} {:<10} {:<8} {:>10} {:<16} {}",
        "Профиль", "Отчёт", "Период", "Формат", "Размер", "Скачан", "Путь"
    );
    println!("{}", "-".repeat(100));
    for e in entries {
        let period = e.period.clone().unwrap_or_else(|| "—".into());
        let size = human_size(u64::try_from(e.file_size).unwrap_or(0));
        let downloaded = e.downloaded_at.format("%Y-%m-%d %H:%M").to_string();
        println!(
            "{:<16} {:<24} {:<10} {:<8} {:>10} {:<16} {}",
            e.profile_name, e.report_type, period, e.file_format, size, downloaded, e.file_path
        );
    }
}

/// Человекочитаемый размер файла (копия из GUI archive.rs).
fn human_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
