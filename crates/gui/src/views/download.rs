//! Вкладка «Загрузка» — самодостаточный интерактивный цикл:
//! провайдер → профиль → отчёт → фильтры → список/генерация → скачивание.
//!
//! Поддерживает оба режима (спец. AcquisitionMode):
//!  * Browsable: список → выбор чекбоксами → «Скачать выбранные».
//!  * Period: период → «Скачать по периоду».

use std::cell::RefCell;
use std::rc::Rc;

use chrono::NaiveDate;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, CheckButton, ComboBoxText, Entry, Label, ListBox, Orientation,
    PolicyType, ScrolledWindow,
};

use mdwf_core::{DocumentEntry, DocumentFilter, DownloadedFile, Profile, ReportParams};

use crate::channels::{CommandSender, DownloadState, ReportInfo};

thread_local! {
    static PROVIDERS: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(Vec::new()));
    static PROFILES: Rc<RefCell<Vec<Profile>>> = Rc::new(RefCell::new(Vec::new()));
    static REPORTS: Rc<RefCell<Vec<ReportInfo>>> = Rc::new(RefCell::new(Vec::new()));
    static DOCS: Rc<RefCell<Vec<DocumentEntry>>> = Rc::new(RefCell::new(Vec::new()));
    static CHECKS: Rc<RefCell<Vec<(String, CheckButton)>>> = Rc::new(RefCell::new(Vec::new()));
    // Командный канал (для авто-запросов при смене выбора).
    static CMD: Rc<RefCell<Option<CommandSender>>> = Rc::new(RefCell::new(None));
    // Виджеты (сохраняем после build для обновления из событий).
    static W_PROVIDER: Rc<RefCell<Option<ComboBoxText>>> = Rc::new(RefCell::new(None));
    static W_PROFILE: Rc<RefCell<Option<ComboBoxText>>> = Rc::new(RefCell::new(None));
    static W_REPORT: Rc<RefCell<Option<ComboBoxText>>> = Rc::new(RefCell::new(None));
    static W_LIST: Rc<RefCell<Option<ListBox>>> = Rc::new(RefCell::new(None));
    static W_RESULT: Rc<RefCell<Option<Label>>> = Rc::new(RefCell::new(None));
    static W_RESULT_BOX: Rc<RefCell<Option<GtkBox>>> = Rc::new(RefCell::new(None));
    static W_MODE_HINT: Rc<RefCell<Option<Label>>> = Rc::new(RefCell::new(None));
    static W_LIST_BTN: Rc<RefCell<Option<Button>>> = Rc::new(RefCell::new(None));
    static W_PERIOD_BTN: Rc<RefCell<Option<Button>>> = Rc::new(RefCell::new(None));
    static W_DOWNLOAD_BTN: Rc<RefCell<Option<Button>>> = Rc::new(RefCell::new(None));
    static W_CATEGORY_COMBO: Rc<RefCell<Option<ComboBoxText>>> = Rc::new(RefCell::new(None));
    static W_CATEGORY_LABEL: Rc<RefCell<Option<Label>>> = Rc::new(RefCell::new(None));
    static W_DATE_FROM: Rc<RefCell<Option<Entry>>> = Rc::new(RefCell::new(None));
    static W_DATE_TO: Rc<RefCell<Option<Entry>>> = Rc::new(RefCell::new(None));
    static W_MONTH: Rc<RefCell<Option<Entry>>> = Rc::new(RefCell::new(None));
    static W_LIMIT: Rc<RefCell<Option<Entry>>> = Rc::new(RefCell::new(None));
}

/// Хук: провайдеры загружены.
pub fn on_providers_loaded(providers: &[crate::channels::ProviderInfo]) {
    PROVIDERS.with(|p| {
        *p.borrow_mut() = providers
            .iter()
            .map(|pr| (pr.id.clone(), pr.display_name.clone()))
            .collect();
    });
    W_PROVIDER.with(|w| {
        if let Some(combo) = w.borrow().as_ref() {
            combo.remove_all();
            for pr in providers {
                combo.append_text(&format!("{} [{}]", pr.display_name, pr.id));
            }
            combo.set_active(Some(0));
        }
    });
    on_provider_changed();
}

/// Хук: профили загружены.
pub fn on_profiles_loaded(profiles: &[Profile]) {
    PROFILES.with(|p| *p.borrow_mut() = profiles.to_vec());
    refresh_profile_combo();
}

