//! Вкладка «Планировщик» — cron-расписания автозагрузки.
//!
//! Список расписаний (CRUD, вкл/выкл, «выполнить сейчас»), переключатель
//! автозапуска с ОС, форма добавления. Фоновый Runner (стартует в app.rs)
//! выполняет наступившие расписания, пока GUI открыт; результаты — в Журнале.
//!
//! Форма добавления привязана к активному магазину (вкладка «Магазин»):
//! профиль = активный, отчёт — из списка отчётов его провайдера. Чтобы создать
//! расписание для другого профиля — переключите активный магазин.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib::clone;
use gtk4::prelude::*;
use gtk4::{Align, Button, CheckButton, ComboBoxText, Entry, Label, ListBox, Orientation, Switch};

use crate::channels::{CommandSender, ReportInfo, ScheduleView, UiCommand};

thread_local! {
    static CMD: Rc<RefCell<Option<CommandSender>>> = Rc::new(RefCell::new(None));
    static W_LIST: Rc<RefCell<Option<ListBox>>> = Rc::new(RefCell::new(None));
    static W_AUTOSTART: Rc<RefCell<Option<Switch>>> = Rc::new(RefCell::new(None));
    static W_WIN_SCHEDULER: Rc<RefCell<Option<Switch>>> = Rc::new(RefCell::new(None));
    static W_REPORT_COMBO: Rc<RefCell<Option<ComboBoxText>>> = Rc::new(RefCell::new(None));
    static W_NAME: Rc<RefCell<Option<Entry>>> = Rc::new(RefCell::new(None));
    static W_CRON: Rc<RefCell<Option<Entry>>> = Rc::new(RefCell::new(None));
    static W_PERIOD: Rc<RefCell<Option<Entry>>> = Rc::new(RefCell::new(None));
    /// (display_name, type_id) отчётов активного провайдера. Индекс 0 = «(выберите)».
    static REPORTS: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(Vec::new()));
    /// Текущий список расписаний (для резолва имени в действиях по строке).
    static SCHEDULES: Rc<RefCell<Vec<ScheduleView>>> = Rc::new(RefCell::new(Vec::new()));
    /// Активный профиль (из ActiveShopChanged) — цель нового расписания.
    static ACTIVE_PROFILE: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    /// Защита от рекурсии при программной установке Switch автозапуска.
    static RESTORING: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
}

pub fn build(cs: &CommandSender) -> gtk4::Box {
    CMD.with(|c| *c.borrow_mut() = Some(cs.clone()));
    let root = gtk4::Box::new(Orientation::Vertical, 12);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.set_margin_top(16);
    root.set_margin_bottom(16);

    root.append(&crate::widgets::tab_help::title_row_with_help(
        "Расписания",
        "title-2",
        SCHEDULER_HELP,
    ));
    root.append(
        &Label::builder()
            .label("Новые расписания создаются для активного магазина (вкладка «Магазин»).")
            .css_classes(["dim-label"])
            .halign(Align::Start)
            .wrap(true)
            .build(),
    );

    // --- Автозапуск с ОС ---
    let auto_box = gtk4::Box::new(Orientation::Horizontal, 12);
    auto_box.append(
        &Label::builder()
            .label("Автозапуск с Windows")
            .halign(Align::Start)
            .hexpand(true)
            .build(),
    );
    let autostart = Switch::builder().halign(Align::End).build();
    autostart.set_active(mdwf_scheduler::is_autostart_enabled());
    autostart.connect_active_notify(|sw| {
        if RESTORING.with(|r| *r.borrow()) {
            return;
        }
        let Some(cs) = CMD.with(|c| c.borrow().clone()) else { return };
        cs.send(UiCommand::SetAutostart { enabled: sw.is_active() });
    });
    W_AUTOSTART.with(|w| *w.borrow_mut() = Some(autostart.clone()));
    auto_box.append(&autostart);
    root.append(&auto_box);

    // --- Фоновый планировщик Windows (Task Scheduler) ---
    // В отличие от autostart (запуск GUI при логине) — это polling-задача ОС,
    // которая каждые ~5 мин запускает `mdwf schedule run`. Работает без открытого
    // GUI (пока пользователь залогинен). Гибрид: in-process Runner активен, когда
    // GUI открыт; Windows-задача подхватывает, когда закрыт. claim_расписания
    // защищает от двойного выполнения.
    let win_box = gtk4::Box::new(Orientation::Horizontal, 12);
    win_box.append(
        &Label::builder()
            .label("Фоновый планировщик Windows")
            .halign(Align::Start)
            .hexpand(true)
            .build(),
    );
    let win_sw = Switch::builder().halign(Align::End).build();
    win_sw.set_active(mdwf_scheduler::is_windows_scheduler_enabled());
    win_sw.connect_active_notify(|sw| {
        if RESTORING.with(|r| *r.borrow()) {
            return;
        }
        let Some(cs) = CMD.with(|c| c.borrow().clone()) else { return };
        cs.send(UiCommand::SetWinScheduler { enabled: sw.is_active() });
    });
    W_WIN_SCHEDULER.with(|w| *w.borrow_mut() = Some(win_sw.clone()));
    win_box.append(&win_sw);
    root.append(&win_box);

    // --- Форма добавления ---
    root.append(&build_add_form());

    // --- Список расписаний ---
    root.append(
        &Label::builder()
            .label("Расписания")
            .css_classes(["heading"])
            .halign(Align::Start)
            .build(),
    );
    let list = ListBox::builder()
        .selection_mode(gtk4::SelectionMode::None)
        .show_separators(true)
        .vexpand(true)
        .build();
    list.set_placeholder(Some(
        &Label::builder()
            .label("Нет расписаний. Добавьте выше.")
            .css_classes(["dim-label"])
            .margin_top(16)
            .build(),
    ));
    W_LIST.with(|w| *w.borrow_mut() = Some(list.clone()));
    root.append(&list);

    cs.send(UiCommand::ListSchedules);
    root
}

