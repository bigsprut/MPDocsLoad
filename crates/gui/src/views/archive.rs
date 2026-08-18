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
    static W_LIST: Rc<RefCell<Option<gtk4::ColumnView>>> = Rc::new(RefCell::new(None));
    /// Модель строк таблицы архива (ListStore из BoxedAnyObject(ArchiveEntry)).
    /// Живёт постоянно: обновление списка = refill модели, а не пересборка
    /// виджета — ширины колонок, выставленные перетаскиванием, переживают
    /// обновление данных.
    static W_STORE: Rc<RefCell<Option<gtk4::gio::ListStore>>> = Rc::new(RefCell::new(None));
    static W_RESULT: Rc<RefCell<Option<Label>>> = Rc::new(RefCell::new(None));
    /// Скролл таблицы: для сохранения позиции прокрутки при удалении записи
    /// (список перерисовывается — без восстановления прокрутка уходит в начало).
    static W_SCROLL: Rc<RefCell<Option<ScrolledWindow>>> = Rc::new(RefCell::new(None));
    /// Флаг «после перерисовки вернуть прокрутку на прежнее место»
    /// (ставится перед refresh после удаления записи).
    static PRESERVE_SCROLL: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
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
    interval_btn.set_tooltip_text(Some("Стандартный интервал: месяц / квартал / полугодие / год (заполнит поля ниже)"));
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
        interval_popover.set_child(Some(&picker.widget));
        // При открытии — позиционируем виджет на текущий период полей дат.
        {
            let sync = picker.sync.clone();
            let df = date_from.clone();
            let dt = date_to.clone();
            interval_popover.connect_notify_local(Some("visible"), move |popw, _| {
                if popw.is_visible() {
                    if let (Some(f), Some(t)) = (
                        super::parse_date_flex(&df.text()),
                        super::parse_date_flex(&dt.text()),
                    ) {
                        sync(f, t);
                    }
                }
            });
        }
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

    // --- Таблица архивных записей: GtkColumnView. Границы колонок тянутся
    // мышью (set_resizable), заголовки встроены и всегда над своими колонками;
    // при расширении колонки обрезанный текст показывается целиком (у ячеек
    // нет капа ширины — natural size по содержимому). Виджет и модель живут
    // постоянно, обновление данных = refill модели. ---
    let store = gtk4::gio::ListStore::new::<glib::BoxedAnyObject>();
    let view = make_archive_table(store.clone());
    view.set_margin_start(8);
    view.set_margin_end(8);
    view.set_margin_top(4);
    view.set_margin_bottom(4);

    let scroll = ScrolledWindow::new();
    scroll.set_child(Some(&view));
    scroll.set_policy(PolicyType::Automatic, PolicyType::Automatic);
    scroll.set_vexpand(true);
    root.append(&scroll);
    W_SCROLL.with(|w| *w.borrow_mut() = Some(scroll.clone()));

    // Сохраняем виджеты в thread-local для обновления из хуков.
    CMD.with(|c| *c.borrow_mut() = Some(cs.clone()));
    W_PROFILE_COMBO.with(|w| *w.borrow_mut() = Some(profile_combo.clone()));
    W_REPORT_COMBO.with(|w| *w.borrow_mut() = Some(report_combo.clone()));
    W_DATE_FROM.with(|w| *w.borrow_mut() = Some(date_from.clone()));
    W_DATE_TO.with(|w| *w.borrow_mut() = Some(date_to.clone()));
    W_STORE.with(|w| *w.borrow_mut() = Some(store));
    W_LIST.with(|w| *w.borrow_mut() = Some(view));
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