/// Хук: категории документов WB загружены → заполняем combo.
pub fn on_document_categories_loaded(res: &Result<Vec<String>, String>) {
    let combo = W_CATEGORY_COMBO.with(|w| w.borrow().clone());
    let Some(combo) = combo else { return };
    combo.remove_all();
    combo.append_text("(все)");
    match res {
        Err(e) => {
            combo.append_text(&format!("(ошибка: {e})"));
        }
        Ok(cats) if cats.is_empty() => {
            combo.append_text("(нет категорий)");
        }
        Ok(cats) => {
            for c in cats {
                combo.append_text(c);
            }
        }
    }
    combo.set_active(Some(0));
}

/// Хук: отчёты загружены (все уже принадлежат запрошенному провайдеру).
pub fn on_reports_loaded(reports: &[ReportInfo]) {
    REPORTS.with(|r| *r.borrow_mut() = reports.to_vec());
    let combo = W_REPORT.with(|w| w.borrow().clone());
    let Some(combo) = combo else { return };
    combo.remove_all();
    if reports.is_empty() {
        combo.append_text("(нет отчётов)");
    } else {
        for r in reports {
            combo.append_text(&format!("{} — {}", r.type_id, r.display_name));
        }
    }

    // Восстанавливаем выбранный отчёт из сохранённого состояния (если есть).
    let pending = PENDING_REPORT.with(|p| p.borrow_mut().take());
    if let Some(rtype) = pending {
        // Ищем индекс отчёта с совпадающим type_id.
        let n = combo.model().map_or(0, |m| m.iter_n_children(None));
        let mut found = false;
        for i in 0..n {
            combo.set_active(Some(i as u32));
            if let Some(text) = combo.active_text() {
                if text.to_string().starts_with(&format!("{rtype} —")) {
                    found = true;
                    break;
                }
            }
        }
        if !found {
            combo.set_active(Some(0));
        }
    } else {
        combo.set_active(Some(0));
    }
    update_mode_hint();
    // Явно запрашиваем категории, т.к. set_active может не вызвать connect_changed.
    maybe_request_categories();
}