/// Форма добавления расписания: имя, отчёт, cron (+пресеты), период.
/// Каждое поле — с tooltip-пояснением «что это и зачем»; под cron и периодом —
/// развёрнутые подсказки; сверху — инструкция по настройке по шагам.
fn build_add_form() -> gtk4::Box {
    let frame = gtk4::Box::new(Orientation::Vertical, 8);

    // --- Инструкция по настройке (по шагам) ---
    frame.append(&Label::builder()
        .label("Как настроить расписание — по шагам")
        .css_classes(["heading"])
        .halign(Align::Start)
        .build());
    for step in [
        "1) Во вкладке «Магазин» выберите магазин и профиль — новое расписание создаётся для активного магазина.",
        "2) Задайте имя (любое, по нему расписание будет в списке) и выберите отчёт.",
        "3) Нажмите кнопку «Когда…» и в понятном диалоге выберите частоту, день и время (там же — за какой месяц выгружать).",
        "4) Проверьте, что на кнопке написано то, что нужно (например «1-го числа каждого месяца, 02:00»).",
        "5) Нажмите «Добавить расписание». Готово — в срок оно выполнится само (пока программа запущена или включён фоновый планировщик ниже).",
    ] {
        frame.append(&Label::builder()
            .label(step)
            .wrap(true)
            .xalign(0.0)
            .halign(Align::Start)
            .hexpand(true)
            .margin_start(8)
            .css_classes(["dim-label"])
            .build());
    }

    let report = ComboBoxText::new();
    report.append_text("(выберите отчёт)");
    report.set_active(Some(0));
    report.set_tooltip_text(Some(
        "Какой отчёт выгружать по расписанию. Список берётся из активного магазина (вкладка «Магазин»)",
    ));
    W_REPORT_COMBO.with(|w| *w.borrow_mut() = Some(report.clone()));

    let name = Entry::builder()
        .placeholder_text("Имя расписания")
        .tooltip_text("Произвольное название — только чтобы отличать расписания в списке")
        .build();
    W_NAME.with(|w| *w.borrow_mut() = Some(name.clone()));

    // --- «Когда выполнять» — понятный диалог вместо ввода cron-цифр вручную ---
    // W_CRON остаётся источником значения для add_schedule(), но поле скрыто:
    // значение задаётся диалогом (частота + день + время) и показывается
    // человеческим текстом на кнопке.
    let cron = Entry::builder().text("0 2 1 * *").build();
    cron.set_visible(false);
    W_CRON.with(|w| *w.borrow_mut() = Some(cron.clone()));

    let when_btn = Button::with_label(&describe_cron("0 2 1 * *"));
    when_btn.set_tooltip_text(Some(
        "Нажмите, чтобы настроить расписание: частота, день и время — без ручного ввода выражения",
    ));
    {
        let btn = when_btn.clone();
        cron.connect_changed(move |e| btn.set_label(&describe_cron(&e.text())));
    }

    // --- Период — combo с понятными названиями (вместо ввода «-1») ---
    let period = Entry::builder().text("-1").build();
    period.set_visible(false);
    W_PERIOD.with(|w| *w.borrow_mut() = Some(period.clone()));
    // «Когда…» открывает диалог настройки; пишет в скрытые поля cron/period формы.
    {
        let ce = cron.clone();
        let pe = period.clone();
        when_btn.connect_clicked(move |_| show_when_dialog(&ce, &pe));
    }

    let period_combo = ComboBoxText::new();
    period_combo.set_tooltip_text(Some(
        "За какой месяц выгружать отчёт. Обычно нужен «Прошлый месяц» — отчёты за месяц готовы в первых числах следующего",
    ));
    for (label, val) in [
        ("Прошлый месяц (обычно)", "-1"),
        ("Текущий месяц", "0"),
        ("Позапрошлый месяц", "-2"),
    ] {
        period_combo.append(Some(val), label);
    }
    period_combo.set_active_id(Some("-1"));
    {
        let p = period.clone();
        period_combo.connect_changed(move |c| {
            if let Some(id) = c.active_id() {
                p.set_text(&id);
            }
        });
    }

    let add_btn = super::icon_button("Добавить расписание", "list-add-symbolic");
    add_btn.add_css_class("suggested-action");
    add_btn.set_tooltip_text(Some("Сохранить расписание — оно появится в списке ниже"));
    add_btn.connect_clicked(|_| add_schedule());

    frame.append(&field_row("Имя:", &name));
    frame.append(&field_row("Отчёт:", &report));
    frame.append(&field_row("Когда:", &when_btn));
    frame.append(&Label::builder()
        .label("Нажмите «Когда…» и выберите частоту, день и время — расписание соберётся само.")
        .wrap(true)
        .xalign(0.0)
        .halign(Align::Start)
        .hexpand(true)
        .margin_start(8)
        .css_classes(["dim-label"])
        .build());
    frame.append(&field_row("Период:", &period_combo));
    frame.append(&Label::builder()
        .label("Период — за какой месяц выгружать отчёт. «Прошлый месяц» — стандартный выбор: отчёты за месяц готовы в начале следующего.")
        .wrap(true)
        .xalign(0.0)
        .halign(Align::Start)
        .hexpand(true)
        .margin_start(8)
        .css_classes(["dim-label"])
        .build());
    frame.append(&add_btn);
    frame
}

