//! Склейка нескольких выгрузок одного отчёта в один xlsx.
//!
//! Использование: вкладка «Архив» → фильтры (профиль/отчёт/интервал) →
//! кнопка «Склеить…» → диалог со списком текущей выборки (xlsx/csv, один
//! отчёт) → результат: лист «Данные» (шапка из первого файла + строки всех
//! файлов по хронологии) и лист «Файлы» (источники). Чтение xlsx — calamine
//! (сетка ячеек, схема отчёта не нужна), csv — построчно (все ячейки
//! текстом — артикулы/номера с ведущими нулями не теряются). Результат
//! пишется в Журнал (кнопки действий в записи работают).

use std::rc::Rc;

use gtk4::glib::clone;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, CheckButton, Label, ListBox, Orientation, PolicyType, ScrolledWindow,
};
use mdwf_storage::ArchiveEntry;

use crate::channels::UiCommand;

/// Значение ячейки склейки. Числа остаются числами (сортировка/суммы в
/// Excel работают), текст — текстом; пустые не пишутся.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MergeCell {
    Text(String),
    Number(f64),
    Bool(bool),
    Empty,
}

/// Файл для склейки: путь + хронологический ключ сортировки (ISO-строка
/// сравнивается лексикографически) + отображаемая подпись.
#[derive(Debug, Clone)]
struct MergeFile {
    entry: ArchiveEntry,
}

impl MergeFile {
    /// Ключ хронологии: период запроса (YYYY-MM / YYYY-MM-DD) → дата
    /// документа → момент скачивания (последняя инстанция).
    fn sort_key(e: &ArchiveEntry) -> String {
        e.period
            .clone()
            .or_else(|| e.document_date.clone())
            .unwrap_or_else(|| e.downloaded_at.to_rfc3339())
    }
}

/// Строки данных (сетка ячеек) — содержимое листа.
type Grid = Vec<Vec<MergeCell>>;
/// Источник склейки: (имя файла, строк данных, шапка совпадает).
type Sources = Vec<(String, usize, bool)>;

/// Объединяет файлы: шапка первого файла + данные всех (у остальных files
/// первая строка считается шапкой и отбрасывается). Возвращает строки
/// «Данных» и список источников (имя, строк данных, шапка совпадает).
pub(crate) fn combine(files: &[(String, Grid)]) -> (Grid, Sources) {
    let mut rows: Vec<Vec<MergeCell>> = Vec::new();
    let mut sources: Vec<(String, usize, bool)> = Vec::new();
    let mut header: Option<&Vec<MergeCell>> = None;
    for (name, file_rows) in files {
        if file_rows.is_empty() {
            sources.push((name.clone(), 0, false));
            continue;
        }
        let same = match header {
            // Первый непустой файл задаёт шапку.
            None => {
                header = Some(&file_rows[0]);
                rows.push(file_rows[0].clone());
                true
            }
            Some(h) => {
                h.len() == file_rows[0].len()
                    && h.iter().zip(file_rows[0].iter()).all(|(a, b)| a == b)
            }
        };
        for r in &file_rows[1..] {
            rows.push(r.clone());
        }
        sources.push((name.clone(), file_rows.len() - 1, same));
    }
    (rows, sources)
}

/// Читает первый лист xlsx в сетку ячеек (calamine).
pub(crate) fn read_xlsx(path: &str) -> Result<Vec<Vec<MergeCell>>, String> {
    use calamine::{open_workbook_auto, Data, Reader};
    let mut wb = open_workbook_auto(path).map_err(|e| e.to_string())?;
    let range = wb
        .worksheet_range_at(0)
        .transpose()
        .map_err(|e| e.to_string())?
        .ok_or("лист 1 пуст или отсутствует")?;
    let mut rows = Vec::new();
    for row in range.rows() {
        rows.push(
            row.iter()
                .map(|c| match c {
                    Data::Empty => MergeCell::Empty,
                    Data::String(s) => MergeCell::Text(s.clone()),
                    Data::Float(f) => MergeCell::Number(*f),
                    Data::Int(i) => MergeCell::Number(*i as f64),
                    Data::Bool(b) => MergeCell::Bool(*b),
                    other => MergeCell::Text(other.to_string()),
                })
                .collect(),
        );
    }
    Ok(rows)
}