pub fn build(cs: &CommandSender) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.set_margin_top(16);
    root.set_margin_bottom(16);

    root.append(&Label::builder()
        .label("Загрузка документов")
        .css_classes(["title-2"])
        .halign(gtk4::Align::Start)
        .build());

    root.append(&Label::builder()
        .label("Выберите маркетплейс → профиль → отчёт, задайте фильтры и нажмите «Список документов» (для отчётов-списков) или «Скачать по периоду» (для отчётов по периоду).")
        .css_classes(["dim-label"])
        .halign(gtk4::Align::Start)
        .wrap(true)
        .build());

    // --- Строка 1: провайдер + профиль + отчёт ---
    let row1 = GtkBox::new(Orientation::Horizontal, 8);

    let provider_combo = ComboBoxText::new();
    provider_combo.set_tooltip_text(Some("Маркетплейс"));
    row1.append(&Label::new(Some("Магазин:")));
    row1.append(&provider_combo);

    let profile_combo = ComboBoxText::new();
    profile_combo.set_tooltip_text(Some("Профиль учётных данных"));
    row1.append(&Label::new(Some("Профиль:")));
    row1.append(&profile_combo);

    let report_combo = ComboBoxText::new();
    report_combo.set_tooltip_text(Some("Тип отчёта"));
    row1.append(&Label::new(Some("Отчёт:")));
    row1.append(&report_combo);

    let load_reports_btn = Button::builder().label("↻ Обновить").tooltip_text("Перезагрузить список отчётов провайдера").build();
    row1.append(&load_reports_btn);
    root.append(&row1);

    // Подсказка о режиме выбранного отчёта.
    let mode_hint = Label::builder()
        .label("")
        .css_classes(["dim-label"])
        .halign(gtk4::Align::Start)
        .wrap(true)
        .build();
    root.append(&mode_hint);

    // --- Строка 2: фильтры ---
    // Период по умолчанию: последний год (диапазон) + прошлый месяц (для period-отчётов).
    let today = chrono::Local::now().date_naive();
    let year_ago = today - chrono::Duration::days(365);
    let last_month_date = today - chrono::Duration::days(30);
    let default_from = year_ago.format("%Y-%m-%d").to_string();
    let default_to = today.format("%Y-%m-%d").to_string();
    let default_month = last_month_date.format("%Y-%m").to_string();

    let row2 = GtkBox::new(Orientation::Horizontal, 8);
    let category_combo = ComboBoxText::new();
    category_combo.append_text("(все)");
    category_combo.set_active(Some(0));
    category_combo.set_tooltip_text(Some("Категория документа (загружается автоматически из WB)"));
    let date_from = Entry::builder().placeholder_text("с YYYY-MM-DD").width_chars(12).text(&default_from).build();
    let date_to = Entry::builder().placeholder_text("по YYYY-MM-DD").width_chars(12).text(&default_to).build();
    let limit_entry = Entry::builder().placeholder_text("лимит").width_chars(6).build();
    let period_entry = Entry::builder().placeholder_text("YYYY-MM").width_chars(9).text(&default_month).build();
    let category_label = Label::new(Some("Категория:"));
    row2.append(&category_label);
    row2.append(&category_combo);
    row2.append(&Label::new(Some("Диапазон:")));
    row2.append(&date_from);
    // Кнопка-календарь для date_from
    row2.append(&make_date_picker(&date_from, "%Y-%m-%d"));
    row2.append(&Label::new(Some("..")));
    row2.append(&date_to);
    // Кнопка-календарь для date_to
    row2.append(&make_date_picker(&date_to, "%Y-%m-%d"));
    row2.append(&Label::new(Some("Месяц:")));
    row2.append(&period_entry);
    // Кнопка-календарь для month
    row2.append(&make_date_picker(&period_entry, "%Y-%m"));
    row2.append(&Label::new(Some("Лимит:")));
    row2.append(&limit_entry);
    root.append(&row2);

    // --- Кнопки действий ---
    let row3 = GtkBox::new(Orientation::Horizontal, 8);
    let list_btn = Button::builder().label("📋 Список документов").tooltip_text("Для отчётов-списков (Browsable)").build();
    let download_btn = Button::builder().label("⬇ Скачать выбранные").css_classes(["suggested-action"]).tooltip_text("Скачать отмеченные документы").build();
    let period_btn = Button::builder().label("📅 Скачать по периоду").tooltip_text("Сгенерировать отчёт за период").build();
    row3.append(&list_btn);
    row3.append(&download_btn);
    row3.append(&period_btn);
    root.append(&row3);

    // --- Список документов (с чекбоксами) ---
    let list_box = ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::None);
    list_box.set_vexpand(true);
    let scroll = ScrolledWindow::builder()
        .child(&list_box)
        .hscrollbar_policy(PolicyType::Never)
        .build();
    root.append(&scroll);

    // --- Результат (контейнер: label + кнопка "Открыть папку") ---
    let result_box = GtkBox::new(Orientation::Horizontal, 8);
    let result_label = Label::builder()
        .label("Готов к работе. Создайте профиль во вкладке «Профили», затем выберите его здесь.")
        .halign(gtk4::Align::Start)
        .css_classes(["dim-label"])
        .wrap(true)
        .hexpand(true)
        .build();
    result_box.append(&result_label);
    root.append(&result_box);

    // Сохраняем виджеты.
    W_PROVIDER.with(|w| *w.borrow_mut() = Some(provider_combo.clone()));
    W_PROFILE.with(|w| *w.borrow_mut() = Some(profile_combo.clone()));
    W_REPORT.with(|w| *w.borrow_mut() = Some(report_combo.clone()));
    CMD.with(|c| *c.borrow_mut() = Some(cs.clone()));
    W_LIST.with(|w| *w.borrow_mut() = Some(list_box.clone()));
    W_RESULT.with(|w| *w.borrow_mut() = Some(result_label.clone()));
    W_RESULT_BOX.with(|w| *w.borrow_mut() = Some(result_box.clone()));
    W_MODE_HINT.with(|w| *w.borrow_mut() = Some(mode_hint.clone()));
    W_LIST_BTN.with(|w| *w.borrow_mut() = Some(list_btn.clone()));
    W_PERIOD_BTN.with(|w| *w.borrow_mut() = Some(period_btn.clone()));
    W_DOWNLOAD_BTN.with(|w| *w.borrow_mut() = Some(download_btn.clone()));
    W_CATEGORY_COMBO.with(|w| *w.borrow_mut() = Some(category_combo.clone()));
    W_CATEGORY_LABEL.with(|w| *w.borrow_mut() = Some(category_label.clone()));
    W_DATE_FROM.with(|w| *w.borrow_mut() = Some(date_from.clone()));
    W_DATE_TO.with(|w| *w.borrow_mut() = Some(date_to.clone()));
    W_MONTH.with(|w| *w.borrow_mut() = Some(period_entry.clone()));
    W_LIMIT.with(|w| *w.borrow_mut() = Some(limit_entry.clone()));

    // Смена провайдера → перезагрузка профилей и отчётов + автосохранение.
    provider_combo.connect_changed(move |_| {
        on_provider_changed();
        schedule_save();
    });
    profile_combo.connect_changed(move |_| {
        schedule_save();
    });
    // Смена отчёта → обновить подсказку режима + доступность кнопок + автосохранение.
    report_combo.connect_changed(move |_| {
        update_mode_hint();
        // Запрашиваем категории WB только при выборе отчёта wb.documents.
        maybe_request_categories();
        schedule_save();
    });
    update_mode_hint();

    // Автосохранение при изменении полей ввода.
    for entry in [&date_from, &date_to, &period_entry, &limit_entry] {
        let e = entry.clone();
        entry.connect_changed(move |_| {
            let _ = &e;
            schedule_save();
        });
    }
    // Автосохранение для category_combo.
    category_combo.connect_changed(move |_| {
        schedule_save();
    });

    // «↻ Обновить» — запросить отчёты выбранного провайдера.
    let cs_rep = cs.clone();
    let pc = provider_combo.clone();
    load_reports_btn.connect_clicked(move |_| {
        if let Some(pid) = current_provider_id(&pc) {
            cs_rep.send(crate::channels::UiCommand::LoadReports(pid));
        }
    });

    // Клоны полей для period-обработчика (list-обработчик замувит оригиналы).
    let df_per = date_from.clone();
    let dt_per = date_to.clone();
    let period_entry_per = period_entry.clone();

    // «Список документов».
    let cs_list = cs.clone();
    let cat_combo_list = category_combo.clone();
    list_btn.connect_clicked(move |_| {
        let Some((pid, pname, rtype)) = current_target() else {
            notify("Выберите профиль и отчёт.");
            return;
        };
        let filter = build_filter(&cat_combo_list, &date_from, &date_to, &limit_entry);
        // Категория опциональна: если не выбрана, вернутся документы всех категорий.
        // Оставляем подсказку только для удобства.
        if rtype == "wb.documents" && filter.category.is_none() {
            notify("Получаю документы всех категорий. Для фильтра выберите категорию из списка.");
        }
        cs_list.send(crate::channels::UiCommand::ListDocuments {
            provider_id: pid,
            profile_name: pname,
            report_type: rtype,
            filter,
        });
        notify("Запрос списка документов…");
    });

    // «Скачать выбранные».
    let cs_dl = cs.clone();
    download_btn.connect_clicked(move |_| {
        let Some((pid, pname, rtype)) = current_target() else {
            notify("Выберите профиль и отчёт.");
            return;
        };
        let ids: Vec<String> = CHECKS.with(|c| {
            c.borrow()
                .iter()
                .filter(|(_, cb)| cb.is_active())
                .map(|(id, _)| id.clone())
                .collect()
        });
        if ids.is_empty() {
            notify("Отметьте документы в списке выше.");
            return;
        }
        let n = ids.len();
        cs_dl.send(crate::channels::UiCommand::Download {
            provider_id: pid,
            profile_name: pname,
            report_type: rtype,
            document_ids: ids,
            params: ReportParams::new(),
        });
        notify(&format!("Скачивание {n} документов…"));
    });

    // «Скачать по периоду».
    let cs_per = cs.clone();
    period_btn.connect_clicked(move |_| {
        let Some((pid, pname, rtype)) = current_target() else {
            notify("Выберите профиль и отчёт.");
            return;
        };
        // Месяц (по умолчанию предзаполнен прошлым месяцем).
        let period = period_entry_per.text().to_string();
        let mut params = ReportParams {
            period: Some(period.clone()),
            ..Default::default()
        };
        // Диапазон дат — на случай, если отчёт требует date_from/date_to.
        params = params
            .with("date_from", df_per.text().to_string())
            .with("date_to", dt_per.text().to_string());
        cs_per.send(crate::channels::UiCommand::Download {
            provider_id: pid,
            profile_name: pname,
            report_type: rtype,
            document_ids: Vec::new(),
            params,
        });
        notify(&format!("Генерация отчёта за период {period}…"));
    });

    root
}