/// Человекочитаемое описание cron-выражения (для кнопки «Когда…»).
fn describe_cron(cron: &str) -> String {
    let f: Vec<&str> = cron.split_whitespace().collect();
    if f.len() == 5 {
        let (min, hour, dom, _mon, dow) = (f[0], f[1], f[2], f[3], f[4]);
        let time = format!("{hour:0>2}:{min:0>2}");
        if dom != "*" && dow == "*" {
            return format!("{dom}-го числа каждого месяца, {time}");
        }
        if dom == "*" && dow != "*" {
            let day = match dow {
                "0" | "7" => "воскресеньям",
                "1" => "понедельникам",
                "2" => "вторникам",
                "3" => "средам",
                "4" => "четвергам",
                "5" => "пятницам",
                "6" => "субботам",
                _ => return format!("по выражению «{cron}»"),
            };
            return format!("по {day}, {time}");
        }
        if dom == "*" && dow == "*" {
            return format!("ежедневно, {time}");
        }
    }
    format!("по выражению «{cron}»")
}

/// Человекочитаемое описание периода выгрузки (period_offset → текст).
/// offset = смещение относительно текущего месяца: 0 — текущий, −1 — прошлый и т.д.
/// (значения задаёт диалог «Когда…»; здесь — только отображение).
fn describe_period(offset: i32) -> String {
    match offset {
        0 => "за текущий месяц".into(),
        -1 => "за прошлый месяц".into(),
        -2 => "за позапрошлый месяц".into(),
        n if n < 0 => format!("за {} мес. назад", -n),
        n => format!("со смещением периода +{n}"),
    }
}

/// Человекочитаемое описание расписания целиком для строки списка:
/// ЧТО выгружать (отчёт), ЗА КАКОЙ ПЕРИОД (смещение месяца) и КОГДА (cron → текст).
fn describe_schedule(s: &ScheduleView) -> String {
    let what = if s.report_names.is_empty() {
        "(отчёт не задан)".to_string()
    } else if s.report_names.len() == 1 {
        format!("«{}»", s.report_names[0])
    } else {
        s.report_names.join(", ")
    };
    let how = describe_period(s.period_offset);
    let when = describe_cron(&s.cron_expr);
    format!("Выгружать {what} ({how}), {when}")
}

