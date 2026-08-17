//! Представления (views) приложения.

use gtk4::prelude::*;

/// Контейнер «symbolic-иконка + текст» для кнопок: монохром, темизация (тёмная/
/// светлая), масштабирование с UI — вместо эмодзи (цветные, не темизуются).
pub(crate) fn icon_label_child(label: &str, icon_name: &str) -> gtk4::Box {
    let b = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    b.append(&gtk4::Image::from_icon_name(icon_name));
    b.append(&gtk4::Label::new(Some(label)));
    b
}

/// Кнопка «иконка + текст» на Adwaita symbolic-иконке (иконки поставляются
/// в бандле: share/icons/Adwaita/symbolic — офлайн, одинаковый вид везде).
pub(crate) fn icon_button(label: &str, icon_name: &str) -> gtk4::Button {
    let btn = gtk4::Button::new();
    btn.set_child(Some(&icon_label_child(label, icon_name)));
    btn
}

/// Иконочная кнопка БЕЗ текста — для компактных строк списков.
/// По GNOME-конвенции требует tooltip (иначе непонятна — см. UX-ревью архива).
pub(crate) fn icon_only_button(icon_name: &str, tooltip: &str) -> gtk4::Button {
    let btn = gtk4::Button::new();
    btn.set_child(Some(&gtk4::Image::from_icon_name(icon_name)));
    btn.set_tooltip_text(Some(tooltip));
    btn
}

pub mod about;
pub mod archive;
pub(crate) mod archive_export;
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

/// Человекочитаемое название типа файла по расширению (для списков UI):
/// `xlsx`→«Excel», `pdf`→«PDF», прочие — в верхнем регистре. Обычному пользователю
/// «Excel/PDF» понятнее сырых расширений.
pub(crate) fn ext_label(ext: &str) -> String {
    match ext.to_ascii_lowercase().as_str() {
        "xlsx" | "xls" | "xlsm" => "Excel".to_string(),
        "csv" => "CSV".into(),
        "pdf" => "PDF".into(),
        "xml" => "XML".into(),
        "zip" => "ZIP".into(),
        "json" => "JSON".into(),
        "txt" => "TXT".into(),
        other => other.to_uppercase(),
    }
}

/// Дата для UI: ISO «2026-07-15» → «15.07.2026», месяц «2026-07» → «07.2026»;
/// прочее — как есть. Российский формат привычнее ISO для бухгалтера.
pub(crate) fn disp_date(iso: &str) -> String {
    if let Ok(d) = chrono::NaiveDate::parse_from_str(iso, "%Y-%m-%d") {
        return d.format("%d.%m.%Y").to_string();
    }
    let t = iso.trim();
    if t.len() == 7 {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(&format!("{t}-01"), "%Y-%m-%d") {
            return d.format("%m.%Y").to_string();
        }
    }
    iso.to_string()
}