// ===== Хелперы =====

/// Возвращает текущий выбранный provider_id (из combo «Магазин»).
fn current_provider_id(combo: &ComboBoxText) -> Option<String> {
    let text = combo.active_text()?.to_string();
    // Формат: "Wildberries [wildberries]".
    let id = text.split(" [").nth(1)?.trim_end_matches(']').to_string();
    Some(id)
}

/// Возвращает текущий выбранный профиль (provider_id, name) из combo «Профиль».
fn current_profile() -> Option<(String, String)> {
    let combo = W_PROFILE.with(|w| w.borrow().clone())?;
    let text = combo.active_text()?.to_string();
    // Формат: "Имяпрофиля [provider_id]".
    let name = text.split(" [").next()?.to_string();
    let pid = text.split(" [").nth(1)?.trim_end_matches(']').to_string();
    Some((pid, name))
}

/// Возвращает (provider_id, profile_name, report_type) для текущего выбора.
fn current_target() -> Option<(String, String, String)> {
    let (pid, pname) = current_profile()?;
    let rtype = current_report_type()?;
    Some((pid, pname, rtype))
}

/// Возвращает выбранный report_type (без display_name).
fn current_report_type() -> Option<String> {
    let combo = W_REPORT.with(|w| w.borrow().clone())?;
    let text = combo.active_text()?.to_string();
    // Формат: "wb.documents — Документы ...".
    text.split(" — ").next().map(str::to_string)
}