/// Диалог настройки расписания с понятными названиями вместо ввода cron-цифр.
/// Частота (ежемесячно/еженедельно/ежедневно) + день + время → собирает cron
/// и записывает в `cron_entry` (и период — в `period_entry`). Параметрический:
/// переиспользуется и формой добавления (W_CRON/W_PERIOD), и диалогом изменения.
fn show_when_dialog(cron_entry: &Entry, period_entry: &Entry) {
    let cur_cron = cron_entry.text().to_string();
    let cur_period = period_entry.text().to_string();
    // Клоны для обработчика «Применить» (запись cron/period в переданные поля).
    let cron_entry = cron_entry.clone();
    let period_entry = period_entry.clone();

    let dlg = gtk4::Dialog::builder()
        .title("Когда выполнять расписание")
        .modal(true)
        .default_width(420)
        .build();
    let content = dlg.content_area();
    let col = gtk4::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(10)
        .margin_top(16)
        .margin_bottom(8)
        .margin_start(16)
        .margin_end(16)
        .build();

    // Разбор текущего cron → начальное состояние контролов (best-effort).
    let f: Vec<&str> = cur_cron.split_whitespace().collect();
    let (mut min, mut hour, mut dom, mut dow) = (0u32, 2u32, 1u32, 1u32);
    let mut freq_idx = 0usize; // 0 ежемесячно, 1 еженедельно, 2 ежедневно
    if f.len() == 5 {
        min = f[0].parse().unwrap_or(0);
        hour = f[1].parse().unwrap_or(2);
        if f[2] != "*" && f[4] == "*" {
            freq_idx = 0;
            dom = f[2].parse().unwrap_or(1);
        } else if f[2] == "*" && f[4] != "*" {
            freq_idx = 1;
            dow = f[4].parse().unwrap_or(1);
        } else {
            freq_idx = 2;
        }
    }

    // Частота.
    let freq = ComboBoxText::new();
    for t in ["Ежемесячно", "Еженедельно", "Ежедневно"] {
        freq.append_text(t);
    }
    freq.set_active(Some(freq_idx as u32));
    freq.set_tooltip_text(Some("Как часто выполнять выгрузку"));
    col.append(&field_row("Как часто:", &freq));

    // День месяца (для «Ежемесячно»).
    let dom_spin = gtk4::SpinButton::with_range(1.0, 28.0, 1.0);
    dom_spin.set_value(f64::from(dom));
    dom_spin.set_tooltip_text(Some("Число месяца (1–28, одинаково для всех месяцев)"));
    let dom_row = field_row("Какого числа:", &dom_spin);

    // День недели (для «Еженедельно»).
    let dow_combo = ComboBoxText::new();
    let days = ["Понедельник", "Вторник", "Среда", "Четверг", "Пятница", "Суббота", "Воскресенье"];
    for (i, t) in days.iter().enumerate()
    {
        dow_combo.append(Some(&(i + 1).to_string()), t);
    }
    dow_combo.set_active_id(Some(&dow.clamp(1, 7).to_string()));
    let dow_row = field_row("В какой день:", &dow_combo);

    // Время.
    let time_box = gtk4::Box::new(Orientation::Horizontal, 6);
    let hour_spin = gtk4::SpinButton::with_range(0.0, 23.0, 1.0);
    hour_spin.set_value(f64::from(hour));
    hour_spin.set_tooltip_text(Some("Час (0–23)"));
    let min_spin = gtk4::SpinButton::with_range(0.0, 59.0, 1.0);
    min_spin.set_value(f64::from(min));
    min_spin.set_tooltip_text(Some("Минуты (0–59)"));
    time_box.append(&Label::new(Some("в")));
    time_box.append(&hour_spin);
    time_box.append(&Label::new(Some(":")));
    time_box.append(&min_spin);
    let time_row = field_row("Во сколько:", &time_box);

    // Период (тоже переносим в диалог — всё расписание в одном месте).
    let period_combo = ComboBoxText::new();
    for (id, t) in [
        ("-1", "за прошлый месяц (обычно)"),
        ("0", "за текущий месяц"),
        ("-2", "за позапрошлый месяц"),
    ] {
        period_combo.append(Some(id), t);
    }
    period_combo.set_active_id(Some(if cur_period == "0" {
        "0"
    } else if cur_period == "-2" {
        "-2"
    } else {
        "-1"
    }));
    period_combo.set_tooltip_text(Some("За какой месяц выгружать отчёт"));
    let period_row = field_row("Выгружать:", &period_combo);

    col.append(&dom_row);
    col.append(&dow_row);
    col.append(&time_row);
    col.append(&period_row);

    // Видимость день-строк — по частоте.
    {
        let dom_r = dom_row.clone();
        let dow_r = dow_row.clone();
        let apply = move |idx: Option<u32>| match idx {
            Some(0) => {
                dom_r.set_visible(true);
                dow_r.set_visible(false);
            }
            Some(1) => {
                dom_r.set_visible(false);
                dow_r.set_visible(true);
            }
            _ => {
                dom_r.set_visible(false);
                dow_r.set_visible(false);
            }
        };
        apply(Some(freq_idx as u32)); // начальное состояние
        freq.connect_changed(move |c| apply(c.active()));
    }

    content.append(&col);
    dlg.add_button("Отмена", gtk4::ResponseType::Cancel);
    dlg.add_button("Применить", gtk4::ResponseType::Accept);

    dlg.connect_response(move |d, resp| {
        if resp == gtk4::ResponseType::Accept {
            let (h, m) = (hour_spin.value() as i32, min_spin.value() as i32);
            let cron_text = match freq.active() {
                Some(0) => format!("{m} {h} {} * *", dom_spin.value() as i32),
                Some(1) => {
                    let dow_id = dow_combo.active_id().unwrap_or_else(|| "1".into());
                    format!("{m} {h} * * {dow_id}")
                }
                _ => format!("{m} {h} * * *"),
            };
            cron_entry.set_text(&cron_text);
            if let Some(pid) = period_combo.active_id() {
                period_entry.set_text(&pid);
            }
        }
        d.destroy();
    });
    dlg.show();
}

