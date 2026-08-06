//! Вкладка «Архив» (П.6) — офлайн-навигация по скачанным документам.
//!
//! Показывает ВСЕ скачанные файлы всех профилей/провайдеров (из таблицы
//! `downloads`), с опциональными фильтрами: профиль / отчёт / период (YYYY-MM).
//! Действия над строкой: 📂 Открыть файл, 📁 Открыть папку, 📋 Копировать путь.
//! Недеструктивно (без удаления). Данные читаются из локального SQLite —
//! сетевых запросов и токенов не требуется.

use std::cell::RefCell;
use std::rc::Rc;

use chrono::Datelike;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, ComboBoxText, Label, ListBox, Orientation, PolicyType, ScrolledWindow,
};

use mdwf_core::Profile;
use mdwf_storage::ArchiveEntry;

use crate::channels::CommandSender;

thread_local! {
    static CMD: Rc<RefCell<Option<CommandSender>>> = Rc::new(RefCell::new(None));
    static W_PROFILE_COMBO: Rc<RefCell<Option<ComboBoxText>>> = Rc::new(RefCell::new(None));
    static W_REPORT_COMBO: Rc<RefCell<Option<ComboBoxText>>> = Rc::new(RefCell::new(None));
    static W_MONTH_COMBO: Rc<RefCell<Option<ComboBoxText>>> = Rc::new(RefCell::new(None));
    static W_YEAR_COMBO: Rc<RefCell<Option<ComboBoxText>>> = Rc::new(RefCell::new(None));
    static W_LIST: Rc<RefCell<Option<ListBox>>> = Rc::new(RefCell::new(None));
    static W_RESULT: Rc<RefCell<Option<Label>>> = Rc::new(RefCell::new(None));
    /// Карта: отображаемое имя профиля → имя (уникальный ключ в БД).
    /// Заполняется из ProfilesLoaded; используется для резолва выбора combo.
    static PROFILES: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    /// Список report_type, реально присутствующих в архиве (из БД).
    static REPORT_TYPES: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
}

/// Названия месяцев по-русски (индекс 0 = Январь). Индекс 0 в combo — «(все)».
const MONTH_NAMES: [&str; 12] = [
    "Январь",
    "Февраль",
    "Март",
    "Апрель",
    "Май",
    "Июнь",
    "Июль",
    "Август",
    "Сентябрь",
    "Октябрь",
    "Ноябрь",
    "Декабрь",
];

/// Строит вкладку «Архив» и возвращает корневой контейнер.
pub fn build(cs: &CommandSender) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.set_margin_top(16);
    root.set_margin_bottom(16);

    root.append(
        &Label::builder()
            .label("Архив скачанных документов")
            .css_classes(["title-2"])
            .halign(gtk4::Align::Start)
            .build(),
    );
    root.append(
        &Label::builder()
            .label("Все выгруженные файлы из локального каталога. Данные только на этом компьютере — сетевых запросов нет. Задайте фильтры и нажмите «Применить».")
            .css_classes(["dim-label"])
            .halign(gtk4::Align::Start)
            .wrap(true)
            .build(),
    );

    // --- Панель фильтров ---
    let filters = GtkBox::new(Orientation::Horizontal, 8);
    filters.set_margin_bottom(4);

    let profile_combo = ComboBoxText::new();
    profile_combo.set_tooltip_text(Some("Профиль (все — любой магазин)"));
    filters.append(&Label::new(Some("Профиль:")));
    filters.append(&profile_combo);

    let report_combo = ComboBoxText::new();
    report_combo.set_tooltip_text(Some("Тип отчёта"));
    filters.append(&Label::new(Some("Отчёт:")));
    filters.append(&report_combo);

    // Combo месяца: первый пункт «(все)», затем Январь…Декабрь.
    let month_combo = ComboBoxText::new();
    month_combo.set_tooltip_text(Some("Период отчёта (месяц)"));
    month_combo.append_text("(все)");
    for name in MONTH_NAMES {
        month_combo.append_text(name);
    }
    month_combo.set_active(Some(0));
    filters.append(&Label::new(Some("Месяц:")));
    filters.append(&month_combo);

    // Combo года.
    let today = chrono::Local::now().date_naive();
    let year_combo = ComboBoxText::new();
    year_combo.set_tooltip_text(Some("Период отчёта (год)"));
    for y in (today.year() - 5)..=(today.year() + 1) {
        year_combo.append_text(&y.to_string());
    }
    year_combo.set_active(Some(0));
    filters.append(&Label::new(Some("Год:")));
    filters.append(&year_combo);

    let apply_btn = Button::builder()
        .label("🔍 Применить")
        .tooltip_text("Применить фильтры и обновить список")
        .build();
    filters.append(&apply_btn);
    root.append(&filters);

    // --- Результат/статус вкладки ---
    let result_label = Label::builder()
        .label("Выберите фильтры и нажмите «Применить».")
        .css_classes(["dim-label"])
        .halign(gtk4::Align::Start)
        .wrap(true)
        .build();
    root.append(&result_label);

    // --- Список архивных записей ---
    let list_box = ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::None);
    list_box.set_show_separators(true);

    let scroll = ScrolledWindow::new();
    scroll.set_child(Some(&list_box));
    scroll.set_policy(PolicyType::Automatic, PolicyType::Automatic);
    scroll.set_vexpand(true);
    root.append(&scroll);

    // Сохраняем виджеты в thread-local для обновления из хуков.
    CMD.with(|c| *c.borrow_mut() = Some(cs.clone()));
    W_PROFILE_COMBO.with(|w| *w.borrow_mut() = Some(profile_combo));
    W_REPORT_COMBO.with(|w| *w.borrow_mut() = Some(report_combo));
    W_MONTH_COMBO.with(|w| *w.borrow_mut() = Some(month_combo));
    W_YEAR_COMBO.with(|w| *w.borrow_mut() = Some(year_combo));
    W_LIST.with(|w| *w.borrow_mut() = Some(list_box));
    W_RESULT.with(|w| *w.borrow_mut() = Some(result_label));

    // «Применить»: собираем фильтры и шлём запрос каталога.
    {
        let cs = cs.clone();
        apply_btn.connect_clicked(move |_| {
            let profile = selected_profile();
            let report = selected_report();
            let period = selected_period();
            notify("Запрос архива…");
            cs.send(crate::channels::UiCommand::ListArchive {
                profile_name: profile,
                report_type: report,
                period,
            });
        });
    }

    root
}

