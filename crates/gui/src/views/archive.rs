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
    /// Диапазон дат фильтра архива [from, to] ("YYYY-MM-DD"). Источник — поля
    /// «С:…По:» (их заполняет виджет интервала или пользователь вручную/календарём).
    /// None = без фильтра по дате (все записи). Заменяет бывшие month/year combos.
    static DATE_RANGE: Rc<RefCell<Option<(String, String)>>> = Rc::new(RefCell::new(None));
    /// Поля произвольного интервала дат (source of truth для DATE_RANGE).
    static W_DATE_FROM: Rc<RefCell<Option<gtk4::Entry>>> = Rc::new(RefCell::new(None));
    static W_DATE_TO: Rc<RefCell<Option<gtk4::Entry>>> = Rc::new(RefCell::new(None));
    static W_LIST: Rc<RefCell<Option<gtk4::Grid>>> = Rc::new(RefCell::new(None));
    static W_RESULT: Rc<RefCell<Option<Label>>> = Rc::new(RefCell::new(None));
    /// Последний показанный набор записей (источник для кнопки «Экспорт»).
    static ENTRIES: Rc<RefCell<Vec<ArchiveEntry>>> = Rc::new(RefCell::new(Vec::new()));
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

    root.append(&crate::widgets::tab_help::title_row_with_help(
        "Архив скачанных документов",
        "title-2",
        ARCHIVE_HELP,
    ));
    root.append(
        &Label::builder()
            .label("Все скачанные файлы из локального каталога. Данные только на этом компьютере — сетевых запросов нет. Задайте фильтры и нажмите «Применить».")
            .css_classes(["dim-label"])
            .halign(gtk4::Align::Start)
            .wrap(true)
            .build(),
    );

    // --- Панель фильтров (ДВЕ строки: одна длинная строка задаёт окну
    // минимальную ширину ~1050px и не даёт уменьшать окно) ---
    let filters = GtkBox::new(Orientation::Vertical, 6);
    filters.set_margin_bottom(4);
    let filters_top = GtkBox::new(Orientation::Horizontal, 8);
    let filters_main = GtkBox::new(Orientation::Horizontal, 8);
    filters.append(&filters_top);
    filters.append(&filters_main);

    let profile_combo = ComboBoxText::new();
    profile_combo.set_tooltip_text(Some("Профиль (все — любой магазин)"));
    filters_top.append(&Label::new(Some("Профиль:")));
    filters_top.append(&profile_combo);

    let report_combo = ComboBoxText::new();
    report_combo.set_tooltip_text(Some("Тип отчёта"));
    filters_top.append(&Label::new(Some("Отчёт:")));
    filters_top.append(&report_combo);

    // Кнопки действий — в первой строке справа (во второй и так тесно от
    // полей дат; первая имеет запас ширины).
    let apply_btn = super::icon_button("Применить", "edit-find-symbolic");
    apply_btn.set_tooltip_text(Some("Применить фильтры и обновить список"));
    let export_btn = super::icon_button("Экспорт", "document-save-symbolic");
    export_btn.set_tooltip_text(Some("Сохранить показанный список в Excel или CSV"));
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    filters_top.append(&spacer);
    filters_top.append(&apply_btn);
    filters_top.append(&export_btn);

    // Фильтр по дате — ДВА способа задать интервал (единый источник правды —
    // поля «С:…—…По:» + DATE_RANGE):
    //   1) «📅 Интервал» — стандартный интервал (неделя/месяц/квартал/год);
    //   2) произвольный интервал: поля date_from/date_to + календари.
    // Выбор стандартного интервала заполняет поля; правка полей обновляет фильтр.
    // Совпадение: дата НАЧАЛА или КОНЦА отчёта внутри интервала
    // (см. Catalog::list_downloads_filtered).
    let date_from = gtk4::Entry::builder()
        .placeholder_text("с ДД.ММ.ГГГГ")
        .width_chars(11)
        .tooltip_text("Начало интервала отбора (дата начала/конца отчёта попадает сюда)")
        .build();
    let date_to = gtk4::Entry::builder()
        .placeholder_text("по ДД.ММ.ГГГГ")
        .width_chars(11)
        .tooltip_text("Конец интервала отбора")
        .build();

    let interval_btn = gtk4::MenuButton::new();
    interval_btn.set_child(Some(&super::icon_label_child("Интервал", "x-office-calendar-symbolic")));
    interval_btn.set_tooltip_text(Some("Стандартный интервал: неделя / месяц / квартал / год (заполнит поля ниже)"));
    let interval_popover = gtk4::Popover::new();
    {
        let pop = interval_popover.clone();
        let df = date_from.clone();
        let dt = date_to.clone();
        let picker = crate::widgets::interval_picker::make_interval_picker(move |f: &str, t: &str| {
            // Пишем в поля — их connect_changed обновит DATE_RANGE/лейбл/автосейв.
            df.set_text(f);
            dt.set_text(t);
            pop.popdown();
            // Автоприменение при выборе стандартного интервала.
            send_list_archive(selected_profile(), selected_report());
        });
        interval_popover.set_child(Some(&picker));
    }
    interval_btn.set_popover(Some(&interval_popover));

    let reset_btn = Button::builder()
        .label("✕ Дата")
        .tooltip_text("Сбросить фильтр даты (показать все)")
        .build();
    reset_btn.connect_clicked(move |_| {
        // Очистка полей → connect_changed выставит DATE_RANGE=None.
        if let Some(e) = W_DATE_FROM.with(|w| w.borrow().clone()) {
            e.set_text("");
        }
        if let Some(e) = W_DATE_TO.with(|w| w.borrow().clone()) {
            e.set_text("");
        }
        send_list_archive(selected_profile(), selected_report());
    });

    filters_main.append(&interval_btn);
    filters_main.append(&Label::new(Some("С:")));
    filters_main.append(&date_from);
    filters_main.append(&super::make_date_picker(&date_from, "%d.%m.%Y"));
    filters_main.append(&Label::new(Some("—")));
    filters_main.append(&date_to);
    filters_main.append(&super::make_date_picker(&date_to, "%d.%m.%Y"));
    filters_main.append(&reset_btn);
    root.append(&filters);

    // --- Результат/статус вкладки ---
    let result_label = Label::builder()
        .label("Загрузка архива…")
        .css_classes(["dim-label"])
        .halign(gtk4::Align::Start)
        .wrap(true)
        .build();
    root.append(&result_label);

    // --- Таблица архивных записей: GtkGrid (заголовок + все строки в ОДНОЙ
    // сетке) — колонки физически общие: заголовок всегда над своей колонкой,
    // линии-разделители стоят ровно. ---
    let grid = gtk4::Grid::new();
    grid.set_column_spacing(12);

    let scroll = ScrolledWindow::new();
    scroll.set_child(Some(&grid));
    scroll.set_policy(PolicyType::Automatic, PolicyType::Automatic);
    scroll.set_vexpand(true);
    root.append(&scroll);

    // Сохраняем виджеты в thread-local для обновления из хуков.
    CMD.with(|c| *c.borrow_mut() = Some(cs.clone()));
    W_PROFILE_COMBO.with(|w| *w.borrow_mut() = Some(profile_combo.clone()));
    W_REPORT_COMBO.with(|w| *w.borrow_mut() = Some(report_combo.clone()));
    W_DATE_FROM.with(|w| *w.borrow_mut() = Some(date_from.clone()));
    W_DATE_TO.with(|w| *w.borrow_mut() = Some(date_to.clone()));
    W_LIST.with(|w| *w.borrow_mut() = Some(grid));
    W_RESULT.with(|w| *w.borrow_mut() = Some(result_label));

    // Правка полей дат (вручную или календарём) → пересобираем DATE_RANGE.
    // Валидная пара [from ≤ to] → Some; иначе (пусто/невалидно) → None (все даты).
    // Запрос НЕ шлём — пользователь жмёт «🔍 Применить» (или сбрасывает «✕ Дата»).
    {
        let update = |e: &gtk4::Entry| {
            let _ = e;
            sync_date_range_from_entries();
        };
        date_from.connect_changed(move |e| update(e));
        date_to.connect_changed(move |e| update(e));
    }

    // «Применить»: собираем фильтры и шлём запрос каталога.
    {
        let cs = cs.clone();
        apply_btn.connect_clicked(move |_| {
            let profile = selected_profile();
            let report = selected_report();
            let date_range = selected_date_range();
            notify("Запрос архива…");
            cs.send(crate::channels::UiCommand::ListArchive {
                profile_name: profile,
                report_type: report,
                date_range,
            });
        });
    }

    // «Экспорт»: текущий список → xlsx/CSV через системный диалог Save.
    export_btn.connect_clicked(|_| export_current_list());

    // Автосохранение фильтров при смене combo. RESTORING защищает от сохранения
    // во время программного set_active при restore сохранённого состояния.
    profile_combo.connect_changed(|_| schedule_save());
    report_combo.connect_changed(|_| schedule_save());

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
            send_list_archive(None, None);
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
            // Диапазон дат (интервальный фильтр) — восстанавливаем в ПОЛЯ «С:…По:»;
            // их connect_changed пересоберёт DATE_RANGE (автосейв блокирован RESTORING).
            if let Some((f, t)) = &st.date_range {
                if let Some(e) = W_DATE_FROM.with(|w| w.borrow().clone()) {
                    e.set_text(&super::disp_date(f));
                }
                if let Some(e) = W_DATE_TO.with(|w| w.borrow().clone()) {
                    e.set_text(&super::disp_date(t));
                }
            }
            // Применяем восстановленный фильтр к списку.
            send_list_archive(st.profile_name.clone(), st.report_type.clone());
        }
    }

    RESTORING.with(|r| *r.borrow_mut() = false);
}