fn field_row(label: &str, widget: &impl IsA<gtk4::Widget>) -> gtk4::Box {
    let b = gtk4::Box::new(Orientation::Horizontal, 10);
    b.append(&Label::builder().label(label).width_chars(10).xalign(0.0).build());
    widget.set_hexpand(true);
    widget.set_halign(Align::Fill);
    b.append(widget);
    b
}

/// Считывает форму и шлёт AddSchedule (профиль — активный магазин).
fn add_schedule() {
    let Some(cs) = CMD.with(|c| c.borrow().clone()) else { return };
    let name = W_NAME
        .with(|w| w.borrow().as_ref().map(|e| e.text().to_string()))
        .unwrap_or_default();
    let cron = W_CRON
        .with(|w| w.borrow().as_ref().map(|e| e.text().to_string()))
        .unwrap_or_default();
    let period = W_PERIOD.with(|w| {
        w.borrow()
            .as_ref()
            .map(|e| e.text().to_string().parse::<i32>().unwrap_or(-1))
    });
    let Some(report_type) = selected_report_value() else { return };
    let Some(profile_name) = ACTIVE_PROFILE.with(|p| p.borrow().clone()) else { return };
    if name.trim().is_empty() {
        return;
    }
    cs.send(UiCommand::AddSchedule {
        name,
        profile_name,
        report_type,
        cron_expr: cron,
        period_offset: period.unwrap_or(-1),
    });
    W_NAME.with(|w| {
        if let Some(e) = w.borrow().as_ref() {
            e.set_text("");
        }
    });
}

/// Хук: отчёты провайдера загружены — заполняем combo формы. (ReportInfo уже
/// несут display_name + type_id; активный профиль задаёт провайдера.)
pub fn on_reports_loaded(reports: &[ReportInfo]) {
    let combo = W_REPORT_COMBO.with(|w| w.borrow().clone());
    let Some(combo) = combo else { return };
    combo.remove_all();
    combo.append_text("(выберите отчёт)");
    let pairs: Vec<(String, String)> = reports
        .iter()
        .map(|r| (r.display_name.clone(), r.type_id.clone()))
        .collect();
    for (label, _type_id) in &pairs {
        combo.append_text(label);
    }
    REPORTS.with(|r| *r.borrow_mut() = pairs);
    combo.set_active(Some(0));
}

/// Хук: активный магазин сменился — запоминаем профиль (цель нового расписания).
pub fn on_active_shop_changed(profile_name: &str) {
    ACTIVE_PROFILE.with(|p| *p.borrow_mut() = Some(profile_name.to_string()));
}

