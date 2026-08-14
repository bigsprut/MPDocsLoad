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

/// Месяцы, именительный падеж (полный месяц: «январь 2025»).
const MONTHS_NOM: [&str; 12] = [
    "январь", "февраль", "март", "апрель", "май", "июнь",
    "июль", "август", "сентябрь", "октябрь", "ноябрь", "декабрь",
];

/// Месяцы, родительный падеж (один день: «23 января 2026»).
const MONTHS_GEN: [&str; 12] = [
    "января", "февраля", "марта", "апреля", "мая", "июня",
    "июля", "августа", "сентября", "октября", "ноября", "декабря",
];

/// Человекочитаемое описание диапазона дат — ЧИСТАЯ функция от [from, to],
/// независимо от того, как даты заданы (виджет интервала, ручной ввод, restore):
/// ровно год → «2024 год»; ровно полугодие → «первое/второе полугодие 2024»;
/// ровно квартал → «3 квартал 2025»; ровно месяц → «январь 2025»;
/// один день (from == to) → «23 января 2026»; прочее → «с 04.03.2025 по 06.03.2025».
/// None — одна из дат не задана/не парсится.
pub(crate) fn describe_range(
    from: Option<chrono::NaiveDate>,
    to: Option<chrono::NaiveDate>,
) -> Option<String> {
    use chrono::Datelike;
    let f = from?;
    let t = to?;

    // Один день.
    if f == t {
        return Some(format!(
            "{} {} {}",
            f.day(),
            MONTHS_GEN[f.month0() as usize],
            f.year()
        ));
    }
    // Стандартные интервалы проверяем только внутри одного года (границы
    // календарных периодов не пересекают годы).
    if f.year() == t.year() {
        let y = f.year();
        // Последний день месяца t (для проверок «ровно …»).
        let last_of = |m: u32| {
            chrono::NaiveDate::from_ymd_opt(y, m + 1, 1)
                .and_then(|d| d.pred_opt())
                .or_else(|| chrono::NaiveDate::from_ymd_opt(y, 12, 31))
        };
        // Ровно год: 1 января .. 31 декабря.
        if (f.month(), f.day()) == (1, 1) && (t.month(), t.day()) == (12, 31) {
            return Some(format!("{y} год"));
        }
        // Ровно полугодие: 01.01–30.06 или 01.07–31.12.
        if (f.month(), f.day()) == (1, 1) && (t.month(), t.day()) == (6, 30) {
            return Some(format!("первое полугодие {y}"));
        }
        if (f.month(), f.day()) == (7, 1) && (t.month(), t.day()) == (12, 31) {
            return Some(format!("второе полугодие {y}"));
        }
        // Ровно месяц: первое число .. последнее число того же месяца.
        if f.day() == 1 && t.month() == f.month() && last_of(f.month()) == Some(t) {
            return Some(format!("{} {}", MONTHS_NOM[f.month0() as usize], y));
        }
        // Ровно квартал: первый месяц квартала (1/4/7/10), день 1, до конца
        // третьего месяца. (Проверка после месяца — диапазоны не пересекаются.)
        if f.day() == 1 && matches!(f.month(), 1 | 4 | 7 | 10) && t.month() == f.month() + 2 {
            if let Some(last) = last_of(t.month()) {
                if last == t {
                    let q = (f.month() - 1) / 3 + 1;
                    return Some(format!("{q} квартал {y}"));
                }
            }
        }
    }
    // Произвольный диапазон.
    Some(format!(
        "с {} по {}",
        f.format("%d.%m.%Y"),
        t.format("%d.%m.%Y")
    ))
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
    let dt = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
        naive.and_hms_opt(12, 0, 0)?,
        chrono::Utc,
    );
    glib::DateTime::from_iso8601(&dt.format("%+").to_string(), None).ok()
}

#[cfg(test)]
mod describe_range_tests {
    use super::describe_range;
    use chrono::NaiveDate;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn full_year() {
        assert_eq!(
            describe_range(Some(d("2024-01-01")), Some(d("2024-12-31"))),
            Some("2024 год".into())
        );
    }

    #[test]
    fn halves() {
        assert_eq!(
            describe_range(Some(d("2024-01-01")), Some(d("2024-06-30"))),
            Some("первое полугодие 2024".into())
        );
        assert_eq!(
            describe_range(Some(d("2024-07-01")), Some(d("2024-12-31"))),
            Some("второе полугодие 2024".into())
        );
    }

    #[test]
    fn quarters() {
        assert_eq!(
            describe_range(Some(d("2025-07-01")), Some(d("2025-09-30"))),
            Some("3 квартал 2025".into())
        );
        assert_eq!(
            describe_range(Some(d("2025-01-01")), Some(d("2025-03-31"))),
            Some("1 квартал 2025".into())
        );
    }

    #[test]
    fn months() {
        // Через границы месяцев (то же, что выбор «Месяц» в виджете).
        assert_eq!(
            describe_range(Some(d("2025-01-01")), Some(d("2025-01-31"))),
            Some("январь 2025".into())
        );
        // Февраль невисокосного года.
        assert_eq!(
            describe_range(Some(d("2025-02-01")), Some(d("2025-02-28"))),
            Some("февраль 2025".into())
        );
    }

    #[test]
    fn single_day() {
        assert_eq!(
            describe_range(Some(d("2026-01-23")), Some(d("2026-01-23"))),
            Some("23 января 2026".into())
        );
    }

    #[test]
    fn arbitrary_range() {
        assert_eq!(
            describe_range(Some(d("2025-03-04")), Some(d("2025-03-06"))),
            Some("с 04.03.2025 по 06.03.2025".into())
        );
        // Почти месяц (не до конца) — произвольный диапазон.
        assert_eq!(
            describe_range(Some(d("2025-01-01")), Some(d("2025-01-30"))),
            Some("с 01.01.2025 по 30.01.2025".into())
        );
        // Межгодовой диапазон.
        assert_eq!(
            describe_range(Some(d("2024-11-01")), Some(d("2025-02-28"))),
            Some("с 01.11.2024 по 28.02.2025".into())
        );
    }

    #[test]
    fn missing_dates() {
        assert_eq!(describe_range(None, Some(d("2025-01-01"))), None);
        assert_eq!(describe_range(Some(d("2025-01-01")), None), None);
    }
}