/// Обновить combo профилей под текущего провайдера.
fn refresh_profile_combo() {
    let combo = W_PROFILE.with(|w| w.borrow().clone());
    let Some(combo) = combo else { return };
    let pid = W_PROVIDER.with(|wp| wp.borrow().as_ref().and_then(current_provider_id));
    let profiles = PROFILES.with(|p| p.borrow().clone());
    combo.remove_all();
    let mut any = false;
    for p in &profiles {
        if pid.as_deref() == Some(p.provider_id.as_str()) {
            combo.append_text(&format!("{} [{}]", p.name, p.provider_id));
            any = true;
        }
    }
    if !any {
        combo.append_text("(нет профилей — создайте во вкладке «Профили»)");
    }
    combo.set_active(Some(0));
}

/// Запрашивает категории WB только если выбран отчёт wb.documents.
fn maybe_request_categories() {
    let rtype = current_report_type();
    let pid = W_PROVIDER.with(|wp| wp.borrow().as_ref().and_then(current_provider_id));
    if rtype.as_deref() == Some("wb.documents") && pid.as_deref() == Some("wildberries") {
        if let Some((_, pname)) = current_profile() {
            if let Some(cs) = CMD.with(|c| c.borrow().clone()) {
                // Только если категории ещё не загружены (combo пустой или только «все»).
                let need = W_CATEGORY_COMBO.with(|w| {
                    w.borrow()
                        .as_ref()
                        .and_then(|c| c.model().map(|m| m.iter_n_children(None)))
                        .map_or(true, |n| n <= 1)
                });
                if need {
                    if let Some(combo) = W_CATEGORY_COMBO.with(|w| w.borrow().clone()) {
                        combo.remove_all();
                        combo.append_text("(загрузка…)");
                        combo.set_active(Some(0));
                    }
                    cs.send(crate::channels::UiCommand::LoadDocumentCategories {
                        provider_id: "wildberries".into(),
                        profile_name: pname,
                    });
                }
            }
        }
    }
}

/// Смена провайдера: обновить combo профилей + автоматически запросить отчёты.
fn on_provider_changed() {
    refresh_profile_combo();
    // Очищаем combo отчётов (покажем «Загрузка…»).
    if let Some(combo) = W_REPORT.with(|w| w.borrow().clone()) {
        combo.remove_all();
        combo.append_text("(загрузка…)");
        combo.set_active(Some(0));
    }
    // Авто-запрос отчётов выбранного провайдера.
    let pid = W_PROVIDER.with(|wp| wp.borrow().as_ref().and_then(current_provider_id));
    if let Some(ref pid) = pid {
        if let Some(cs) = CMD.with(|c| c.borrow().clone()) {
            cs.send(crate::channels::UiCommand::LoadReports(pid.clone()));
        }
    }
}