/// Хук: список профилей загружен — заполняем combo «Профиль» (с «(все)»).
pub fn on_profiles_loaded(profiles: &[Profile]) {
    let combo = W_PROFILE_COMBO.with(|w| w.borrow().clone());
    let Some(combo) = combo else {
        return;
    };
    combo.remove_all();
    combo.append_text("(все)");
    let names: Vec<String> = profiles.iter().map(|p| p.name.clone()).collect();
    PROFILES.with(|p| *p.borrow_mut() = names.clone());
    for name in &names {
        combo.append_text(name);
    }
    combo.set_active(Some(0));
}

/// Хук: список report_type загружен — заполняем combo «Отчёт» (с «(все)»).
pub fn on_report_types_loaded(report_types: &[String]) {
    let combo = W_REPORT_COMBO.with(|w| w.borrow().clone());
    let Some(combo) = combo else {
        return;
    };
    combo.remove_all();
    combo.append_text("(все)");
    REPORT_TYPES.with(|r| *r.borrow_mut() = report_types.to_vec());
    for rt in report_types {
        combo.append_text(rt);
    }
    combo.set_active(Some(0));
}

/// Хук: результат запроса архива — рендерим список.
pub fn on_archive_listed(res: &Result<Vec<ArchiveEntry>, String>) {
    match res {
        Err(e) => notify(&format!("Ошибка: {e}")),
        Ok(entries) => {
            render_archive(entries);
            notify(&format!("Найдено записей: {}", entries.len()));
        }
    }
}