/// Заполняет таблицу Архива данными. Виджет/колонки/модель живут постоянно —
/// ширины колонок, выставленные перетаскиванием границ, переживают обновление
/// списка; при удалении записи сохраняем и возвращаем позицию прокрутки.
fn render_archive(entries: &[ArchiveEntry]) {
    // Памятка текущего списка — для кнопки «Экспорт».
    ENTRIES.with(|e| *e.borrow_mut() = entries.to_vec());
    // Прокрутка: при удалении записи (флаг) сохраняем позицию и возвращаем
    // после обновления — иначе список прыгает в начало и место удаления
    // приходится искать заново.
    let saved_scroll = if PRESERVE_SCROLL.with(|p| p.replace(false)) {
        W_SCROLL.with(|w| {
            w.borrow()
                .as_ref()
                .map(gtk4::ScrolledWindow::vadjustment)
                .map(|adj| adj.value())
        })
    } else {
        None
    };
    let view = W_LIST.with(|w| w.borrow().clone());
    let store = W_STORE.with(|w| w.borrow().clone());
    let scroll = W_SCROLL.with(|w| w.borrow().clone());
    let (Some(view), Some(store), Some(scroll)) = (view, store, scroll) else {
        return;
    };
    store.remove_all();
    if entries.is_empty() {
        scroll.set_child(Some(&Label::new(Some(
            "Ничего не найдено по заданным фильтрам.",
        ))));
    } else {
        scroll.set_child(Some(&view));
        for e in entries {
            store.append(&glib::BoxedAnyObject::new(e.clone()));
        }
    }
    // Возврат прокрутки после обновления (после пересчёта раскладки — в idle;
    // set_value сам клампится в допустимый диапазон).
    if let Some(v) = saved_scroll {
        let adj = scroll.vadjustment();
        glib::source::idle_add_local_once(move || {
            adj.set_value(v);
        });
    }
}

// ===== Таблица архива: GtkColumnView с перетаскиваемыми границами колонок =====

/// Извлекает ArchiveEntry из элемента модели.
fn entry_of(item: &glib::Object) -> ArchiveEntry {
    item.clone()
        .downcast::<glib::BoxedAnyObject>()
        .expect("элемент модели архива")
        .borrow::<ArchiveEntry>()
        .clone()
}

/// Текстовая колонка. Ячейка — Label с обрезкой («…») и ПОЛНЫМ текстом в
/// подсказке; капа ширины у ячейки нет — при расширении колонки текст
/// показывается целиком, начальная ширина задаётся fixed_width.
fn text_column(
    title: &str,
    width: Option<i32>,
    expand: bool,
    cell: impl Fn(&ArchiveEntry) -> (String, Option<String>) + 'static,
) -> gtk4::ColumnViewColumn {
    let factory = gtk4::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let label = Label::builder()
            .xalign(0.0)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .margin_top(4)
            .margin_bottom(4)
            .build();
        item.set_child(Some(&label));
    });
    factory.connect_bind(move |_, item| {
        let e = entry_of(&item.item().expect("item модели"));
        let (text, tooltip) = cell(&e);
        let label = item
            .child()
            .and_downcast::<Label>()
            .expect("Label колонки архива");
        label.set_label(&text);
        match tooltip {
            Some(t) => label.set_tooltip_text(Some(&t)),
            None => label.set_tooltip_text(None),
        }
    });
    let col = gtk4::ColumnViewColumn::new(Some(title), Some(factory));
    col.set_resizable(true);
    if let Some(w) = width {
        col.set_fixed_width(w);
    }
    col.set_expand(expand);
    col
}

/// Колонка «Формат»: иконка типа файла (PNG из gresource) + короткий текст.
fn format_column() -> gtk4::ColumnViewColumn {
    let factory = gtk4::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let b = GtkBox::new(Orientation::Horizontal, 6);
        b.set_margin_top(4);
        b.set_margin_bottom(4);
        b.append(&Image::new());
        b.append(&Label::builder().xalign(0.0).build());
        item.set_child(Some(&b));
    });
    factory.connect_bind(|_, item| {
        let e = entry_of(&item.item().expect("item модели"));
        let b = item
            .child()
            .and_downcast::<GtkBox>()
            .expect("Box колонки формата");
        let img = b
            .first_child()
            .and_downcast::<Image>()
            .expect("иконка формата");
        let lbl = img
            .next_sibling()
            .and_downcast::<Label>()
            .expect("подпись формата");
        img.set_resource(Some(ext_icon_resource(&e.file_format)));
        lbl.set_label(&super::ext_label(&e.file_format));
    });
    let col = gtk4::ColumnViewColumn::new(Some("Формат"), Some(factory));
    col.set_resizable(true);
    col.set_fixed_width(120);
    col
}

