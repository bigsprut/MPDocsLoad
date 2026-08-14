//! Команды CLI.

pub mod archive;
mod context;
pub mod doctor;
pub mod download;
pub mod out_of_scope;
pub mod profiles;
pub mod providers;
pub mod reports;
pub mod schedule;

pub use context::Context;

/// Пишет запись в журнал событий (общая БД с GUI, вкладка «Журнал»).
/// `kind` — код уровня: info/success/error (словарь см. gui channels::LogKind).
/// Сбой записи не должен ломать команду — только предупреждение в stderr.
pub(crate) fn journal_write(
    ctx: &Context,
    kind: &str,
    origin: &mdwf_core::LogOrigin,
    message: &str,
) {
    use chrono::Utc;
    if let Err(e) =
        ctx.catalog
            .add_journal_entry(Utc::now(), kind, &origin.as_str(), message)
    {
        eprintln!("журнал: не удалось записать в БД: {e}");
    }
}

/// «Субъект» записи журнала: человекочитаемое название отчёта + период
/// (тот же словарь, что в GUI — mdwf_core::journal). `display_name` — из
/// capabilities провайдера; fallback — технический type_id.
pub(crate) fn journal_subject(
    display_name: Option<&str>,
    report_type: &str,
    period: Option<&str>,
) -> String {
    let name = display_name.unwrap_or(report_type);
    match mdwf_core::describe_report_period(period) {
        Some(p) => format!("«{name}» ({p})"),
        None => format!("«{name}»"),
    }
}