/// Обновить подсказку режима и доступность кнопок для выбранного отчёта.
fn update_mode_hint() {
    let (is_browsable, name) = current_report_type()
        .and_then(|t| REPORTS.with(|r| r.borrow().iter().find(|x| x.type_id == t).cloned()))
        .map_or((false, String::new()), |r| (r.is_browsable, r.display_name));

    W_MODE_HINT.with(|w| {
        if let Some(l) = w.borrow().as_ref() {
            if name.is_empty() {
                l.set_text("");
            } else if is_browsable {
                l.set_text(&format!(
                    "«{name}»: режим списка. Задайте диапазон дат и категорию (если нужно), нажмите «Список документов», отметьте нужные, затем «Скачать выбранные»."
                ));
            } else {
                l.set_text(&format!(
                    "«{name}»: режим периода. Укажите месяц (по умолчанию — прошлый) и нажмите «Скачать по периоду»."
                ));
            }
        }
    });

    // Доступность кнопок по режиму.
    let list_enabled = is_browsable;
    let dl_enabled = is_browsable;
    let period_enabled = !is_browsable;
    W_LIST_BTN.with(|w| { if let Some(b) = w.borrow().as_ref() { b.set_sensitive(list_enabled); } });
    W_DOWNLOAD_BTN.with(|w| { if let Some(b) = w.borrow().as_ref() { b.set_sensitive(dl_enabled); } });
    W_PERIOD_BTN.with(|w| { if let Some(b) = w.borrow().as_ref() { b.set_sensitive(period_enabled); } });

    // Категория нужна только для wb.documents — скрываем для остальных отчётов.
    let cat_visible = name == "wb.documents";
    if let Some(combo) = W_CATEGORY_COMBO.with(|w| w.borrow().clone()) {
        combo.set_visible(cat_visible);
    }
    if let Some(label) = W_CATEGORY_LABEL.with(|w| w.borrow().clone()) {
        label.set_visible(cat_visible);
    }
}

fn build_filter(category: &ComboBoxText, date_from: &Entry, date_to: &Entry, limit: &Entry) -> DocumentFilter {
    let mut f = DocumentFilter::default();
    if let Some(cat) = category.active_text() {
        let cat = cat.to_string();
        if cat != "(все)" && !cat.is_empty() {
            f.category = Some(cat);
        }
    }
    if let Ok(d) = NaiveDate::parse_from_str(&date_from.text(), "%Y-%m-%d") {
        f.date_from = Some(d);
    }
    if let Ok(d) = NaiveDate::parse_from_str(&date_to.text(), "%Y-%m-%d") {
        f.date_to = Some(d);
    }
    if let Ok(n) = limit.text().parse::<u32>() {
        f.limit = Some(n);
    }
    f
}

// ===== Автосохранение состояния =====

macro_rules! entry_value {
    ($static_:ident) => {
        $static_.with(|c| c.borrow().as_ref().map(|e| e.text().to_string()))
    };
}

/// Собирает текущее состояние экрана из виджетов.
fn collect_state() -> DownloadState {
    let provider_id = W_PROVIDER.with(|w| w.borrow().as_ref().and_then(current_provider_id));
    let profile_name = current_profile().map(|(_, n)| n);
    let report_type = current_report_type();
    DownloadState {
        provider_id,
        profile_name,
        report_type,
        category: W_CATEGORY_COMBO.with(|w| {
            w.borrow()
                .as_ref()
                .and_then(gtk4::ComboBoxText::active_text)
                .map(|s| s.to_string())
                .filter(|s| s != "(все)")
        }),
        date_from: entry_value!(W_DATE_FROM),
        date_to: entry_value!(W_DATE_TO),
        month: entry_value!(W_MONTH),
        limit: entry_value!(W_LIMIT),
    }
}

/// Отправляет команду сохранения состояния (вызывается из обработчиков).
fn schedule_save() {
    let Some(cs) = CMD.with(|c| c.borrow().clone()) else {
        return;
    };
    cs.send(crate::channels::UiCommand::SaveDownloadState(collect_state()));
}