/// Читает csv в строки текстовых ячеек. Разделитель сниффается по первой
/// строке («;» против «,»). Все ячейки — Text: номера/артикулы с ведущими
/// нулями и «числа» с запятой не должны мутировать.
pub(crate) fn read_csv(path: &str) -> Result<Vec<Vec<MergeCell>>, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let text = raw.trim_start_matches('\u{feff}').trim_end();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let first = text.lines().next().unwrap_or_default();
    let delim = if first.matches(';').count() >= first.matches(',').count() {
        ';'
    } else {
        ','
    };
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        rows.push(split_csv_line(line, delim));
    }
    Ok(rows)
}

/// Разбиение csv-строки с учётом кавычек: `"a;b"` — одна ячейка `a;b`,
/// `""` внутри кавычек — экранированная кавычка.
fn split_csv_line(line: &str, delim: char) -> Vec<MergeCell> {
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                cur.push(ch);
            }
        } else if ch == '"' {
            in_quotes = true;
        } else if ch == delim {
            cells.push(MergeCell::Text(cur.trim().to_string()));
            cur.clear();
        } else {
            cur.push(ch);
        }
    }
    cells.push(MergeCell::Text(cur.trim().to_string()));
    cells
}

/// Пишет результат: лист «Данные» + лист «Файлы». Числа — числами.
pub(crate) fn write_merged(path: &str, rows: &[Vec<MergeCell>], sources: &Sources) -> Result<(), String> {
    let mut wb = rust_xlsxwriter::Workbook::new();
    let bold = rust_xlsxwriter::Format::new().set_bold();
    let ws = wb.add_worksheet().set_name("Данные").map_err(|e| e.to_string())?;
    let is_header_row = |r: usize| r == 0;
    for (r, row) in rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let urow: u32 = u32::try_from(r).unwrap_or(u32::MAX);
            let ucol: u16 = u16::try_from(c).unwrap_or(u16::MAX);
            let header = is_header_row(r);
            match cell {
                MergeCell::Text(s) => {
                    if header {
                        ws.write_with_format(urow, ucol, s, &bold).ok();
                    } else {
                        ws.write(urow, ucol, s).ok();
                    }
                }
                MergeCell::Number(f) => {
                    if header {
                        ws.write_with_format(urow, ucol, *f, &bold).ok();
                    } else {
                        ws.write(urow, ucol, *f).ok();
                    }
                }
                MergeCell::Bool(b) => {
                    let v = if *b { "да" } else { "нет" };
                    if header {
                        ws.write_with_format(urow, ucol, v, &bold).ok();
                    } else {
                        ws.write(urow, ucol, v).ok();
                    }
                }
                MergeCell::Empty => {}
            }
        }
    }
    ws.autofit();
    let fs = wb.add_worksheet().set_name("Файлы").map_err(|e| e.to_string())?;
    fs.write_with_format(0, 0, "№", &bold).ok();
    fs.write_with_format(0, 1, "Файл", &bold).ok();
    fs.write_with_format(0, 2, "Строк данных", &bold).ok();
    fs.write_with_format(0, 3, "Шапка совпадает", &bold).ok();
    for (i, (name, n, same)) in sources.iter().enumerate() {
        let r: u32 = u32::try_from(i + 1).unwrap_or(u32::MAX);
        fs.write(r, 0, (i + 1) as u32).ok();
        fs.write(r, 1, name.as_str()).ok();
        fs.write(r, 2, *n as u32).ok();
        fs.write(r, 3, if *same { "да" } else { "НЕТ — проверьте колонки" }).ok();
    }
    fs.autofit();
    wb.save(path).map_err(|e| e.to_string())
}

/// Безопасное имя файла из названия отчёта (для дефолта диалога Save):
/// всё кроме букв/цифр → «_», повторы схлопываются.
pub(crate) fn sanitize_report_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    while s.contains("__") {
        s = s.replace("__", "_");
    }
    s.trim_matches('_').to_string()
}

/// Короткое имя файла (для списков); fallback — весь путь.
fn file_label(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |s| s.to_string_lossy().to_string())
}