/// Парсит дату из поля ввода: принимает «ДД.ММ.ГГГГ» и (для совместимости) «YYYY-MM-DD».
pub(crate) fn parse_date_flex(s: &str) -> Option<chrono::NaiveDate> {
    let t = s.trim();
    chrono::NaiveDate::parse_from_str(t, "%d.%m.%Y")
        .or_else(|_| chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d"))
        .ok()
}

/// Дата из поля ввода → ISO «YYYY-MM-DD» для API. При ошибке парсинга — исходная
/// строка (провайдер/валидация сообщит о некорректности).
pub(crate) fn to_iso(s: &str) -> String {
    match parse_date_flex(s) {
        Some(d) => d.format("%Y-%m-%d").to_string(),
        None => s.trim().to_string(),
    }
}

// describe_range/describe_report_period (описание периода для UI и журнала)
// живут в mdwf-core::journal — общий словарь для GUI и CLI. Здесь — реэкспорт,
// чтобы вызовы внутри views не менялись.
pub(crate) use mdwf_core::journal::{describe_range, describe_report_period};

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
    // Флаг навигации по месяцам/годам: GTK при смене месяца САМ переносит
    // выделение на тот же номер дня в новом месяце и стреляет day_selected —
    // это не выбор пользователя (жалоба: «после смены января на февраль 28-е
    // не выбирается» — клик по уже «выбранному» номеру дня сигнала не даёт).
    // После навигации сбрасываем выделение на 1-е число показанного месяца,
    // а «мёртвые» клики (по уже выделенному дню) ловит GestureClick-фоллбэк.
    let navigating = Rc::new(RefCell::new(false));
    // Клик уже обработан (day_selected применил дату) — фоллбэку делать нечего.
    let applied = Rc::new(RefCell::new(false));

    // При выборе даты ПОЛЬЗОВАТЕЛЕМ — записываем в Entry и закрываем popover.
    let entry_clone = entry.clone();
    let fmt = date_format.to_string();
    let popover_clone = popover.clone();
    let suppressing_sel = suppressing.clone();
    let navigating_sel = navigating.clone();
    let applied_sel = applied.clone();
    calendar.connect_day_selected(move |cal| {
        if *suppressing_sel.borrow() {
            *suppressing_sel.borrow_mut() = false;
            return;
        }
        if *navigating_sel.borrow() {
            // Перенос выделения при навигации — не выбор (сбросит idle ниже).
            return;
        }
        let selected = cal.date();
        if let Ok(formatted) = selected.format(&fmt) {
            entry_clone.set_text(&formatted);
            popover_clone.popdown();
            *applied_sel.borrow_mut() = true;
        }
    });

    // Навигация (стрелки месяца/года в шапке календаря): после неё в idle —
    // сброс выделения на 1-е число показанного месяца (подавленно), иначе
    // «старый» номер дня в новом месяце не кликается.
    {
        let calendar_nav = calendar.clone();
        let suppressing_nav = suppressing.clone();
        let navigating_nav = navigating.clone();
        let on_nav = move || {
            *navigating_nav.borrow_mut() = true;
            let cal = calendar_nav.clone();
            let sup = suppressing_nav.clone();
            let nav = navigating_nav.clone();
            glib::source::idle_add_local_once(move || {
                let d = cal.date();
                let first = naive_to_calendar_date(d.year(), d.month() as u32, 1);
                if let Some(first) = first {
                    *sup.borrow_mut() = true;
                    cal.select_day(&first);
                    *sup.borrow_mut() = false;
                }
                *nav.borrow_mut() = false;
            });
        };
        calendar.connect_next_month({
            let f = on_nav.clone();
            move |_| f()
        });
        calendar.connect_prev_month({
            let f = on_nav.clone();
            move |_| f()
        });
        calendar.connect_next_year({
            let f = on_nav.clone();
            move |_| f()
        });
        calendar.connect_prev_year({
            let f = on_nav;
            move |_| f()
        });
    }

    // Фоллбэк для «мёртвых» кликов: клик по УЖЕ выделенному дню не стреляет
    // day_selected — применяем текущую дату вручную. Пиксельный фильтр:
    // реагируем только на область дней (ниже шапки с месяцем и строки
    // названий дней, правее колонки номеров недель).
    {
        let entry_fb = entry.clone();
        let popover_fb = popover.clone();
        let cal_fb = calendar.clone();
        let fmt_fb = date_format.to_string();
        let navigating_fb = navigating.clone();
        let applied_fb = applied.clone();
        let click = gtk4::GestureClick::new();
        click.set_button(gtk4::gdk::BUTTON_PRIMARY);
        click.connect_pressed({
            let applied_p = applied.clone();
            move |_, _, _, _| {
                *applied_p.borrow_mut() = false;
            }
        });
        click.connect_released(move |_, _n, x, y| {
            if *applied_fb.borrow() || *navigating_fb.borrow() {
                *applied_fb.borrow_mut() = false;
                return;
            }
            // Область дней: ниже шапки+строки дней (~56px), правее
            // номеров недель (~30px).
            if y < 56.0 || x < 30.0 {
                return;
            }
            let selected = cal_fb.date();
            if let Ok(formatted) = selected.format(&fmt_fb) {
                entry_fb.set_text(&formatted);
                popover_fb.popdown();
            }
        });
        calendar.add_controller(click);
    }

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

/// Открывает URL в браузере по ассоциации системы (кнопки «Открыть в ЛК»).
pub(crate) fn open_url(url: &str) -> Result<(), String> {
    gtk4::gio::AppInfo::launch_default_for_uri(
        url,
        None::<&gtk4::gio::AppLaunchContext>,
    )
        .map_err(|e| e.to_string())
}

/// Парсит текст из Entry в glib::DateTime для предустановки календаря.
/// Поддерживает ДД.ММ.ГГГГ, YYYY-MM-DD (совместимость) и YYYY-MM.
fn parse_date_for_calendar(s: &str) -> Option<glib::DateTime> {
    let naive = parse_date_flex(s).or_else(|| {
        let t = s.trim();
        if t.len() == 7 {
            chrono::NaiveDate::parse_from_str(&format!("{t}-01"), "%Y-%m-%d").ok()
        } else {
            None
        }
    })?;
    naive_to_calendar(naive)
}

/// chrono::NaiveDate → glib::DateTime (полдень UTC — как показывает календарь).
fn naive_to_calendar(naive: chrono::NaiveDate) -> Option<glib::DateTime> {
    let dt = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
        naive.and_hms_opt(12, 0, 0)?,
        chrono::Utc,
    );
    glib::DateTime::from_iso8601(&dt.format("%+").to_string(), None).ok()
}

/// (год, месяц, день) → glib::DateTime для календаря.
fn naive_to_calendar_date(y: i32, m: u32, d: u32) -> Option<glib::DateTime> {
    naive_to_calendar(chrono::NaiveDate::from_ymd_opt(y, m, d)?)
}