/// Обработчик: сохранённое состояние загружено при старте → восстанавливаем выбор.
pub fn on_download_state_loaded(state: Option<&DownloadState>) {
    let Some(state) = state else {
        return;
    };

    // 1. Восстанавливаем категорию в combo (ищем совпадение).
    if let Some(v) = &state.category {
        W_CATEGORY_COMBO.with(|w| {
            if let Some(combo) = w.borrow().as_ref() {
                let n = combo.model().map_or(0, |m| m.iter_n_children(None));
                for i in 0..n {
                    combo.set_active(Some(i as u32));
                    if let Some(text) = combo.active_text() {
                        if text.as_str() == v {
                            break;
                        }
                    }
                }
            }
        });
    }
    if let Some(v) = &state.date_from {
        W_DATE_FROM.with(|w| { if let Some(e) = w.borrow().as_ref() { e.set_text(v); } });
    }
    if let Some(v) = &state.date_to {
        W_DATE_TO.with(|w| { if let Some(e) = w.borrow().as_ref() { e.set_text(v); } });
    }
    if let Some(v) = &state.month {
        W_MONTH.with(|w| { if let Some(e) = w.borrow().as_ref() { e.set_text(v); } });
    }
    if let Some(v) = &state.limit {
        W_LIMIT.with(|w| { if let Some(e) = w.borrow().as_ref() { e.set_text(v); } });
    }

    // 2. Восстанавливаем выбор провайдера в combo (по id).
    if let Some(pid) = &state.provider_id {
        let combo = W_PROVIDER.with(|w| w.borrow().clone());
        if let Some(combo) = &combo {
            let n = combo.model().map_or(0, |m| m.iter_n_children(None));
            let suffix = format!(" [{pid}]");
            for i in 0..n {
                combo.set_active(Some(i as u32));
                if let Some(text) = combo.active_text() {
                    if text.to_string().ends_with(&suffix) {
                        break;
                    }
                }
            }
        }
    }
    // on_provider_changed вызовется автоматически (через connect_changed).

    // 3. Восстанавливаем выбор отчёта (после загрузки списка отчётов).
    if let Some(rtype) = &state.report_type {
        // Отложим восстановление: combo отчётов заполнится после LoadReports.
        // Запоминаем желаемый report_type для on_reports_loaded.
        PENDING_REPORT.with(|p| *p.borrow_mut() = Some(rtype.clone()));
    }
}

thread_local! {
    /// Желаемый report_type, который нужно выбрать после загрузки списка отчётов.
    static PENDING_REPORT: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
}

fn notify(msg: &str) {
    W_RESULT.with(|rw| {
        if let Some(l) = rw.borrow().as_ref() {
            l.set_text(msg);
        }
    });
}

// ===== События =====

/// Обработчик: список документов получен.
pub fn on_documents_listed(res: &Result<Vec<DocumentEntry>, String>) {
    match res {
        Err(e) => notify(&format!("Ошибка: {e}")),
        Ok(docs) => {
            DOCS.with(|d| *d.borrow_mut() = docs.clone());
            render_list(docs);
            notify(&format!("Получено документов: {}", docs.len()));
        }
    }
}

fn render_list(docs: &[DocumentEntry]) {
    W_LIST.with(|lw| {
        let list_box = match lw.borrow().as_ref() {
            Some(lb) => lb.clone(),
            None => return,
        };
        while let Some(child) = list_box.first_child() {
            list_box.remove(&child);
        }
        CHECKS.with(|c| c.borrow_mut().clear());

        if docs.is_empty() {
            list_box.append(&Label::new(Some("Документы не найдены.")));
            return;
        }

        let header = GtkBox::new(Orientation::Horizontal, 12);
        header.set_margin_start(8);
        header.set_margin_end(8);
        header.set_margin_top(4);
        header.set_margin_bottom(4);
        header.append(&Label::builder().label("").width_chars(3).build());
        header.append(&Label::builder().label("Имя").width_chars(40).xalign(0.0).build());
        header.append(&Label::builder().label("Дата").width_chars(12).xalign(0.0).build());
        header.append(&Label::builder().label("Форматы").width_chars(18).xalign(0.0).build());
        header.append(&Label::builder().label("Размер").width_chars(10).xalign(0.0).build());
        list_box.append(&header);

        for doc in docs {
            let row = GtkBox::new(Orientation::Horizontal, 12);
            row.set_margin_start(8);
            row.set_margin_end(8);
            row.set_margin_top(2);
            row.set_margin_bottom(2);
            row.set_css_classes(&["doc-list-row"]);

            let cb = CheckButton::new();
            row.append(&cb);
            row.append(&Label::builder().label(&doc.display_name).width_chars(40).xalign(0.0).ellipsize(gtk4::pango::EllipsizeMode::End).build());
            let date_str = doc.date.map(|d| d.to_string()).unwrap_or_default();
            row.append(&Label::builder().label(&date_str).width_chars(12).xalign(0.0).build());
            let exts = doc.extensions.join(", ");
            row.append(&Label::builder().label(&exts).width_chars(18).xalign(0.0).build());
            let size = doc.size_hint.map(human_size).unwrap_or_default();
            row.append(&Label::builder().label(&size).width_chars(10).xalign(0.0).build());

            CHECKS.with(|c| c.borrow_mut().push((doc.id.clone(), cb)));
            list_box.append(&row);
        }
    });
}

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