/// Открывает диалог склейки по текущей выборке Архива.
pub fn show_dialog() {
    let mut entries = super::archive::current_entries();
    // Хронология: старые сверху (месяцы по порядку).
    entries.sort_by_key(MergeFile::sort_key);
    // Склеивать можно файлы одного отчёта; только xlsx/csv.
    entries.retain(|e| matches!(e.file_format.to_ascii_lowercase().as_str(), "xlsx" | "xls" | "csv"));
    if entries.is_empty() {
        super::archive::notify_archive("Нечего склеивать: в текущей выборке нет файлов Excel/CSV.");
        return;
    }
    let report_types: Vec<String> = entries.iter().map(|e| e.report_type.clone()).collect();
    let single_report = report_types.iter().all(|t| t == &report_types[0]);

    let win = gtk4::Window::new();
    win.set_title(Some("Склейка выгрузок в один Excel"));
    win.set_default_size(680, 480);
    win.set_modal(true);
    let root = GtkBox::new(Orientation::Vertical, 10);
    root.set_margin_start(14);
    root.set_margin_end(14);
    root.set_margin_top(14);
    root.set_margin_bottom(14);

    let info = if single_report {
        let name = entries[0]
            .report_display_name
            .clone()
            .unwrap_or_else(|| entries[0].report_type.clone());
        format!(
            "Отчёт: «{name}». Файлов в выборке: {}. Отметьте склеиваемые — порядок хронологический.",
            entries.len()
        )
    } else {
        "⚠ В выборке несколько РАЗНЫХ отчётов — склейка возможна только одного. \
         Установите фильтр «Отчёт» на вкладке «Архив» и повторите."
            .to_string()
    };
    root.append(&Label::new(Some(&info)));

    let checks: Rc<std::cell::RefCell<Vec<(ArchiveEntry, CheckButton)>>> =
        Rc::new(std::cell::RefCell::new(Vec::new()));
    let list = ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    let scroll = ScrolledWindow::new();
    scroll.set_policy(PolicyType::Automatic, PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&list));
    for e in &entries {
        let row = GtkBox::new(Orientation::Horizontal, 8);
        let cb = CheckButton::builder()
            .active(true)
            .label(format!(
                "{} ({})",
                file_label(&e.file_path),
                e.period.as_deref().map_or_else(
                    || e.downloaded_at.format("%d.%m.%Y").to_string(),
                    super::disp_date
                )
            ))
            .build();
        row.append(&cb);
        list.append(&row);
        checks.borrow_mut().push((e.clone(), cb));
    }
    root.append(&scroll);

    let hint = Label::new(Some(
        "Шапка берётся из первого (самого старого) файла; у остальных первая строка \
         считается шапкой и отбрасывается. Лист «Файлы» в результате перечисляет \
         источники и отмечает несовпадение колонок.",
    ));
    hint.set_wrap(true);
    hint.set_css_classes(&["dim-label"]);
    root.append(&hint);

    let btns = GtkBox::new(Orientation::Horizontal, 8);
    let cancel = gtk4::Button::with_label("Отмена");
    let merge_btn = gtk4::Button::with_label("Склеить в Excel…");
    merge_btn.add_css_class("suggested-action");
    merge_btn.set_sensitive(single_report);
    btns.append(&cancel);
    btns.append(&merge_btn);
    root.append(&btns);
    win.set_child(Some(&root));

    cancel.connect_clicked(clone!(@weak win => move |_| win.close()));
    merge_btn.connect_clicked(clone!(@weak win => move |_| {
        let selected: Vec<ArchiveEntry> = checks
            .borrow()
            .iter()
            .filter(|(_, cb)| cb.is_active())
            .map(|(e, _)| e.clone())
            .collect();
        if selected.len() < 2 {
            super::archive::notify_archive("Для склейки отметьте не меньше двух файлов.");
            return;
        }
        win.close();
        run_merge(&selected);
    }));

    win.present();
}