/// Колонка «Действия»: 📂 открыть, 📁 папка, 📋 путь, 🔗 в ЛК, 🗑 удалить.
/// Кнопки пересобираются при каждой привязке записи (list items пере-
/// используются при прокрутке).
fn actions_column() -> gtk4::ColumnViewColumn {
    let factory = gtk4::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let b = GtkBox::new(Orientation::Horizontal, 2);
        b.set_halign(gtk4::Align::Start);
        b.set_margin_top(4);
        b.set_margin_bottom(4);
        item.set_child(Some(&b));
    });
    factory.connect_bind(|_, item| {
        let e = entry_of(&item.item().expect("item модели"));
        let b = item
            .child()
            .and_downcast::<GtkBox>()
            .expect("Box действий архива");
        while let Some(child) = b.first_child() {
            b.remove(&child);
        }
        build_action_buttons(&b, &e);
    });
    let col = gtk4::ColumnViewColumn::new(Some("Действия"), Some(factory));
    col.set_fixed_width(220);
    col
}

/// Кнопки действий строки архива (колонка «Действия»).
fn build_action_buttons(b: &gtk4::Box, e: &ArchiveEntry) {
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
    b.append(&open_btn);

    let folder_btn = super::icon_only_button("folder-symbolic", "Открыть папку с файлом");
    folder_btn.set_tooltip_text(Some("Открыть папку с файлом"));
    {
        let path = file_path.clone();
        folder_btn.connect_clicked(move |_| {
            let folder = std::path::Path::new(&path)
                .parent()
                .map_or_else(|| path.clone(), |p| p.to_string_lossy().to_string());
            if let Err(err) = crate::views::open_folder(&folder) {
                notify(&format!("Не удалось открыть папку: {err}"));
            }
        });
    }
    b.append(&folder_btn);

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
    b.append(&copy_btn);

    // 🔗 Открыть раздел отчёта в ЛК — только если у отчёта есть ссылка
    // (все 21 Ozon; у WB ссылок нет — кнопка не показывается).
    if let Some(url) = e.cabinet_url.clone() {
        let lk_btn = super::icon_only_button("insert-link-symbolic", "Открыть в ЛК");
        lk_btn.set_tooltip_text(Some("Открыть раздел этого отчёта в личном кабинете"));
        lk_btn.connect_clicked(move |_| {
            if let Err(err) = super::open_url(&url) {
                eprintln!("open_url: {err}");
                super::show_url_error(&url, &err);
            }
        });
        b.append(&lk_btn);
    }

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
    b.append(&del_btn);
}

/// Собирает таблицу архива: ColumnView + 7 колонок. Границы колонок тянутся
/// мышью; «Отчёт» — эластичная, без фиксированной ширины: полное название
/// видно целиком, когда окно достаточно широкое.
fn make_archive_table(store: gtk4::gio::ListStore) -> gtk4::ColumnView {
    let selection = gtk4::NoSelection::new(Some(store));
    let view = gtk4::ColumnView::new(Some(selection));

    // Профиль: полное имя — в подсказке (колонка узкая, имя обрезается).
    view.append_column(&text_column("Профиль", Some(150), false, |e| {
        (e.profile_name.clone(), Some(e.profile_name.clone()))
    }));
    // Отчёт: без фиксированной ширины + expand — лишняя ширина окна уходит
    // сюда; полное название + технический type_id — в подсказке.
    view.append_column(&text_column("Отчёт", None, true, |e| {
        let name = e
            .report_display_name
            .clone()
            .unwrap_or_else(|| e.report_type.clone());
        let tip = format!("{name}\n({})", e.report_type);
        (name, Some(tip))
    }));
    view.append_column(&text_column("Период", Some(95), false, |e| {
        (
            e.period
                .as_deref()
                .map_or_else(|| "—".to_string(), super::disp_date),
            None,
        )
    }));
    view.append_column(&format_column());
    view.append_column(&text_column("Размер", Some(90), false, |e| {
        (human_size(u64::try_from(e.file_size).unwrap_or(0)), None)
    }));
    view.append_column(&text_column("Скачан", Some(150), false, |e| {
        (
            e.downloaded_at
                .with_timezone(&chrono::Local)
                .format("%d.%m.%Y %H:%M")
                .to_string(),
            None,
        )
    }));
    view.append_column(&actions_column());
    view
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
            // Refresh списка с текущими фильтрами; прокрутку сохранить —
            // пользователь удаляет записи подряд и не должен искать место.
            PRESERVE_SCROLL.with(|p| *p.borrow_mut() = true);
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
