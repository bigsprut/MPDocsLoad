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

    root.append(
        &Label::builder()
            .label("Планировщик (cron)")
            .css_classes(["title-2"])
            .halign(Align::Start)
            .build(),
    );
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
fn build_add_form() -> gtk4::Box {
    let frame = gtk4::Box::new(Orientation::Vertical, 8);

    let report = ComboBoxText::new();
    report.append_text("(выберите отчёт)");
    report.set_active(Some(0));
    W_REPORT_COMBO.with(|w| *w.borrow_mut() = Some(report.clone()));

    let name = Entry::builder().placeholder_text("Имя расписания").build();
    W_NAME.with(|w| *w.borrow_mut() = Some(name.clone()));

    let cron = Entry::builder()
        .placeholder_text("cron: мин час день мес день_недели")
        .text("0 2 1 * *")
        .build();
    W_CRON.with(|w| *w.borrow_mut() = Some(cron.clone()));

    let presets = gtk4::Box::new(Orientation::Horizontal, 6);
    for (label, expr) in [
        ("Ежемесячно (1-го, 02:00)", "0 2 1 * *"),
        ("Ежедневно (09:00)", "0 9 * * *"),
        ("Еженедельно (пн, 09:00)", "0 9 * * 1"),
    ] {
        let b = Button::with_label(label);
        let expr_owned = expr.to_string();
        b.connect_clicked(clone!(@weak cron => move |_| cron.set_text(&expr_owned)));
        presets.append(&b);
    }

    let period = Entry::builder()
        .placeholder_text("Смещение периода в месяцах (-1 = прошлый месяц)")
        .text("-1")
        .width_chars(6)
        .build();
    W_PERIOD.with(|w| *w.borrow_mut() = Some(period.clone()));

    let add_btn = Button::with_label("Добавить расписание");
    add_btn.add_css_class("suggested-action");
    add_btn.connect_clicked(|_| add_schedule());

    frame.append(&field_row("Имя:", &name));
    frame.append(&field_row("Отчёт:", &report));
    frame.append(&field_row("Cron:", &cron));
    frame.append(&presets);
    frame.append(&field_row("Период:", &period));
    frame.append(&add_btn);
    frame
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

/// Строка расписания.
fn make_row(s: &ScheduleView) -> gtk4::ListBoxRow {
    let row = gtk4::ListBoxRow::builder().selectable(false).activatable(false).build();
    let box_ = gtk4::Box::new(Orientation::Horizontal, 12);
    box_.set_margin_start(8);
    box_.set_margin_end(8);
    box_.set_margin_top(6);
    box_.set_margin_bottom(6);

    let name = Label::builder()
        .label(&s.name)
        .css_classes(["heading"])
        .width_chars(14)
        .xalign(0.0)
        .build();
    let prof = Label::builder()
        .label(&s.profile_name)
        .width_chars(12)
        .xalign(0.0)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .build();
    let reports = Label::builder()
        .label(s.report_names.join(", "))
        .width_chars(20)
        .xalign(0.0)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .tooltip_text(s.reports.join(", "))
        .build();
    let cron = Label::builder()
        .label(&s.cron_expr)
        .css_classes(["monospace"])
        .width_chars(13)
        .xalign(0.0)
        .build();
    let next = Label::builder()
        .label(s.next_run_at.as_deref().unwrap_or("—"))
        .css_classes(["dim-label"])
        .width_chars(20)
        .xalign(0.0)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .build();
    let status = Label::builder()
        .label(s.last_run_status.as_deref().unwrap_or("—"))
        .css_classes(["dim-label"])
        .width_chars(8)
        .xalign(0.0)
        .build();

    let name_for_toggle = s.name.clone();
    let enabled = CheckButton::with_label("вкл");
    enabled.set_active(s.enabled);
    enabled.connect_toggled(move |cb| {
        let Some(cs) = CMD.with(|c| c.borrow().clone()) else { return };
        cs.send(UiCommand::SetScheduleEnabled {
            name: name_for_toggle.clone(),
            enabled: cb.is_active(),
        });
    });

    let run_btn = Button::with_label("▶");
    let name_for_run = s.name.clone();
    run_btn.connect_clicked(move |_| {
        if let Some(cs) = CMD.with(|c| c.borrow().clone()) {
            cs.send(UiCommand::RunScheduleNow { name: name_for_run.clone() });
        }
    });
    let del_btn = Button::with_label("🗑");
    del_btn.add_css_class("destructive-action");
    let name_for_del = s.name.clone();
    del_btn.connect_clicked(move |_| {
        if let Some(cs) = CMD.with(|c| c.borrow().clone()) {
            cs.send(UiCommand::DeleteSchedule { name: name_for_del.clone() });
        }
    });

    box_.append(&name);
    box_.append(&prof);
    box_.append(&reports);
    box_.append(&cron);
    box_.append(&enabled);
    box_.append(&next);
    box_.append(&status);
    box_.append(&run_btn);
    box_.append(&del_btn);
    row.set_child(Some(&box_));
    row
}