/// Рендерит таблицу Архива: заголовок и все строки — в ОДНОМ GtkGrid.
/// Колонки сетки физически общие на все строки: заголовок всегда стоит ровно
/// над своей колонкой, вертикальные линии-разделители — строго по границам
/// колонок (прежде строки были независимыми HBox — заголовки «уезжали»).
/// Раскладка: чётные колонки — данные (7 шт), нечётные — вертикальные линии;
/// строка 0 — заголовок, далее данные, между ними горизонтальные линии.
fn render_archive(entries: &[ArchiveEntry]) {
    // Памятка текущего списка — для кнопки «Экспорт».
    ENTRIES.with(|e| *e.borrow_mut() = entries.to_vec());
    W_LIST.with(|gw| {
        // Колонки данных (чётные): Профиль, Отчёт, Период, Формат, Размер,
        // Скачан, Действия; между ними (нечётные) — вертикальные линии.
        const CELLS: [i32; 7] = [0, 2, 4, 6, 8, 10, 12];
        const SPAN: i32 = 13;

        // Прикрепить ячейку в строку row (с внутренними отступами строки).
        fn put(grid: &gtk4::Grid, w: &impl IsA<gtk4::Widget>, col: i32, row: i32) {
            w.set_margin_top(3);
            w.set_margin_bottom(3);
            grid.attach(w, col, row, 1, 1);
        }
        // Вертикальная линия между колонками (вся высота строки).
        fn vline(grid: &gtk4::Grid, col: i32, row: i32) {
            let sep = vsep();
            grid.attach(&sep, col, row, 1, 1);
        }
        // Горизонтальная линия на всю ширину таблицы.
        fn hline(grid: &gtk4::Grid, row: i32) {
            let sep = gtk4::Separator::new(gtk4::Orientation::Horizontal);
            grid.attach(&sep, 0, row, SPAN, 1);
        }

        let Some(grid) = gw.borrow().clone() else {
            return;
        };
        grid.set_margin_start(8);
        grid.set_margin_end(8);
        grid.set_margin_top(4);
        grid.set_margin_bottom(4);
        // Очищаем старое содержимое.
        while let Some(child) = grid.first_child() {
            grid.remove(&child);
        }

        if entries.is_empty() {
            grid.attach(
                &Label::new(Some("Ничего не найдено по заданным фильтрам.")),
                0,
                0,
                1,
                1,
            );
            return;
        }

        // --- Строка 0: заголовок (жирный; ширина ячеек та же, что у данных). ---
        let head = |text: &str, w: i32, expand: bool| {
            Label::builder()
                .label(format!("<b>{text}</b>"))
                .use_markup(true)
                .width_chars(w)
                .max_width_chars(w)
                .xalign(0.0)
                .hexpand(expand)
                .build()
        };
        // Все заголовки — от левого края своей колонки (одинаковое
        // выравнивание); «Формат» выравнивается по иконке в данных.
        put(&grid, &head("Профиль", 16, false), CELLS[0], 0);
        vline(&grid, CELLS[0] + 1, 0);
        // «Отчёт» — эластичная колонка (hexpand и в заголовке, и в данных):
        // лишняя ширина окна уходит в неё, остальные колонки стоят на месте.
        put(&grid, &head("Отчёт", 22, true), CELLS[1], 0);
        vline(&grid, CELLS[1] + 1, 0);
        put(&grid, &head("Период", 10, false), CELLS[2], 0);
        vline(&grid, CELLS[2] + 1, 0);
        put(&grid, &head("Формат", 8, false), CELLS[3], 0);
        vline(&grid, CELLS[3] + 1, 0);
        put(&grid, &head("Размер", 10, false), CELLS[4], 0);
        vline(&grid, CELLS[4] + 1, 0);
        put(&grid, &head("Скачан", 16, false), CELLS[5], 0);
        vline(&grid, CELLS[5] + 1, 0);
        put(&grid, &head("Действия", 0, false), CELLS[6], 0);
        hline(&grid, 1);

        let mut row = 2i32;
        for e in entries {
            // Профиль — имя (фиксированная ширина, длинные — с обрезкой).
            put(
                &grid,
                &Label::builder()
                    .label(&e.profile_name)
                    .width_chars(16)
                    .max_width_chars(16)
                    .xalign(0.0)
                    .ellipsize(gtk4::pango::EllipsizeMode::End)
                    .build(),
                CELLS[0],
                row,
            );
            vline(&grid, CELLS[0] + 1, row);

            // Человекочитаемое имя отчёта (с fallback на type_id); tooltip —
            // технический type_id для точной идентификации.
            let report_label = e
                .report_display_name
                .clone()
                .unwrap_or_else(|| e.report_type.clone());
            put(
                &grid,
                &Label::builder()
                    .label(&report_label)
                    .tooltip_text(&e.report_type)
                    .width_chars(22)
                    .max_width_chars(22)
                    .xalign(0.0)
                    .hexpand(true)
                    .ellipsize(gtk4::pango::EllipsizeMode::End)
                    .build(),
                CELLS[1],
                row,
            );
            vline(&grid, CELLS[1] + 1, row);

            let period_str = e
                .period
                .as_deref()
                .map_or_else(|| "—".to_string(), super::disp_date);
            put(
                &grid,
                &Label::builder()
                    .label(&period_str)
                    .width_chars(10)
                    .max_width_chars(10)
                    .xalign(0.0)
                    .build(),
                CELLS[2],
                row,
            );
            vline(&grid, CELLS[2] + 1, row);

            // Формат: иконка типа файла (PNG из gresource) + короткий текст.
            let fmt_box = GtkBox::new(Orientation::Horizontal, 6);
            fmt_box.append(
                &Image::builder()
                    .resource(ext_icon_resource(&e.file_format))
                    .pixel_size(20)
                    .build(),
            );
            fmt_box.append(
                &Label::builder()
                    .label(super::ext_label(&e.file_format))
                    .width_chars(8)
                    .max_width_chars(8)
                    .xalign(0.0)
                    .build(),
            );
            put(&grid, &fmt_box, CELLS[3], row);
            vline(&grid, CELLS[3] + 1, row);

            let size_str = human_size(u64::try_from(e.file_size).unwrap_or(0));
            put(
                &grid,
                &Label::builder()
                    .label(&size_str)
                    .width_chars(10)
                    .max_width_chars(10)
                    .xalign(0.0)
                    .build(),
                CELLS[4],
                row,
            );
            vline(&grid, CELLS[4] + 1, row);

            let dt_str = e
                .downloaded_at
                .with_timezone(&chrono::Local)
                .format("%d.%m.%Y %H:%M")
                .to_string();
            put(
                &grid,
                &Label::builder()
                    .label(&dt_str)
                    .width_chars(16)
                    .max_width_chars(16)
                    .xalign(0.0)
                    .build(),
                CELLS[5],
                row,
            );
            vline(&grid, CELLS[5] + 1, row);

            // Действия: 📂 открыть, 📁 папка, 📋 путь, 🗑 удалить — компактной
            // группой (ширина колонки — по кнопкам).
            let actions_box = GtkBox::new(Orientation::Horizontal, 2);
            actions_box.set_halign(gtk4::Align::Start);
            let file_path = e.file_path.clone();

            let open_btn = super::icon_only_button("document-open-symbolic", "Открыть файл");
            open_btn.set_tooltip_text(Some("Открыть файл"));
            {
                let path = file_path.clone();
                open_btn.connect_clicked(move |_| {
                    if let Err(err) = crate::views::open_file(&path) {
                        notify(&format!("Не удалось открыть: {err}"));
                    }
                });
            }
            actions_box.append(&open_btn);

            let folder_btn = super::icon_only_button("folder-open-symbolic", "Открыть папку с файлом");
            folder_btn.set_tooltip_text(Some("Открыть папку с файлом"));
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

            let copy_btn = super::icon_only_button("edit-copy-symbolic", "Копировать путь в буфер обмена");
            copy_btn.set_tooltip_text(Some("Копировать путь в буфер обмена"));
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
            let del_btn = super::icon_only_button("user-trash-symbolic", "Удалить запись и файл");
            del_btn.add_css_class("destructive-action");
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

            put(&grid, &actions_box, CELLS[6], row);

            row += 1;
            if row < 2 + entries.len() as i32 {
                hline(&grid, row);
                row += 1;
            }
        }
    });
}

