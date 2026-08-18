//! Экспорт списка Архива в xlsx/CSV (кнопка «Экспорт» на вкладке «Архив»).
//!
//! Чистая логика (без GTK) — диалог выбора файла и запись на диск остаются
//! в `archive.rs`. Колонки и форматирование повторяют таблицу вкладки
//! (урок #54: пользователь видит в файле то же, что на экране) + добавлен
//! столбец «Путь к файлу» — ради него выгрузку и делают.

use mdwf_storage::ArchiveEntry;

use super::{disp_date, ext_label};

/// Заголовки колонок экспорта (xlsx и CSV).
pub(crate) const HEADERS: [&str; 7] = [
    "Профиль",
    "Отчёт",
    "Период",
    "Формат",
    "Размер",
    "Скачан",
    "Путь к файлу",
];

/// Одна строка экспорта — те же значения, что в таблице «Архива».
fn row_of(e: &ArchiveEntry) -> [String; 7] {
    [
        e.profile_name.clone(),
        e.report_display_name
            .clone()
            .unwrap_or_else(|| e.report_type.clone()),
        e.period
            .as_deref()
            .map_or_else(|| "—".into(), disp_date),
        ext_label(&e.file_format),
        super::archive::human_size(u64::try_from(e.file_size).unwrap_or(0)),
        e.downloaded_at
            .with_timezone(&chrono::Local)
            .format("%d.%m.%Y %H:%M")
            .to_string(),
        e.file_path.clone(),
    ]
}

/// Собирает xlsx с листом «Архив»: жирная шапка, autofit ширины.
pub(crate) fn to_xlsx(entries: &[ArchiveEntry]) -> Result<Vec<u8>, String> {
    use rust_xlsxwriter::Format;
    let mut wb = rust_xlsxwriter::Workbook::new();
    let sheet = wb.add_worksheet();
    sheet.set_name("Архив").map_err(|e| e.to_string())?;
    let bold = Format::new().set_bold();
    for (col, title) in HEADERS.iter().enumerate() {
        sheet
            .write_string_with_format(0, col as u16, *title, &bold)
            .map_err(|e| e.to_string())?;
    }
    for (i, e) in entries.iter().enumerate() {
        let row = (i + 1) as u32;
        for (col, cell) in row_of(e).iter().enumerate() {
            sheet
                .write_string(row, col as u16, cell)
                .map_err(|e| e.to_string())?;
        }
    }
    let _ = sheet.autofit();
    wb.save_to_buffer().map_err(|e| e.to_string())
}

/// Собирает CSV для Excel (русская локаль): разделитель «;», BOM UTF-8.
pub(crate) fn to_csv(entries: &[ArchiveEntry]) -> String {
    let mut out = String::from('\u{FEFF}');
    write_csv_row(&mut out, HEADERS.iter().copied());
    for e in entries {
        let row = row_of(e);
        write_csv_row(&mut out, row.iter().map(String::as_str));
    }
    out
}

/// Одна CSV-строка: поля с «;», кавычками или переводом строки берутся в
/// кавычки (внутренние кавычки удваиваются — RFC 4180).
fn write_csv_row<'a>(out: &mut String, cells: impl Iterator<Item = &'a str>) {
    let mut first = true;
    for c in cells {
        if !first {
            out.push(';');
        }
        first = false;
        if c.contains(';') || c.contains('"') || c.contains('\n') || c.contains('\r') {
            out.push('"');
            out.push_str(&c.replace('"', "\"\""));
            out.push('"');
        } else {
            out.push_str(c);
        }
    }
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use mdwf_storage::ArchiveEntry;

    fn entry(profile: &str, report: &str, size: i64, fmt: &str) -> ArchiveEntry {
        ArchiveEntry {
            id: 1,
            profile_id: 1,
            profile_name: profile.into(),
            provider_id: "ozon".into(),
            report_type: report.into(),
            report_display_name: Some("Отчёт по реализации".into()),
            cabinet_url: None,
            period: Some("2026-07".into()),
            file_path: "D:/документы; файлы/x.csv".into(),
            file_size: size,
            file_format: fmt.into(),
            document_id: None,
            document_date: None,
            downloaded_at: chrono::Utc
                .with_ymd_and_hms(2026, 8, 15, 10, 30, 0)
                .unwrap(),
        }
    }

    #[test]
    fn csv_shape_and_escaping() {
        let entries = vec![entry("oz_prof1", "ozon.realization", 123_456, "csv")];
        let csv = to_csv(&entries);
        let lines: Vec<&str> = csv.trim_end_matches('\n').split('\n').collect();
        assert_eq!(lines.len(), 2, "шапка + 1 строка");
        assert!(csv.starts_with('\u{FEFF}'), "BOM для Excel");
        assert!(csv.contains("Профиль;Отчёт;Период;Формат;Размер;Скачан;Путь к файлу"));
        // Путь с «;» взят в кавычки с удвоением — не ломает колонки.
        assert!(csv.contains("\"D:/документы; файлы/x.csv\""));
        assert!(csv.contains("oz_prof1"));
        assert!(csv.contains("Отчёт по реализации"));
        assert!(csv.contains("07.2026"));
        assert!(csv.contains("15.08.2026"));
    }

    #[test]
    fn csv_quote_escaping() {
        let mut e = entry("p", "r", 1, "pdf");
        e.report_display_name = Some("Отчёт \"годовой\"".into());
        let csv = to_csv(&[e]);
        assert!(csv.contains("\"Отчёт \"\"годовой\"\"\""), "внутренние кавычки удвоены");
    }

    #[test]
    fn xlsx_is_zip_with_rows() {
        let entries = vec![
            entry("oz_prof1", "ozon.realization", 100, "csv"),
            entry("wb_prof", "wb.detailed", 200, "xlsx"),
        ];
        let bytes = to_xlsx(&entries).expect("xlsx собирается");
        // Настоящий xlsx — ZIP (magic bytes PK; урок #29: содержимое = формат).
        assert!(bytes.starts_with(b"PK"), "xlsx должен быть ZIP-архивом");
        assert!(bytes.len() > 500);
    }

    #[test]
    fn empty_entries_still_export_headers() {
        let csv = to_csv(&[]);
        assert!(csv.contains("Профиль;Отчёт"));
        assert!(to_xlsx(&[]).is_ok());
    }
}