fn selected_report_value() -> Option<String> {
    let combo = W_REPORT_COMBO.with(|w| w.borrow().clone())?;
    let idx = combo.active()? as usize;
    if idx == 0 {
        return None;
    }
    REPORTS.with(|r| r.borrow().get(idx - 1).map(|(_, t)| t.clone()))
}

/// Хук: список расписаний загружен — рендерим.
pub fn on_schedules_loaded(result: &Result<Vec<ScheduleView>, String>) {
    let list = W_LIST.with(|w| w.borrow().clone());
    let Some(list) = list else { return };
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    match result {
        Ok(scheds) => {
            SCHEDULES.with(|s| *s.borrow_mut() = scheds.clone());
            for s in scheds {
                list.append(&make_row(s));
            }
        }
        Err(e) => {
            list.append(
                &Label::builder()
                    .label(format!("Ошибка: {e}"))
                    .css_classes(["error"])
                    .build(),
            );
        }
    }
}

/// Хук: автозапуск изменён. На успех — ничего (switch уже в состоянии клика);
/// на ошибку — откатываем switch к реальному состоянию (без повторной отправки).
pub fn on_autostart_changed(result: &Result<bool, String>) {
    if result.is_ok() {
        return;
    }
    let sw = W_AUTOSTART.with(|w| w.borrow().clone());
    let Some(sw) = sw else { return };
    RESTORING.with(|r| *r.borrow_mut() = true);
    sw.set_active(mdwf_scheduler::is_autostart_enabled());
    RESTORING.with(|r| *r.borrow_mut() = false);
}

/// Хук: фоновый планировщик Windows изменён. На ошибку — откатываем switch.
pub fn on_win_scheduler_changed(result: &Result<bool, String>) {
    if result.is_ok() {
        return;
    }
    let sw = W_WIN_SCHEDULER.with(|w| w.borrow().clone());
    let Some(sw) = sw else { return };
    RESTORING.with(|r| *r.borrow_mut() = true);
    sw.set_active(mdwf_scheduler::is_windows_scheduler_enabled());
    RESTORING.with(|r| *r.borrow_mut() = false);
}

/// Диалог ИЗМЕНЕНИЯ расписания: имя + «Когда…» (cron и период). Отчёт и профиль
/// сохраняются (сменить их для готового расписания — редко нужно; для этого
/// удалите и создайте заново). Переиспользует параметрический show_when_dialog.
fn show_edit_dialog(s: &ScheduleView) {
    let Some(cs) = CMD.with(|c| c.borrow().clone()) else { return };

    let dlg = gtk4::Dialog::builder()
        .title("Изменить расписание")
        .modal(true)
        .default_width(420)
        .build();
    let content = dlg.content_area();
    let col = gtk4::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(10)
        .margin_top(16)
        .margin_bottom(8)
        .margin_start(16)
        .margin_end(16)
        .build();

    let name_entry = Entry::builder().text(&s.name).build();

    // Скрытые поля cron/period предзаполнены текущими значениями расписания;
    // «Когда…» пишет в них через show_when_dialog.
    let cron_entry = Entry::builder().text(&s.cron_expr).build();
    cron_entry.set_visible(false);
    let period_entry = Entry::builder()
        .text(s.period_offset.to_string())
        .build();
    period_entry.set_visible(false);

    let when_btn = Button::with_label(&describe_cron(&s.cron_expr));
    when_btn.set_tooltip_text(Some("Нажмите, чтобы изменить время и период"));
    {
        let btn = when_btn.clone();
        cron_entry.connect_changed(move |e| btn.set_label(&describe_cron(&e.text())));
    }
    {
        let ce = cron_entry.clone();
        let pe = period_entry.clone();
        when_btn.connect_clicked(move |_| show_when_dialog(&ce, &pe));
    }

    col.append(&field_row("Имя:", &name_entry));
    col.append(&field_row("Когда:", &when_btn));
    content.append(&col);

    dlg.add_button("Отмена", gtk4::ResponseType::Cancel);
    dlg.add_button("Сохранить", gtk4::ResponseType::Accept);

    let id = s.id;
    let name_for_save = name_entry.clone();
    let cron_for_save = cron_entry.clone();
    let period_for_save = period_entry.clone();
    dlg.connect_response(move |d, resp| {
        if resp == gtk4::ResponseType::Accept {
            let name = name_for_save.text().to_string();
            let cron = cron_for_save.text().to_string();
            let period = period_for_save
                .text()
                .to_string()
                .parse::<i32>()
                .unwrap_or(-1);
            if !name.trim().is_empty() {
                cs.send(UiCommand::UpdateSchedule {
                    id,
                    name,
                    cron_expr: cron,
                    period_offset: period,
                });
            }
        }
        d.destroy();
    });
    dlg.show();
}