/// «Экспорт»: сохранить текущий список Архива в xlsx или CSV (диалог Save;
/// формат — по расширению выбранного имени файла).
fn export_current_list() {
    let entries = ENTRIES.with(|e| e.borrow().clone());
    if entries.is_empty() {
        notify("Нечего экспортировать: список пуст.");
        return;
    }
    let dlg = gtk4::FileChooserDialog::builder()
        .title("Экспорт списка Архива")
        .action(gtk4::FileChooserAction::Save)
        .modal(true)
        .build();
    dlg.add_button("Отмена", gtk4::ResponseType::Cancel);
    dlg.add_button("Сохранить", gtk4::ResponseType::Accept);

    let xlsx_filter = gtk4::FileFilter::new();
    xlsx_filter.set_name(Some("Таблица Excel (.xlsx)"));
    xlsx_filter.add_pattern("*.xlsx");
    dlg.add_filter(&xlsx_filter);
    let csv_filter = gtk4::FileFilter::new();
    csv_filter.set_name(Some("CSV для Excel (разделитель «;»)"));
    csv_filter.add_pattern("*.csv");
    dlg.add_filter(&csv_filter);

    dlg.set_current_name(&format!(
        "mdwf-архив-{}.xlsx",
        chrono::Local::now().format("%Y-%m-%d")
    ));
    // Стартовая папка — «Документы» (рядом обычно лежат выгрузки).
    if let Some(home) = std::env::var_os("USERPROFILE") {
        let docs = std::path::Path::new(&home).join("Documents");
        let _ = dlg.set_current_folder(Some(&gtk4::gio::File::for_path(docs)));
    }

    dlg.connect_response(move |d, resp| {
        if resp == gtk4::ResponseType::Accept {
            if let Some(path) = d.file().and_then(|f| f.path()) {
                let is_csv = path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("csv"));
                let data = if is_csv {
                    Ok(super::archive_export::to_csv(&entries).into_bytes())
                } else {
                    super::archive_export::to_xlsx(&entries)
                };
                match data.and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
                    Ok(()) => notify(&format!("Список сохранён: {}", path.display())),
                    Err(e) => notify(&format!("Не удалось сохранить: {e}")),
                }
            }
        }
        d.destroy();
    });
    dlg.show();
}

