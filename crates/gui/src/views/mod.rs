//! Представления (views) приложения.

pub mod about;
pub mod archive;
pub mod download;
pub mod help;
pub mod logs;
pub mod main_window;
pub mod reports;
pub mod scheduler;
pub mod settings;
pub mod shop;

/// Открывает файл ассоциированным приложением (напр. Excel — для .xlsx).
/// Если файл не существует — возвращает ошибку (UI предложит «Перекачать»).
/// Общий хелпер для вкладок «Загрузка» и «Архив» (П.6).
pub(crate) fn open_file(path: &str) -> std::io::Result<()> {
    if !std::path::Path::new(path).exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "файл не найден (возможно, удалён/перемещён) — перекачайте",
        ));
    }
    #[cfg(target_os = "windows")]
    {
        // cmd /c start "" "<path>" — открывает ассоциированным приложением.
        std::process::Command::new("cmd")
            .args(["/C", "start", "", path])
            .spawn()?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("xdg-open").arg(path).spawn()?;
    }
    Ok(())
}

/// Открывает папку в проводнике (Windows). Общий хелпер (П.6).
///
/// Использует `cmd /c start "" <path>` (как open_file) — надёжнее прямого
/// `explorer <path>`, который при уже запущенном проводнике иногда открывает
/// 2 окна (handoff в работающий экземпляр).
pub(crate) fn open_folder(path: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", path])
            .spawn()?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
    }
    Ok(())
}

// ===== Кнопка-календарь для выбора дат (общая для «Загрузка» и «Архив») =====

/// Создаёт кнопку с иконкой календаря (MenuButton + Calendar в Popover).
/// При клике открывает календарь. Выбор даты записывает её в `entry`
/// в формате `date_format` (напр. "%Y-%m-%d" или "%Y-%m").
pub(crate) fn make_date_picker(entry: &gtk4::Entry, date_format: &str) -> gtk4::MenuButton {
    use std::cell::RefCell;
    use std::rc::Rc;

    use gtk4::prelude::*;

    let menu_btn = gtk4::MenuButton::builder()
        .icon_name("x-office-calendar-symbolic")
        .tooltip_text("Выбрать дату из календаря")
        .build();

    let calendar = gtk4::Calendar::builder()
        .show_day_names(true)
        .show_heading(true)
        .show_week_numbers(true)
        .build();

    let popover = gtk4::Popover::builder().build();

    // Предустановка календаря из текущего значения Entry (на момент постройки).
    let current_text = entry.text().to_string();
    if let Some(dt) = parse_date_for_calendar(&current_text) {
        calendar.select_day(&dt);
    }

    // Флаг подавления: программный select_day (синхронизация при открытии) тоже
    // стреляет day_selected, но НЕ должен писать в Entry / закрывать popover —
    // это делает только выбор даты пользователем.
    let suppressing = Rc::new(RefCell::new(false));

    // При выборе даты ПОЛЬЗОВАТЕЛЕМ — записываем в Entry и закрываем popover.
    let entry_clone = entry.clone();
    let fmt = date_format.to_string();
    let popover_clone = popover.clone();
    let suppressing_sel = suppressing.clone();
    calendar.connect_day_selected(move |cal| {
        if *suppressing_sel.borrow() {
            *suppressing_sel.borrow_mut() = false;
            return;
        }
        let selected = cal.date();
        if let Ok(formatted) = selected.format(&fmt) {
            entry_clone.set_text(&formatted);
            popover_clone.popdown();
        }
    });

    // При ОТКРЫТИИ popover синхронизируем календарь с ТЕКУЩИМ значением Entry —
    // дату в поле могли сменить извне (виджет интервала / restore) — календарь
    // должен открыться именно на ней, а не на construction-time дате.
    let entry_sync = entry.clone();
    let calendar_sync = calendar.clone();
    let suppressing_sync = suppressing.clone();
    popover.connect_notify_local(
        Some("visible"),
        move |popw: &gtk4::Popover, _pspec: &glib::ParamSpec| {
            if popw.is_visible() {
                if let Some(dt) = parse_date_for_calendar(&entry_sync.text()) {
                    *suppressing_sync.borrow_mut() = true;
                    calendar_sync.select_day(&dt); // синхронно стреляет day_selected (обработчик сбросит флаг)
                    *suppressing_sync.borrow_mut() = false; // гарантия сброса, если день не изменился
                }
            }
        },
    );

    // MenuButton управляет popover автоматически.
    popover.set_child(Some(&calendar));
    menu_btn.set_popover(Some(&popover));

    menu_btn
}

/// Парсит текст из Entry в glib::DateTime для предустановки календаря.
/// Поддерживает форматы YYYY-MM-DD и YYYY-MM.
fn parse_date_for_calendar(s: &str) -> Option<glib::DateTime> {
    let naive = if s.len() >= 10 {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?
    } else if s.len() == 7 {
        chrono::NaiveDate::parse_from_str(&format!("{s}-01"), "%Y-%m-%d").ok()?
    } else {
        return None;
    };
    let dt = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
        naive.and_hms_opt(12, 0, 0)?,
        chrono::Utc,
    );
    glib::DateTime::from_iso8601(&dt.format("%+").to_string(), None).ok()
}