/// Обработчик: скачивание завершено (с путями к файлам).
pub fn on_download_finished(result: &crate::channels::DownloadResult) {
    let n = result.files.len();
    if n == 0 {
        notify("Файлы не найдены.");
        return;
    }

    // Показываем пути к файлам.
    let paths_text = result
        .saved_paths
        .iter()
        .map(|p| format!("  • {p}"))
        .collect::<Vec<_>>()
        .join("\n");
    notify(&format!("✅ Скачано файлов: {n}.\n{paths_text}"));

    // Добавляем кнопку «Открыть папку» рядом с результатом.
    let result_box = W_RESULT_BOX.with(|w| w.borrow().clone());
    if let Some(rbox) = result_box {
        // Удаляем старую кнопку, если была.
        let mut child = rbox.last_child();
        while let Some(c) = child {
            let next = c.prev_sibling();
            let is_btn = c.downcast_ref::<gtk4::LinkButton>().is_some();
            if is_btn {
                rbox.remove(&c);
            }
            child = next;
        }

        // Определяем папку из первого пути.
        if let Some(first_path) = result.saved_paths.first() {
            if let Some(parent) = std::path::Path::new(first_path).parent() {
                let folder = parent.display().to_string();
                let link = gtk4::LinkButton::builder()
                    .label("📁 Открыть папку")
                    .uri(format!("file:///{folder}"))
                    .has_tooltip(true)
                    .tooltip_text(&folder)
                    .build();
                link.connect_clicked(move |_| {
                    let _ = open_folder(&folder);
                });
                rbox.append(&link);
            }
        }
    }
}

/// Открывает папку в проводнике Windows.
fn open_folder(path: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
    }
    Ok(())
}

/// Обработчик: ошибка скачивания.
pub fn on_download_error(err: &str) {
    notify(&format!("Ошибка выгрузки: {err}"));
}

// ===== Кнопка-календарь для выбора дат =====

/// Создаёт кнопку с иконкой календаря (MenuButton + Calendar в Popover).
/// При клике открывает календарь. Выбор даты записывает её в `entry`
/// в формате `date_format` (напр. "%Y-%m-%d" или "%Y-%m").
fn make_date_picker(entry: &Entry, date_format: &str) -> gtk4::MenuButton {
    let menu_btn = gtk4::MenuButton::builder()
        .icon_name("x-office-calendar-symbolic")
        .tooltip_text("Выбрать дату из календаря")
        .build();

    let calendar = gtk4::Calendar::builder()
        .show_day_names(true)
        .show_heading(true)
        .show_week_numbers(true)
        .build();

    // Предустановка календаря из текущего значения Entry.
    let current_text = entry.text().to_string();
    if let Some(dt) = parse_date_for_calendar(&current_text) {
        calendar.select_day(&dt);
    }

    // При выборе даты — записываем в Entry и закрываем popover.
    let entry_clone = entry.clone();
    let fmt = date_format.to_string();
    let popover = gtk4::Popover::builder().build();
    let popover_clone = popover.clone();
    calendar.connect_day_selected(move |cal| {
        let selected = cal.date();
        if let Ok(formatted) = selected.format(&fmt) {
            entry_clone.set_text(&formatted);
            popover_clone.popdown();
        }
    });

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