/// Возвращает выбранный профиль (None = «(все)»).
fn selected_profile() -> Option<String> {    let combo = W_PROFILE_COMBO.with(|w| w.borrow().clone())?;
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

/// Возвращает выбранный диапазон дат фильтра `[from, to]` (None = без фильтра по
/// дате). Берётся из DATE_RANGE (синхронизируется с полями «С:…По:»).
fn selected_date_range() -> Option<(String, String)> {
    DATE_RANGE.with(|d| d.borrow().clone())
}

/// Пересобирает DATE_RANGE из текущих значений полей «С:…По:».
/// Валидная пара дат (from ≤ to) → Some; пусто/невалидно → None (все даты).
/// Также автосохраняет состояние фильтров.
fn sync_date_range_from_entries() {
    let from = W_DATE_FROM.with(|w| w.borrow().as_ref().map(|e| e.text().to_string()));
    let to = W_DATE_TO.with(|w| w.borrow().as_ref().map(|e| e.text().to_string()));
    let range = match (from, to) {
        (Some(f), Some(t)) => match (super::parse_date_flex(&f), super::parse_date_flex(&t)) {
            (Some(fd), Some(td)) if fd <= td => Some((
                fd.format("%Y-%m-%d").to_string(),
                td.format("%Y-%m-%d").to_string(),
            )),
            _ => None,
        },
        _ => None,
    };
    DATE_RANGE.with(|d| *d.borrow_mut() = range);
    schedule_save();
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

/// Вертикальная линия-разделитель колонок Архива (высотой в строку):
/// сетка таблицы «рамками» — горизонтали даёт ListBox (show_separators),
/// вертикали — эти сепараторы между ячейками (заголовок и строки alike).
fn vsep() -> gtk4::Separator {
    let s = gtk4::Separator::new(gtk4::Orientation::Vertical);
    s.set_valign(gtk4::Align::Fill);
    s
}

/// Человекочитаемый размер («1,2 МБ», «456 Б»). Используется и экспортом
/// Архива (archive_export) — формат в файле совпадает с таблицей на экране.
pub(crate) fn human_size(bytes: u64) -> String {
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
        date_range: selected_date_range(),
    }));
}