/// Рендерит список архивных записей в ListBox.
fn render_archive(entries: &[ArchiveEntry]) {
    W_LIST.with(|lw| {
        let Some(list_box) = lw.borrow().clone() else {
            return;
        };
        // Очищаем старые строки.
        while let Some(child) = list_box.first_child() {
            list_box.remove(&child);
        }

        if entries.is_empty() {
            list_box.append(&Label::new(Some("Ничего не найдено по заданным фильтрам.")));
            return;
        }

        // Заголовок таблицы (симметричные колонки по width_chars).
        let header = GtkBox::new(Orientation::Horizontal, 12);
        header.set_margin_start(8);
        header.set_margin_end(8);
        header.set_margin_top(4);
        header.set_margin_bottom(4);
        header.append(&Label::builder().label("Профиль").width_chars(16).xalign(0.0).build());
        header.append(&Label::builder().label("Отчёт").width_chars(22).xalign(0.0).build());
        header.append(&Label::builder().label("Период").width_chars(10).xalign(0.0).build());
        header.append(&Label::builder().label("Формат").width_chars(8).xalign(0.0).build());
        header.append(&Label::builder().label("Размер").width_chars(10).xalign(0.0).build());
        header.append(&Label::builder().label("Скачан").width_chars(12).xalign(0.0).build());
        header.append(&Label::builder().label("Действия").width_chars(20).xalign(0.0).build());
        list_box.append(&header);

        for e in entries {
            let row = GtkBox::new(Orientation::Horizontal, 12);
            row.set_margin_start(8);
            row.set_margin_end(8);
            row.set_margin_top(2);
            row.set_margin_bottom(2);
            row.set_css_classes(&["doc-list-row"]);

            // Профиль + иконка типа файла (П.5 паттерн) как префикс.
            let profile_icon = ext_emoji(&e.file_format);
            row.append(
                &Label::builder()
                    .label(format!("{profile_icon} {}", e.profile_name))
                    .width_chars(16)
                    .xalign(0.0)
                    .ellipsize(gtk4::pango::EllipsizeMode::End)
                    .build(),
            );
            row.append(
                &Label::builder()
                    .label(&e.report_type)
                    .width_chars(22)
                    .xalign(0.0)
                    .ellipsize(gtk4::pango::EllipsizeMode::End)
                    .build(),
            );
            let period_str = e.period.clone().unwrap_or_else(|| "—".to_string());
            row.append(
                &Label::builder()
                    .label(&period_str)
                    .width_chars(10)
                    .xalign(0.0)
                    .build(),
            );
            row.append(
                &Label::builder()
                    .label(&e.file_format)
                    .width_chars(8)
                    .xalign(0.0)
                    .build(),
            );
            let size_str = human_size(u64::try_from(e.file_size).unwrap_or(0));
            row.append(
                &Label::builder()
                    .label(&size_str)
                    .width_chars(10)
                    .xalign(0.0)
                    .build(),
            );
            let dt_str = e.downloaded_at.format("%Y-%m-%d %H:%M").to_string();
            row.append(
                &Label::builder()
                    .label(&dt_str)
                    .width_chars(12)
                    .xalign(0.0)
                    .build(),
            );

            // Действия: 📂 Открыть файл, 📁 Открыть папку, 📋 Копировать путь.
            let actions_box = GtkBox::new(Orientation::Horizontal, 4);
            let file_path = e.file_path.clone();

            let open_btn = Button::builder()
                .label("📂")
                .tooltip_text("Открыть файл")
                .build();
            {
                let path = file_path.clone();
                open_btn.connect_clicked(move |_| {
                    if let Err(err) = crate::views::open_file(&path) {
                        notify(&format!("Не удалось открыть: {err}"));
                    }
                });
            }
            actions_box.append(&open_btn);

            let folder_btn = Button::builder()
                .label("📁")
                .tooltip_text("Открыть папку с файлом")
                .build();
            {
                let path = file_path.clone();
                folder_btn.connect_clicked(move |_| {
                    // Берём родительский каталог пути.
                    let folder = std::path::Path::new(&path)
                        .parent()
                        .map_or_else(|| path.clone(), |p| p.to_string_lossy().to_string());
                    if let Err(err) = crate::views::open_folder(&folder) {
                        notify(&format!("Не удалось открыть папку: {err}"));
                    }
                });
            }
            actions_box.append(&folder_btn);

            let copy_btn = Button::builder()
                .label("📋")
                .tooltip_text("Копировать путь в буфер обмена")
                .build();
            {
                let path = file_path.clone();
                copy_btn.connect_clicked(move |_| {
                    let display = gtk4::gdk::Display::default();
                    if let Some(d) = display {
                        d.clipboard().set_text(&path);
                        notify("Путь скопирован в буфер обмена.");
                    }
                });
            }
            actions_box.append(&copy_btn);

            row.append(&actions_box);
            list_box.append(&row);
        }
    });
}

/// Возвращает выбранный профиль (None = «(все)»).
fn selected_profile() -> Option<String> {
    let combo = W_PROFILE_COMBO.with(|w| w.borrow().clone())?;
    let text = combo.active_text()?.to_string();
    if text == "(все)" || text.is_empty() {
        return None;
    }
    Some(text)
}

/// Возвращает выбранный report_type (None = «(все)»).
fn selected_report() -> Option<String> {
    let combo = W_REPORT_COMBO.with(|w| w.borrow().clone())?;
    let text = combo.active_text()?.to_string();
    if text == "(все)" || text.is_empty() {
        return None;
    }
    Some(text)
}

/// Возвращает выбранный период в формате YYYY-MM (None = «(все)»).
/// Берётся из combo Месяц (индекс 0 = «(все)») + combo Год.
fn selected_period() -> Option<String> {
    let month_idx = W_MONTH_COMBO.with(|w| {
        w.borrow()
            .as_ref()
            .and_then(gtk4::prelude::ComboBoxExtManual::active)
            .map(|i| i as usize)
    })?;
    // Индекс 0 = «(все)» → период не выбран.
    if month_idx == 0 {
        return None;
    }
    let year_text = W_YEAR_COMBO.with(|w| {
        w.borrow()
            .as_ref()
            .and_then(gtk4::ComboBoxText::active_text)
            .map(|s| s.to_string())
    })?;
    let year: i32 = year_text.parse().ok()?;
    // month_idx 1..12 → MONTH_NAMES[month_idx-1] → месяц 1..12.
    let month = month_idx; // combo[1]=Январь=месяц1
    Some(format!("{year}-{month:02}"))
}

/// Локальный статус вкладки (пишет в W_RESULT лейбл).
fn notify(msg: &str) {
    W_RESULT.with(|rw| {
        if let Some(l) = rw.borrow().as_ref() {
            l.set_text(msg);
        }
    });
}

/// Эмодзи по типу файла (как в П.5, для префикса в колонке «Профиль»).
fn ext_emoji(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "xlsx" | "xls" | "csv" => "📊",
        "zip" | "rar" | "7z" | "gz" | "tar" => "📦",
        _ => "📄",
    }
}

/// Человекочитаемый размер файла.
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
