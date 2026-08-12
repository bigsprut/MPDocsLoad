//! Вкладка «Архив» (П.6) — офлайн-навигация по скачанным документам.
//!
//! Показывает ВСЕ скачанные файлы всех профилей/провайдеров (из таблицы
//! `downloads`), с опциональными фильтрами: профиль / отчёт / период (YYYY-MM).
//! Действия над строкой: 📂 Открыть файл, 📁 Открыть папку, 📋 Копировать путь.
//! Недеструктивно (без удаления). Данные читаются из локального SQLite —
//! сетевых запросов и токенов не требуется.
//!
//! Фильтры автосохраняются в `ui_state["archive_screen"]` (как DownloadState во
//! вкладке «Загрузка») и восстанавливаются при старте.

use std::cell::RefCell;
use std::rc::Rc;

use chrono::Datelike;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, ComboBoxText, Image, Label, ListBox, Orientation, PolicyType,
    ScrolledWindow,
};
use libadwaita as adw;
use libadwaita::prelude::MessageDialogExt;

use mdwf_core::Profile;
use mdwf_storage::ArchiveEntry;

use crate::channels::{ArchiveState, CommandSender, ReportTypeInfo};

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
    /// Список (display_name, type_id) отчётов, реально присутствующих в архиве.
    /// combo показывает display_name (label), фильтр в БД — по type_id (value).
    /// Паттерн label→value как WB-категории (CATEGORIES в download.rs).
    static REPORT_TYPES: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(Vec::new()));
    /// Отложенный restore: желаемый профиль/отчёт, если combo ещё не заполнен
    /// при приходе ArchiveStateLoaded (аналог PENDING_REPORT во вкладке «Загрузка»).
    static PENDING_PROFILE: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    static PENDING_REPORT_TYPE: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    /// Флаг защиты от автосохранения во время программного set_active (restore).
    /// connect_changed проверяет его, чтобы не перезаписывать восстанавливаемое
    /// состояние дефолтными значениями.
    static RESTORING: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
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
        .label("Загрузка архива…")
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
    W_PROFILE_COMBO.with(|w| *w.borrow_mut() = Some(profile_combo.clone()));
    W_REPORT_COMBO.with(|w| *w.borrow_mut() = Some(report_combo.clone()));
    W_MONTH_COMBO.with(|w| *w.borrow_mut() = Some(month_combo.clone()));
    W_YEAR_COMBO.with(|w| *w.borrow_mut() = Some(year_combo.clone()));
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

    // Автосохранение фильтров при смене combo (как DownloadState во вкладке
    // «Загрузка»). RESTORING защищает от сохранения во время программного
    // set_active при restore сохранённого состояния.
    profile_combo.connect_changed(|_| schedule_save());
    report_combo.connect_changed(|_| schedule_save());
    month_combo.connect_changed(|_| schedule_save());
    year_combo.connect_changed(|_| schedule_save());

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
    // Отложенный restore: если сохранённый профиль ожидает применения — ищем.
    let desired = PENDING_PROFILE.with(|p| p.borrow().clone());
    if let Some(name) = desired {
        set_combo_active_by_text(&combo, &name);
        PENDING_PROFILE.with(|p| *p.borrow_mut() = None);
    }
}