/// Шлёт ListArchive с заданными профилем/отчётом + текущим диапазоном дат из
/// виджета интервала (DATE_RANGE).
fn send_list_archive(profile: Option<String>, report: Option<String>) {
    let Some(cs) = CMD.with(|c| c.borrow().clone()) else {
        return;
    };
    cs.send(crate::channels::UiCommand::ListArchive {
        profile_name: profile,
        report_type: report,
        date_range: selected_date_range(),
    });
}

// Восстанавливать combo периода больше не нужно: month/year combos удалены,
// фильтр архива теперь — интервал дат (DATE_RANGE), восстанавливаемый в
// on_archive_state_loaded напрямую из st.date_range.

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
            // Refresh списка с текущими фильтрами.
            send_list_archive(selected_profile(), selected_report());
        }
        Err(e) => notify(&format!("Ошибка удаления: {e}")),
    }
}

/// Контекстная помощь вкладки «Архив» (кнопка «?» в заголовке).
const ARCHIVE_HELP: &[crate::widgets::tab_help::HelpBlock] = &[
    crate::widgets::tab_help::HelpBlock::H("Что здесь"),
    crate::widgets::tab_help::HelpBlock::T("Все скачанные файлы из локального каталога. Работает офлайн — без сети и ключей."),
    crate::widgets::tab_help::HelpBlock::H("Фильтры"),
    crate::widgets::tab_help::HelpBlock::B(&[
        "Профиль и отчёт — точный выбор или «(все)».",
        "Интервал дат: кнопка «📅 Интервал» (неделя/месяц/квартал/год) или поля «С:/По:» с календарями.",
        "«✕ Дата» — сбросить фильтр даты; «🔍 Применить» — обновить список.",
    ]),
    crate::widgets::tab_help::HelpBlock::H("Правило отбора по дате"),
    crate::widgets::tab_help::HelpBlock::T("Отчёт попадает в выборку, если дата его <b>начала или конца</b> входит в выбранный интервал."),
    crate::widgets::tab_help::HelpBlock::H("Действия"),
    crate::widgets::tab_help::HelpBlock::B(&[
        "📂 — открыть файл; 📁 — показать папку; 📋 — копировать путь.",
        "🗑 — удалить запись и файл с диска (с подтверждением).",
    ]),
    crate::widgets::tab_help::HelpBlock::H("Экспорт"),
    crate::widgets::tab_help::HelpBlock::T(
        "Кнопка «Экспорт» сохраняет показанный (с учётом фильтров) список в таблицу Excel (.xlsx) или CSV. \
         Формат — по расширению имени файла в диалоге; в файле те же колонки, что на экране, плюс путь к каждому файлу.",
    ),
];