/// Строка расписания — карточка с человекочитаемым описанием
/// (что выгружать / за какой период / когда). Сырое cron-выражение и прочие
/// технические детали вынесены в отдельную приглушённую строку для справки.
fn make_row(s: &ScheduleView) -> gtk4::ListBoxRow {
    let row = gtk4::ListBoxRow::builder().selectable(false).activatable(false).build();
    let card = gtk4::Box::new(Orientation::Vertical, 5);
    card.set_margin_start(10);
    card.set_margin_end(10);
    card.set_margin_top(8);
    card.set_margin_bottom(8);

    // --- Верх: имя расписания + действия (вкл / выполнить / удалить) ---
    let top = gtk4::Box::new(Orientation::Horizontal, 10);
    let name = Label::builder()
        .label(&s.name)
        .css_classes(["heading"])
        .xalign(0.0)
        .hexpand(true)
        .halign(Align::Start)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .build();
    top.append(&name);

    let name_for_toggle = s.name.clone();
    let enabled = CheckButton::with_label("вкл");
    enabled.set_active(s.enabled);
    enabled.set_tooltip_text(Some(
        "Включено — расписание выполняется автоматически. Снято — пауза (не удаляется)",
    ));
    enabled.connect_toggled(move |cb| {
        let Some(cs) = CMD.with(|c| c.borrow().clone()) else { return };
        cs.send(UiCommand::SetScheduleEnabled {
            name: name_for_toggle.clone(),
            enabled: cb.is_active(),
        });
    });
    top.append(&enabled);

    let edit_btn = super::icon_only_button("document-edit-symbolic", "Изменить расписание: имя, время и период");
    edit_btn.set_tooltip_text(Some("Изменить расписание: имя, время и период"));
    let s_for_edit = s.clone();
    edit_btn.connect_clicked(move |_| show_edit_dialog(&s_for_edit));
    top.append(&edit_btn);

    let run_btn = super::icon_only_button("media-playback-start-symbolic", "Выполнить прямо сейчас (не дожидаясь срока) — удобно для проверки настройки");
    run_btn.set_tooltip_text(Some(
        "Выполнить прямо сейчас (не дожидаясь срока) — удобно для проверки настройки",
    ));
    let name_for_run = s.name.clone();
    run_btn.connect_clicked(move |_| {
        if let Some(cs) = CMD.with(|c| c.borrow().clone()) {
            cs.send(UiCommand::RunScheduleNow { name: name_for_run.clone() });
        }
    });
    top.append(&run_btn);

    let del_btn = super::icon_only_button("user-trash-symbolic", "Удалить расписание");
    del_btn.set_tooltip_text(Some("Удалить расписание"));
    del_btn.add_css_class("destructive-action");
    let name_for_del = s.name.clone();
    del_btn.connect_clicked(move |_| {
        if let Some(cs) = CMD.with(|c| c.borrow().clone()) {
            cs.send(UiCommand::DeleteSchedule { name: name_for_del.clone() });
        }
    });
    top.append(&del_btn);
    card.append(&top);

    // --- Главное: человекочитаемое описание (ЧТО / ЗА КАКОЙ ПЕРИОД / КОГДА) ---
    let summary = describe_schedule(s);
    let desc = Label::builder()
        .label(&summary)
        .wrap(true)
        .xalign(0.0)
        .halign(Align::Start)
        .tooltip_text(format!(
            "выражение: {} | смещение периода (месяцев): {}",
            s.cron_expr, s.period_offset
        ))
        .build();
    card.append(&desc);

    // --- Технические детали (приглушённые): профиль, след. запуск, статус, cron ---
    let mut meta: Vec<String> = Vec::new();
    meta.push(format!("Профиль: {}", s.profile_name));
    meta.push(format!(
        "След. запуск: {}",
        s.next_run_at
            .as_deref()
            .map_or_else(|| "—".to_string(), mdwf_scheduler::fmt_local)
    ));
    if let Some(st) = s.last_run_status.as_deref() {
        meta.push(format!("Статус: {st}"));
    }
    meta.push(format!("выражение: {}", s.cron_expr));
    let meta_lbl = Label::builder()
        .label(meta.join("   •   "))
        .css_classes(["dim-label"])
        .wrap(true)
        .xalign(0.0)
        .halign(Align::Start)
        .build();
    card.append(&meta_lbl);

    row.set_child(Some(&card));
    row
}