/// Хук: список report_type загружен — заполняем combo «Отчёт» (с «(все)»).
/// combo показывает человекочитаемый display_name, фильтр в БД — по type_id.
pub fn on_report_types_loaded(infos: &[ReportTypeInfo]) {
    let combo = W_REPORT_COMBO.with(|w| w.borrow().clone());
    let Some(combo) = combo else {
        return;
    };
    combo.remove_all();
    combo.append_text("(все)");
    // Пары (display_name, type_id): видим — имя, фильтруем — type_id.
    let pairs: Vec<(String, String)> = infos
        .iter()
        .map(|i| (i.display_name.clone(), i.type_id.clone()))
        .collect();
    for (label, _value) in &pairs {
        combo.append_text(label);
    }
    REPORT_TYPES.with(|r| *r.borrow_mut() = pairs);
    combo.set_active(Some(0));
    // Отложенный restore сохранённого отчёта (по type_id — value, не по тексту).
    let desired = PENDING_REPORT_TYPE.with(|p| p.borrow().clone());
    if let Some(rt) = desired {
        set_report_combo_active_by_value(&combo, &rt);
        PENDING_REPORT_TYPE.with(|p| *p.borrow_mut() = None);
    }
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

/// Хук: сохранённое состояние фильтров загружено (при старте).
/// Восстанавливаем combos (период сразу, профиль/отчёт — из pending или сразу,
/// если combos уже заполнены) и применяем фильтр к списку.
pub fn on_archive_state_loaded(state: Option<&ArchiveState>) {
    // RESTORING: пока true, connect_changed не шлёт автосохранение.
    RESTORING.with(|r| *r.borrow_mut() = true);

    match state {
        None => {
            // Сохранённого состояния нет — показать все записи.
            send_list_archive(None, None, None);
        }
        Some(st) => {
            // Профиль: combo может быть уже заполнен (ProfilesLoaded приходит
            // раньше ArchiveStateLoaded) — restore сразу; иначе в pending.
            if let Some(name) = &st.profile_name {
                let combo = W_PROFILE_COMBO.with(|w| w.borrow().clone());
                if let Some(combo) = combo {
                    if !set_combo_active_by_text(&combo, name) {
                        // Не нашли в combo — запомним для отложенного restore.
                        PENDING_PROFILE.with(|p| *p.borrow_mut() = Some(name.clone()));
                    }
                }
            }
            // Отчёт — аналогично.
            if let Some(rt) = &st.report_type {
                let combo = W_REPORT_COMBO.with(|w| w.borrow().clone());
                if let Some(combo) = combo {
                    if !set_report_combo_active_by_value(&combo, rt) {
                        PENDING_REPORT_TYPE.with(|p| *p.borrow_mut() = Some(rt.clone()));
                    }
                }
            }
            // Период — combos месяц/год статичны (заполнены в build), restore сразу.
            if let Some(period) = &st.period {
                restore_period(period);
            }
            // Применяем восстановленный фильтр к списку.
            send_list_archive(
                st.profile_name.clone(),
                st.report_type.clone(),
                st.period.clone(),
            );
        }
    }

    RESTORING.with(|r| *r.borrow_mut() = false);
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

            // Колонка «Профиль»: иконка типа файла (PNG из gresource) + имя
            // профиля в одном Box, чтобы не сбить выравнивание с header.
            let profile_box = GtkBox::new(Orientation::Horizontal, 6);
            profile_box.append(
                &Image::builder()
                    .resource(ext_icon_resource(&e.file_format))
                    .pixel_size(20)
                    .build(),
            );
            profile_box.append(
                &Label::builder()
                    .label(&e.profile_name)
                    .width_chars(16)
                    .xalign(0.0)
                    .ellipsize(gtk4::pango::EllipsizeMode::End)
                    .build(),
            );
            row.append(&profile_box);
            // Человекочитаемое имя отчёта (с fallback на type_id); tooltip —
            // технический type_id для точной идентификации.
            let report_label = e
                .report_display_name
                .clone()
                .unwrap_or_else(|| e.report_type.clone());
            row.append(
                &Label::builder()
                    .label(&report_label)
                    .tooltip_text(&e.report_type)
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

            // 🗑 Удалить запись и файл (деструктивно, с подтверждением).
            let del_btn = Button::builder()
                .label("🗑")
                .tooltip_text("Удалить запись и файл")
                .css_classes(["destructive-action"])
                .build();
            {
                let id = e.id;
                let path = file_path.clone();
                let file_name = std::path::Path::new(&path)
                    .file_name()
                    .map_or_else(|| path.clone(), |s| s.to_string_lossy().to_string());
                del_btn.connect_clicked(move |_| {
                    show_delete_confirm(id, &file_name, &path);
                });
            }
            actions_box.append(&del_btn);

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
/// combo показывает display_name (label); возвращаем type_id (value) для фильтра БД.
fn selected_report() -> Option<String> {
    let combo = W_REPORT_COMBO.with(|w| w.borrow().clone())?;
    let text = combo.active_text()?.to_string();
    if text == "(все)" || text.is_empty() {
        return None;
    }
    REPORT_TYPES.with(|r| {
        r.borrow()
            .iter()
            .find(|(label, _)| label == &text)
            .map(|(_, value)| value.clone())
    })
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

/// Путь к иконке типа файла в gresource. Регистронезависимо.
fn ext_icon_resource(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "txt" => "/org/mdwf/icons/file-txt.png",
        "xlsx" | "xls" | "csv" => "/org/mdwf/icons/file-xlsx.png",
        "pdf" => "/org/mdwf/icons/file-pdf.png",
        "json" => "/org/mdwf/icons/file-json.png",
        "xml" => "/org/mdwf/icons/file-xml.png",
        "zip" | "rar" | "7z" | "gz" | "tar" => "/org/mdwf/icons/file-zip.png",
        _ => "/org/mdwf/icons/file-generic.png",
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

// ===== Persist фильтров =====

/// Собирает ArchiveState из текущих combo и шлёт SaveArchiveState.
/// Игнорируется во время restore (RESTORING=true), чтобы не перезаписывать
/// восстанавливаемые значения дефолтными.
fn schedule_save() {
    if RESTORING.with(|r| *r.borrow()) {
        return;
    }
    let Some(cs) = CMD.with(|c| c.borrow().clone()) else {
        return;
    };
    cs.send(crate::channels::UiCommand::SaveArchiveState(ArchiveState {
        profile_name: selected_profile(),
        report_type: selected_report(),
        period: selected_period(),
    }));
}

/// Шлёт ListArchive с заданными значениями фильтров.
fn send_list_archive(profile: Option<String>, report: Option<String>, period: Option<String>) {
    let Some(cs) = CMD.with(|c| c.borrow().clone()) else {
        return;
    };
    cs.send(crate::channels::UiCommand::ListArchive {
        profile_name: profile,
        report_type: report,
        period,
    });
}

/// Восстанавливает combo периода из строки YYYY-MM: месяц → combo Месяц
/// (индекс 1..12), год → combo Год (поиск по тексту, т.к. годы динамические).
fn restore_period(period: &str) {
    let Some((year_s, month_s)) = period.split_once('-') else {
        return;
    };
    let Ok(month) = month_s.parse::<u32>() else {
        return;
    };
    let Ok(year) = year_s.parse::<i32>() else {
        return;
    };
    if !(1..=12).contains(&month) {
        return;
    }
    // combo[0]="(все)", combo[month]=месяц. set_active синхронно эмиссирует changed,
    // но RESTORING=true блокирует автосохранение.
    W_MONTH_COMBO.with(|w| {
        if let Some(combo) = w.borrow().as_ref() {
            combo.set_active(Some(month));
        }
    });
    W_YEAR_COMBO.with(|w| {
        if let Some(combo) = w.borrow().as_ref() {
            set_combo_active_by_text(combo, &year.to_string());
        }
    });
}

/// Ищет текст в combo и делает его активным. Возвращает true если найден.
fn set_combo_active_by_text(combo: &ComboBoxText, text: &str) -> bool {
    let n = combo.model().map_or(0, |m| m.iter_n_children(None));
    for i in 0..n {
        combo.set_active(Some(i as u32));
        if combo
            .active_text()
            .is_some_and(|t| t.as_str() == text)
        {
            return true;
        }
    }
    // Не нашли — возвращаем дефолт.
    combo.set_active(Some(0));
    false
}

/// Делает активным элемент combo «Отчёт» по type_id (value), НЕ по видимому тексту.
/// combo показывает display_name (label), поэтому restore сохранённого type_id
/// требует поиска по value в REPORT_TYPES: индекс i в массиве = combo index i+1
/// (индекс 0 — «(все)»). Возвращает true если найден.
fn set_report_combo_active_by_value(combo: &ComboBoxText, type_id: &str) -> bool {
    let idx = REPORT_TYPES.with(|r| r.borrow().iter().position(|(_, value)| value == type_id));
    if let Some(i) = idx {
        combo.set_active(Some((i + 1) as u32));
        true
    } else {
        combo.set_active(Some(0));
        false
    }
}

// ===== Удаление записи + файла =====

/// Показывает диалог подтверждения удаления записи и файла. На «Удалить» —
/// шлёт UiCommand::DeleteDownload. По образцу shop.rs (adw::MessageDialog).
fn show_delete_confirm(id: i64, file_name: &str, file_path: &str) {
    let dialog = adw::MessageDialog::builder()
        .heading("Удалить файл?")
        .body(format!(
            "{file_name}\n\nЗапись будет удалена из архива, а файл — стёрт с диска. Действие необратимо."
        ))
        .build();
    dialog.add_response("cancel", "Отмена");
    dialog.add_response("delete", "Удалить");
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);

    let path = file_path.to_string();
    dialog.connect_response(None, move |_dlg, response: &str| {
        if response == "delete" {
            let Some(cs) = CMD.with(|c| c.borrow().clone()) else {
                return;
            };
            cs.send(crate::channels::UiCommand::DeleteDownload {
                id,
                file_path: path.clone(),
            });
            notify("Удаление…");
        }
    });
    dialog.present();
}

/// Хук: результат удаления записи. При успехе — обновляем список (переотправляем
/// ListArchive с текущими выбранными фильтрами). При ошибке — сообщаем.
pub fn on_download_deleted(res: &Result<i64, String>) {
    match res {
        Ok(_id) => {
            notify("Запись удалена.");
            // Refresh списка с текущими фильтрами combos.
            send_list_archive(selected_profile(), selected_report(), selected_period());
        }
        Err(e) => notify(&format!("Ошибка удаления: {e}")),
    }
}