/// Читает выбранные файлы и пишет объединённый xlsx (дилог Save → Журнал).
fn run_merge(selected: &[ArchiveEntry]) {
    let report_type = selected[0].report_type.clone();
    let display_name = selected[0]
        .report_display_name
        .clone()
        .unwrap_or_else(|| report_type.clone());
    let first = selected
        .iter()
        .map(|e| MergeFile::sort_key(e))
        .min()
        .unwrap_or_default();
    let last = selected
        .iter()
        .map(|e| MergeFile::sort_key(e))
        .max()
        .unwrap_or_default();
    let default_name = format!(
        "склейка_{}_{}-{}.xlsx",
        sanitize_report_name(&display_name),
        first.chars().take(7).collect::<String>(),
        last.chars().take(7).collect::<String>()
    );

    let dlg = gtk4::FileChooserDialog::builder()
        .title("Куда сохранить склейку")
        .action(gtk4::FileChooserAction::Save)
        .build();
    dlg.set_current_name(&default_name);
    dlg.add_button("Отмена", gtk4::ResponseType::Cancel);
    dlg.add_button("Сохранить", gtk4::ResponseType::Accept);

    let selected = selected.to_vec();
    dlg.connect_response(move |dlg, resp| {
        if resp != gtk4::ResponseType::Accept {
            dlg.close();
            return;
        }
        let Some(file) = dlg.file() else { dlg.close(); return; };
        let Some(path) = file.path() else { dlg.close(); return; };
        let out = path.display().to_string();
        dlg.close();

        // Чтение всех файлов (xlsx первым листом / csv).
        let mut files: Vec<(String, Vec<Vec<MergeCell>>)> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        for e in &selected {
            let ext = e.file_format.to_ascii_lowercase();
            let res = if ext == "csv" {
                read_csv(&e.file_path)
            } else {
                read_xlsx(&e.file_path)
            };
            match res {
                Ok(rows) => files.push((file_label(&e.file_path), rows)),
                Err(err) => errors.push(format!("{}: {err}", e.file_path)),
            }
        }
        if !errors.is_empty() {
            super::archive::notify_archive(&format!(
                "Не удалось прочитать файлы — склейка прервана:\n{}",
                errors.join("\n")
            ));
            return;
        }
        let (rows, sources) = combine(&files);
        let data_rows = rows.len().saturating_sub(1);
        match write_merged(&out, &rows, &sources) {
            Ok(()) => {
                super::archive::notify_archive(&format!(
                    "Склеено файлов: {}, строк данных: {data_rows} → {out}",
                    files.len()
                ));
                if let Some(cs) = super::archive::cmd_sender() {
                    cs.send(UiCommand::LogCustom {
                        kind: crate::channels::LogKind::Success,
                        message: format!(
                            "«Склейка {display_name}»: файлов {}, строк {data_rows}",
                            files.len()
                        ),
                        file_path: out.clone(),
                        report_type: report_type.clone(),
                    });
                }
            }
            Err(e) => {
                super::archive::notify_archive(&format!("Не удалось записать файл: {e}"));
            }
        }
    });
    dlg.present();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> MergeCell {
        MergeCell::Text(s.to_string())
    }

    #[test]
    fn combine_concatenates_and_tracks_headers() {
        let h = vec![t("Месяц"), t("Сумма")];
        let f1 = (String::from("a.xlsx"), vec![h.clone(), vec![t("янв"), MergeCell::Number(1.0)]]);
        let f2 = (String::from("b.xlsx"), vec![h.clone(), vec![t("фев"), MergeCell::Number(2.0)]]);
        let other_h = vec![t("Период"), t("Сумма")];
        let f3 = (String::from("c.xlsx"), vec![other_h, vec![t("мар"), MergeCell::Number(3.0)]]);
        let (rows, sources) = combine(&[f1, f2, f3]);
        // 1 шапка + 3 строки данных.
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[3][0], t("мар"));
        assert_eq!(sources.len(), 3);
        assert_eq!(sources[0], (String::from("a.xlsx"), 1, true));
        assert_eq!(sources[1], (String::from("b.xlsx"), 1, true));
        assert_eq!(sources[2], (String::from("c.xlsx"), 1, false));
    }

    #[test]
    fn csv_sniffs_delimiter_and_strips_quotes() {
        let dir = std::env::temp_dir().join("mdwf_merge_test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.csv");
        std::fs::write(&p, "a;b;c\n\"x;1\";2;3\n").unwrap();
        let rows = read_csv(p.to_str().unwrap()).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].len(), 3);
        assert_eq!(rows[1][0], t("x;1"));
        assert_eq!(rows[1][1], t("2"));
    }

    #[test]
    #[ignore = "локальный smoke на реальных файлах пользователя (не CI)"]
    fn merge_real_postings_files() {
        let files = [
            "D:/work/Learn/ZCode/MPDocsLoad/MDWF/downloads/ozon/2026/ozon_oz_prof1_ozon.postings_2026-05.xlsx",
            "D:/work/Learn/ZCode/MPDocsLoad/MDWF/downloads/ozon/2026/ozon_oz_prof1_ozon.postings_2026-06.xlsx",
            "D:/work/Learn/ZCode/MPDocsLoad/MDWF/downloads/ozon/2026/ozon_oz_prof1_ozon.postings_2026-07.xlsx",
        ];
        let mut fs: Vec<(String, Grid)> = Vec::new();
        for f in files {
            fs.push((file_label(f), read_xlsx(f).expect(f)));
        }
        let (rows, sources) = combine(&fs);
        let out = "C:/Users/MAN-MADE/mdwf_merge_smoke.xlsx";
        write_merged(out, &rows, &sources).unwrap();
        println!("rows total={} sources={:?}", rows.len(), sources);
    }

    #[test]
    fn sanitize_keeps_alnum() {
        assert_eq!(sanitize_report_name("Отчёт о реализации (позаказный)"), "Отчёт_о_реализации_позаказный");
        assert_eq!(sanitize_report_name("A - B"), "A_B");
    }
}