/// Контекстная помощь вкладки «Планировщик» (кнопка «?» в заголовке).
const SCHEDULER_HELP: &[crate::widgets::tab_help::HelpBlock] = &[
    crate::widgets::tab_help::HelpBlock::H("Что здесь"),
    crate::widgets::tab_help::HelpBlock::T("Автоматическая выгрузка отчётов по расписанию. Пошаговая инструкция — в форме добавления выше."),
    crate::widgets::tab_help::HelpBlock::H("Поля расписания"),
    crate::widgets::tab_help::HelpBlock::B(&[
        "Имя — произвольное, чтобы отличать в списке.",
        "Отчёт — что выгружать (берётся из активного магазина).",
        "Когда — кнопка «Когда…»: частота (ежемесячно/еженедельно/ежедневно), день и время; выражение расписания собирается автоматически.",
        "Период — за какой месяц: 0 — текущий, −1 — прошлый (обычно так), −2 — позапрошлый.",
    ]),
    crate::widgets::tab_help::HelpBlock::H("Управление"),
    crate::widgets::tab_help::HelpBlock::B(&[
        "«вкл» — включено/пауза; «▶» — выполнить сейчас (проверка настройки); «🗑» — удалить.",
    ]),
    crate::widgets::tab_help::HelpBlock::H("Автозапуск"),
    crate::widgets::tab_help::HelpBlock::T("Опции ниже («Автозапуск с Windows», «Фоновый планировщик») позволяют расписаниям выполняться без открытого окна программы."),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(name: &str, reports: &[&str], cron: &str, offset: i32) -> ScheduleView {
        ScheduleView {
            id: 1,
            name: name.into(),
            profile_id: 1,
            profile_name: "oz_prof1".into(),
            reports: reports.iter().map(|s| (*s).to_string()).collect(),
            report_names: reports.iter().map(|s| (*s).to_string()).collect(),
            cron_expr: cron.into(),
            period_offset: offset,
            enabled: true,
            next_run_at: None,
            last_run_at: None,
            last_run_status: None,
        }
    }

    #[test]
    fn period_describes_common_offsets() {
        assert_eq!(describe_period(0), "за текущий месяц");
        assert_eq!(describe_period(-1), "за прошлый месяц");
        assert_eq!(describe_period(-2), "за позапрошлый месяц");
        assert_eq!(describe_period(-5), "за 5 мес. назад");
        assert_eq!(describe_period(2), "со смещением периода +2");
    }

    #[test]
    fn schedule_combines_what_period_when() {
        let s = sample("Реализации", &["Отчёт по реализации"], "0 2 1 * *", -1);
        let d = describe_schedule(&s);
        assert!(d.contains("«Отчёт по реализации»"), "got: {d}");
        assert!(d.contains("за прошлый месяц"), "got: {d}");
        assert!(d.contains("1-го числа каждого месяца"), "got: {d}");
        assert!(d.contains("02:00"), "got: {d}");
    }

    #[test]
    fn schedule_weekly_and_daily() {
        let s = sample("Еженед.", &["Баланс"], "30 9 * * 1", 0);
        assert!(describe_schedule(&s).contains("по понедельникам"));
        let s2 = sample("Ежедн.", &["Баланс"], "0 6 * * *", -2);
        let d2 = describe_schedule(&s2);
        assert!(d2.contains("ежедневно"), "got: {d2}");
        assert!(d2.contains("за позапрошлый месяц"), "got: {d2}");
    }

    #[test]
    fn schedule_multiple_reports_unquoted() {
        let s = sample("Два", &["Отчёт A", "Отчёт B"], "0 2 1 * *", -1);
        let d = describe_schedule(&s);
        assert!(d.contains("Отчёт A, Отчёт B"), "got: {d}");
        assert!(!d.contains('«'), "несколько отчётов — без кавычек: {d}");
    }

    #[test]
    fn schedule_empty_report() {
        let s = sample("Пусто", &[], "0 2 1 * *", -1);
        assert!(describe_schedule(&s).contains("(отчёт не задан)"));
    }
}
