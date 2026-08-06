//! Вкладка «Загрузка» — самодостаточный интерактивный цикл:
//! провайдер → профиль → отчёт → фильтры → список/генерация → скачивание.
//!
//! Поддерживает оба режима (спец. AcquisitionMode):
//!  * Browsable: список → выбор чекбоксами → «Скачать выбранные».
//!  * Period: период → «Скачать по периоду».

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use chrono::{Datelike, NaiveDate};
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, CheckButton, ComboBoxText, Entry, Label, ListBox, Orientation,
    PolicyType, ScrolledWindow,
};

use mdwf_core::{DocumentEntry, DocumentFilter, DownloadedFile, ReportParams};
use mdwf_storage::DownloadedDocInfo;

use crate::channels::{
    ActiveShop, CommandSender, DocumentCategoryInfo, DocumentSel, DownloadState, ReportInfo,
};

thread_local! {
    static REPORTS: Rc<RefCell<Vec<ReportInfo>>> = Rc::new(RefCell::new(Vec::new()));
    static DOCS: Rc<RefCell<Vec<DocumentEntry>>> = Rc::new(RefCell::new(Vec::new()));
    static CHECKS: Rc<RefCell<Vec<(DocumentSel, CheckButton)>>> = Rc::new(RefCell::new(Vec::new()));
    /// Скачанные документы активного магазина+отчёта (document_id → info).
    /// Заполняется из UiEvent::DownloadsListed; используется для значка «уже загружен».
    static DOWNLOADED: Rc<RefCell<HashMap<String, DownloadedDocInfo>>> = Rc::new(RefCell::new(HashMap::new()));
    // Командный канал (для авто-запросов при смене выбора).
    static CMD: Rc<RefCell<Option<CommandSender>>> = Rc::new(RefCell::new(None));
    /// Активный магазин (из вкладки «Магазин») — единый источник правды выбора.
    /// None — магазин ещё не выбран, операции выгрузки недоступны.
    static ACTIVE_SHOP: Rc<RefCell<Option<ActiveShop>>> = Rc::new(RefCell::new(None));
    // Виджеты (сохраняем после build для обновления из событий).
    static W_REPORT: Rc<RefCell<Option<ComboBoxText>>> = Rc::new(RefCell::new(None));
    /// Read-only лейбл активного магазина (обновляется из ActiveShopChanged).
    static W_SHOP_LABEL: Rc<RefCell<Option<Label>>> = Rc::new(RefCell::new(None));
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
    /// Combo выбора месяца (1..12) и года для period-отчётов. Заменяет бывший
    /// текстовый W_MONTH: выбор месяца «без чисел», названия по-русски.
    static W_MONTH_COMBO: Rc<RefCell<Option<ComboBoxText>>> = Rc::new(RefCell::new(None));
    static W_YEAR_COMBO: Rc<RefCell<Option<ComboBoxText>>> = Rc::new(RefCell::new(None));
    static W_LIMIT: Rc<RefCell<Option<Entry>>> = Rc::new(RefCell::new(None));
    /// Карта: отображаемое имя категории → технический идентификатор (для WB API).
    /// Заполняется при загрузке категорий, используется в build_filter.
    static CATEGORIES: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(Vec::new()));
}

/// Названия месяцев по-русски (индекс 0 = Январь = месяц 1).
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

/// Возвращает provider_id активного магазина (из вкладки «Магазин»).
fn active_provider_id() -> Option<String> {
    ACTIVE_SHOP.with(|a| a.borrow().as_ref().map(|s| s.provider_id.clone()))
}

/// Возвращает (provider_id, profile_name) активного магазина.
fn active_target() -> Option<(String, String)> {
    ACTIVE_SHOP.with(|a| {
        a.borrow().as_ref().map(|s| (s.provider_id.clone(), s.profile_name.clone()))
    })
}

/// Хук: активный магазин изменён (из вкладки «Магазин» или восстановление).
/// Обновляем read-only лейбл, перезагружаем список отчётов провайдера.
pub fn on_active_shop_changed(
    provider_id: &str,
    provider_display_name: &str,
    seller_name: Option<&str>,
    profile_name: &str,
) {
    ACTIVE_SHOP.with(|a| {
        *a.borrow_mut() = Some(ActiveShop {
            provider_id: provider_id.to_string(),
            profile_name: profile_name.to_string(),
        });
    });
    // Read-only лейбл магазина.
    W_SHOP_LABEL.with(|w| {
        if let Some(l) = w.borrow().as_ref() {
            let display = seller_name.unwrap_or(profile_name);
            l.set_text(&format!("Магазин: {provider_display_name} — {display}"));
        }
    });
    // Перезагружаем отчёты нового провайдера (очистит combo + авто-запрос).
    if let Some(cs) = CMD.with(|c| c.borrow().clone()) {
        // Очищаем combo отчётов (покажем «загрузка…»).
        if let Some(combo) = W_REPORT.with(|w| w.borrow().clone()) {
            combo.remove_all();
            combo.append_text("(загрузка…)");
            combo.set_active(Some(0));
        }
        cs.send(crate::channels::UiCommand::LoadReports(provider_id.to_string()));
    }
}

/// Хук: категории документов WB загружены → заполняем combo.
///
/// В combo показываем человекочитаемый `label` (русское название, напр. «УПД»),
/// а в `CATEGORIES` храним карту `label → value`, чтобы при сборке фильтра
/// переводить выбранное имя обратно в технический идентификатор (`value`),
/// который WB ожидает в параметре `category`.
pub fn on_document_categories_loaded(res: &Result<Vec<DocumentCategoryInfo>, String>) {
    let combo = W_CATEGORY_COMBO.with(|w| w.borrow().clone());
    let Some(combo) = combo else { return };
    combo.remove_all();
    combo.append_text("(все)");
    // Очищаем карту перед заполнением — список мог быть перезагружен.
    CATEGORIES.with(|c| c.borrow_mut().clear());
    match res {
        Err(e) => {
            combo.append_text(&format!("(ошибка: {e})"));
        }
        Ok(cats) if cats.is_empty() => {
            combo.append_text("(нет категорий)");
        }
        Ok(cats) => {
            CATEGORIES.with(|c| {
                *c.borrow_mut() = cats
                    .iter()
                    .map(|cat| (cat.label.clone(), cat.value.clone()))
                    .collect();
            });
            for cat in cats {
                combo.append_text(&cat.label);
            }
        }
    }
    combo.set_active(Some(0));
}

/// Хук: отчёты загружены (все уже принадлежат запрошенному провайдеру).
pub fn on_reports_loaded(reports: &[ReportInfo]) {
    // Защита от гонки: если пользователь уже сменил магазин (=> провайдера),
    // устаревший результат игнорируем — иначе он затрёт актуальный список.
    let active_pid = active_provider_id();
    let result_pid = reports.first().map(|r| r.provider_id.clone());
    if let (Some(active), Some(got)) = (active_pid.as_deref(), result_pid.as_deref()) {
        if active != got {
            tracing::debug!(
                "on_reports_loaded: игнорируем устаревший результат \
                 (провайдер {got:?}, сейчас активен {active:?})"
            );
            return;
        }
    }

    REPORTS.with(|r| *r.borrow_mut() = reports.to_vec());
    let combo = W_REPORT.with(|w| w.borrow().clone());
    let Some(combo) = combo else { return };

    // Блокируем connect_changed на время программной перестройки combo,
    // чтобы не вызвать каскад лишних maybe_request_categories.
    REPORT_CHANGED_HANDLER.with(|h| {
        if let Some(id) = h.borrow().as_ref() {
            combo.block_signal(id);
        }
    });

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

    REPORT_CHANGED_HANDLER.with(|h| {
        if let Some(id) = h.borrow().as_ref() {
            combo.unblock_signal(id);
        }
    });

    update_mode_hint();
    // Явно запрашиваем категории, т.к. set_active при заблокированном сигнале
    // не вызовет connect_changed.
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
        .label("Магазин выбирается во вкладке «Магазин». Здесь задайте отчёт и фильтры, затем нажмите «Список документов» (для отчётов-списков) или «Скачать по периоду» (для отчётов по периоду).")
        .css_classes(["dim-label"])
        .halign(gtk4::Align::Start)
        .wrap(true)
        .build());

    // Read-only индикатор активного магазина (обновляется из ActiveShopChanged).
    let shop_label = Label::builder()
        .label("Магазин: не выбран — выберите во вкладке «Магазин».")
        .css_classes(["heading"])
        .halign(gtk4::Align::Start)
        .wrap(true)
        .build();
    root.append(&shop_label.clone());

    // --- Строка 1: отчёт + обновить (магазин берётся из вкладки «Магазин») ---
    let row1 = GtkBox::new(Orientation::Horizontal, 8);

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
    let default_from = year_ago.format("%Y-%m-%d").to_string();
    let default_to = today.format("%Y-%m-%d").to_string();
    // Месяц по умолчанию — прошлый (для period-отчётов). Учитываем переход через январь.
    let default_year = if today.month() == 1 {
        today.year() - 1
    } else {
        today.year()
    };
    let default_month0 = if today.month() == 1 {
        11 // декабрь прошлого года (индекс 0-based)
    } else {
        today.month0() as usize - 1 // прошлый месяц, индекс 0-based
    };

    let row2 = GtkBox::new(Orientation::Horizontal, 8);
    let category_combo = ComboBoxText::new();
    category_combo.append_text("(все)");
    category_combo.set_active(Some(0));
    category_combo.set_tooltip_text(Some("Категория документа (загружается автоматически из WB)"));
    let date_from = Entry::builder().placeholder_text("с YYYY-MM-DD").width_chars(12).text(&default_from).build();
    let date_to = Entry::builder().placeholder_text("по YYYY-MM-DD").width_chars(12).text(&default_to).build();
    let limit_entry = Entry::builder().placeholder_text("лимит").width_chars(6).build();

    // Combo выбора месяца (названия по-русски) и года. Заменяют бывшее
    // текстовое поле YYYY-MM: выбор «без чисел», названия по-русски.
    let month_combo = ComboBoxText::new();
    for name in MONTH_NAMES {
        month_combo.append_text(name);
    }
    month_combo.set_active(Some(default_month0 as u32));
    month_combo.set_tooltip_text(Some("Месяц для period-отчётов"));
    let year_combo = ComboBoxText::new();
    // Диапазон лет: 5 лет назад .. текущий+1 (с запасом на отчёты будущего периода).
    for y in (today.year() - 5)..=(today.year() + 1) {
        year_combo.append_text(&y.to_string());
    }
    // Индекс выбранного года в combo (0 = самый старый).
    let default_year_idx = (default_year - (today.year() - 5)) as u32;
    year_combo.set_active(Some(default_year_idx));
    year_combo.set_tooltip_text(Some("Год для period-отчётов"));

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
    row2.append(&month_combo);
    row2.append(&year_combo);
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
        .label("Готов к работе. Выберите магазин во вкладке «Магазин».")
        .halign(gtk4::Align::Start)
        .css_classes(["dim-label"])
        .wrap(true)
        .hexpand(true)
        .build();
    result_box.append(&result_label);
    root.append(&result_box);

    // Сохраняем виджеты.
    W_SHOP_LABEL.with(|w| *w.borrow_mut() = Some(shop_label.clone()));
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
    W_MONTH_COMBO.with(|w| *w.borrow_mut() = Some(month_combo.clone()));
    W_YEAR_COMBO.with(|w| *w.borrow_mut() = Some(year_combo.clone()));
    W_LIMIT.with(|w| *w.borrow_mut() = Some(limit_entry.clone()));

    // Смена отчёта → обновить подсказку режима + доступность кнопок + автосохранение.
    {
        let handler_id = report_combo.connect_changed(move |_| {
            update_mode_hint();
            // Запрашиваем категории WB только при выборе отчёта wb.documents.
            maybe_request_categories();
            schedule_save();
        });
        REPORT_CHANGED_HANDLER.with(|h| *h.borrow_mut() = Some(handler_id));
    }
    update_mode_hint();

    // Автосохранение при изменении полей ввода.
    for entry in [&date_from, &date_to, &limit_entry] {
        let e = entry.clone();
        entry.connect_changed(move |_| {
            let _ = &e;
            schedule_save();
        });
    }
    // Автосохранение + автообновление диапазона при смене месяца/года.
    // При выборе месяца: date_from = 1-е число месяца, date_to = сегодня
    // (если месяц текущий) или последнее число месяца.
    {
        let df = date_from.clone();
        let dt = date_to.clone();
        let mc = month_combo.clone();
        let yc = year_combo.clone();
        month_combo.connect_changed(move |_| {
            apply_month_to_range(&mc, &yc, &df, &dt);
            schedule_save();
        });
    }
    {
        let df = date_from.clone();
        let dt = date_to.clone();
        let mc = month_combo.clone();
        let yc = year_combo.clone();
        year_combo.connect_changed(move |_| {
            apply_month_to_range(&mc, &yc, &df, &dt);
            schedule_save();
        });
    }
    // Автосохранение для category_combo.
    category_combo.connect_changed(move |_| {
        schedule_save();
    });

    // «↻ Обновить» — запросить отчёты активного провайдера (из вкладки «Магазин»).
    let cs_rep = cs.clone();
    load_reports_btn.connect_clicked(move |_| {
        if let Some(pid) = active_provider_id() {
            cs_rep.send(crate::channels::UiCommand::LoadReports(pid));
        }
    });

    // Клоны полей для period-обработчика (list-обработчик замувит оригиналы).
    let df_per = date_from.clone();
    let dt_per = date_to.clone();

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
        let docs: Vec<DocumentSel> = CHECKS.with(|c| {
            c.borrow()
                .iter()
                .filter(|(_, cb)| cb.is_active())
                .map(|(sel, _)| sel.clone())
                .collect()
        });
        if docs.is_empty() {
            notify("Отметьте документы в списке выше.");
            return;
        }
        let n = docs.len();
        cs_dl.send(crate::channels::UiCommand::Download {
            provider_id: pid,
            profile_name: pname,
            report_type: rtype,
            documents: docs,
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
        // Месяц собираем из combo (YYYY-MM, напр. «2026-07»).
        let Some(period) = current_month_value() else {
            notify("Выберите месяц и год.");
            return;
        };
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
            documents: Vec::new(),
            params,
        });
        notify(&format!("Генерация отчёта за период {period}…"));
    });

    root
}

// ===== Хелперы =====

/// Возвращает (provider_id, profile_name, report_type) для активного магазина
/// и выбранного отчёта. provider/profile — из вкладки «Магазин» (ACTIVE_SHOP).
fn current_target() -> Option<(String, String, String)> {
    let (pid, pname) = active_target()?;
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

/// Возвращает выбранный месяц в формате `YYYY-MM` (напр. «2026-07») из combo
/// месяца и года. `None`, если какой-то из combo не выбран.
fn current_month_value() -> Option<String> {
    let month0 = W_MONTH_COMBO.with(|w| {
        w.borrow().as_ref().and_then(gtk4::prelude::ComboBoxExtManual::active).map(|i| i as usize)
    })?;
    let year_text = W_YEAR_COMBO.with(|w| {
        w.borrow()
            .as_ref()
            .and_then(gtk4::ComboBoxText::active_text)
            .map(|s| s.to_string())
    })?;
    let year: i32 = year_text.parse().ok()?;
    // month0 — индекс 0-based (0=Январь). Месяц для формата — 1-based.
    Some(format!("{year}-{:02}", month0 + 1))
}

/// Парсит значение `YYYY-MM` → (год, индекс месяца 0-based). `None` при ошибке.
fn parse_month_value(v: &str) -> Option<(i32, u32)> {
    let (y, m) = v.split_once('-')?;
    let year: i32 = y.parse().ok()?;
    let month: u32 = m.parse::<u32>().ok()?.checked_sub(1)?; // 1..12 → 0..11
    if month > 11 {
        return None;
    }
    Some((year, month))
}

/// Выбирает в combo года ближайший к `year` (если точного нет — ближайший
/// из доступных). Используется при восстановлении сохранённого состояния.
fn set_year_combo(combo: &ComboBoxText, year: i32) {
    let n = combo.model().map_or(0, |m| m.iter_n_children(None));
    let mut best_i = 0u32;
    let mut best_diff = i32::MAX;
    for i in 0..n {
        combo.set_active(Some(i as u32));
        if let Some(text) = combo.active_text() {
            if let Ok(y) = text.to_string().parse::<i32>() {
                let diff = (y - year).abs();
                if diff < best_diff {
                    best_diff = diff;
                    best_i = i as u32;
                }
            }
        }
    }
    combo.set_active(Some(best_i));
}

/// При смене месяца/года обновляет диапазон дат:
/// `date_from` = 1-е число выбранного месяца,
/// `date_to` = сегодня (если месяц текущий) или последнее число месяца.
fn apply_month_to_range(
    month_combo: &ComboBoxText,
    year_combo: &ComboBoxText,
    date_from: &Entry,
    date_to: &Entry,
) {
    let Some(month0) = month_combo.active() else {
        return;
    };
    let Some(year_text) = year_combo.active_text() else {
        return;
    };
    let Ok(year) = year_text.to_string().parse::<i32>() else {
        return;
    };
    let month = month0 + 1; // 1-based
    let Some(first) = chrono::NaiveDate::from_ymd_opt(year, month, 1) else {
        return;
    };
    // Последний день месяца = 1-й день следующего месяца минус 1 день.
    let last = chrono::NaiveDate::from_ymd_opt(year, month, 1)
        .and_then(|d| d.checked_add_months(chrono::Months::new(1)))
        .and_then(|d| d.pred_opt())
        .unwrap_or(first);
    let today = chrono::Local::now().date_naive();
    // date_to: последний день месяца, но не в будущем (для текущего/будущего
    // месяца — сегодня, для прошлого — последний день месяца).
    let to = if last > today { today } else { last };
    date_from.set_text(&first.format("%Y-%m-%d").to_string());
    date_to.set_text(&to.format("%Y-%m-%d").to_string());
}

/// Запрашивает категории WB только если активный магазин = wildberries и выбран
/// отчёт wb.documents. provider/profile берёт из активного магазина.
fn maybe_request_categories() {
    let rtype = current_report_type();
    let Some((pid, pname)) = active_target() else {
        return;
    };

    if rtype.as_deref() != Some("wb.documents") || pid != "wildberries" {
        return;
    }
    let Some(cs) = CMD.with(|c| c.borrow().clone()) else {
        return;
    };

    // Показываем «загрузка…» и отправляем запрос.
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

/// Обновить подсказку режима и доступность кнопок для выбранного отчёта.
fn update_mode_hint() {
    let rtype = current_report_type();
    let info = rtype
        .as_ref()
        .and_then(|t| REPORTS.with(|r| r.borrow().iter().find(|x| x.type_id == *t).cloned()));
    let (is_browsable, name) = info
        .as_ref()
        .map_or((false, String::new()), |r| (r.is_browsable, r.display_name.clone()));

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
    let cat_visible = rtype.as_deref() == Some("wb.documents");
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
            // Переводим выбранное отображаемое имя (label) в технический
            // идентификатор (value), который WB ожидает в параметре category.
            let resolved = CATEGORIES.with(|c| {
                c.borrow()
                    .iter()
                    .find(|(label, _)| label == &cat)
                    .map(|(_, value)| value.clone())
            });
            // Если перевод не найден (напр. служебные пункты combo),
            // категорию не передаём — WB вернёт документы всех категорий.
            f.category = resolved;
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
    // provider_id/profile_name берём из активного магазина (вкладка «Магазин»).
    // Эти поля в DownloadState дублируют ActiveShop (для обратной совместимости
    // сохранённого JSON), но источник правды выбора — ui_state/active_shop.
    let (provider_id, profile_name) = active_target()
        .map_or((None, None), |(pid, pname)| (Some(pid), Some(pname)));
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
        month: current_month_value(),
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
        // Сохранённый месяц имеет формат YYYY-MM: выставляем combo месяца и года.
        // "YYYY-MM" → индекс месяца (0-based) и год. При успехе НЕ обновляем
        // диапазон здесь — это сделает connect_changed, но он блокирован на
        // время восстановления (см. PENDING_REPORT). Диапазон восстановим явно ниже.
        if let Some((year, month0)) = parse_month_value(v) {
            W_MONTH_COMBO.with(|w| {
                if let Some(combo) = w.borrow().as_ref() {
                    combo.set_active(Some(month0));
                }
            });
            W_YEAR_COMBO.with(|w| {
                if let Some(combo) = w.borrow().as_ref() {
                    set_year_combo(combo, year);
                }
            });
        }
    }
    if let Some(v) = &state.limit {
        W_LIMIT.with(|w| { if let Some(e) = w.borrow().as_ref() { e.set_text(v); } });
    }

    // provider_id/profile_name НЕ восстанавливаем здесь — выбор магазина теперь
    // живет во вкладке «Магазин» и восстанавливается через ActiveShopLoaded.
    // Поля provider_id/profile_name в DownloadState сохраняются для обратной
    // совместимости сохранённого JSON, но источником правды не являются.

    // Восстанавливаем выбор отчёта (после загрузки списка отчётов активного магазина).
    if let Some(rtype) = &state.report_type {
        // Отложим восстановление: combo отчётов заполнится после LoadReports.
        // Запоминаем желаемый report_type для on_reports_loaded.
        PENDING_REPORT.with(|p| *p.borrow_mut() = Some(rtype.clone()));
    }
}

thread_local! {
    /// Желаемый report_type, который нужно выбрать после загрузки списка отчётов.
    static PENDING_REPORT: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    /// Handler id сигнала connect_changed у report_combo — чтобы блокировать
    /// его на время программной перестройки combo.
    static REPORT_CHANGED_HANDLER: Rc<RefCell<Option<glib::SignalHandlerId>>> =
        Rc::new(RefCell::new(None));
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
            // Запрашиваем статус «уже загружен» для активного магазина+отчёта.
            // (только для Browsable-отчётов со списком документов.)
            request_downloads_status();
        }
    }
}

/// Обработчик: список скачанных документов получен (для значка «уже загружен»).
/// Сохраняет в DOWNLOADED (если report_type совпадает с активным) и перерисовывает
/// список, чтобы показать/скрыть значки.
pub fn on_downloads_listed(report_type: &str, docs: Vec<DownloadedDocInfo>) {
    // Защита от гонки: применяем только если report_type совпадает с активным.
    let active_matches = current_report_type().is_some_and(|rt| rt == report_type);
    if !active_matches {
        return;
    }
    DOWNLOADED.with(|d| {
        let mut map = d.borrow_mut();
        map.clear();
        for info in &docs {
            map.insert(info.document_id.clone(), info.clone());
        }
    });
    // Перерисовываем список, чтобы отразить значки.
    let docs = DOCS.with(|d| d.borrow().clone());
    render_list(&docs);
}

/// Запрашивает у доменного слоя список уже скачанных документов для активного
/// магазина и выбранного отчёта. Ответ придёт в on_downloads_listed.
fn request_downloads_status() {
    let Some((_, profile_name)) = active_target() else {
        return;
    };
    let Some(report_type) = current_report_type() else {
        return;
    };
    let Some(cs) = CMD.with(|c| c.borrow().clone()) else {
        return;
    };
    cs.send(crate::channels::UiCommand::ListDownloads {
        profile_name,
        report_type,
    });
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
        header.append(&Label::builder().label("").width_chars(3).build()); // чекбокс
        header.append(&Label::builder().label("").width_chars(3).build()); // значок статуса
        header.append(&Label::builder().label("Имя").width_chars(36).xalign(0.0).build());
        header.append(&Label::builder().label("Дата").width_chars(12).xalign(0.0).build());
        header.append(&Label::builder().label("Форматы").width_chars(16).xalign(0.0).build());
        header.append(&Label::builder().label("Размер").width_chars(10).xalign(0.0).build());
        header.append(&Label::builder().label("Действия").width_chars(16).xalign(0.0).build());
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

            // Значок «уже загружен»: если document_id есть в DOWNLOADED — зелёный ✓.
            let downloaded_info = DOWNLOADED.with(|d| d.borrow().get(&doc.id).cloned());
            let status_label = if let Some(info) = &downloaded_info {
                let date = info.downloaded_at.format("%Y-%m-%d %H:%M").to_string();
                let lbl = Label::builder()
                    .label("✓")
                    .css_classes(["success"])
                    .tooltip_text(format!("Скачан {date}:\n{}", info.file_path).as_str())
                    .build();
                lbl
            } else {
                Label::builder().label("").width_chars(3).build()
            };
            row.append(&status_label);

            // П.5: иконка типа файла (эмодзи) как префикс названия — по первому
            // доступному расширению из doc.extensions (pdf/xlsx/xml/zip/…).
            let name_with_icon = match doc.extensions.first() {
                Some(e) => format!("{} {}", ext_emoji(e), doc.display_name),
                None => doc.display_name.clone(),
            };
            row.append(&Label::builder().label(&name_with_icon).width_chars(36).xalign(0.0).ellipsize(gtk4::pango::EllipsizeMode::End).build());
            let date_str = doc.date.map(|d| d.to_string()).unwrap_or_default();
            row.append(&Label::builder().label(&date_str).width_chars(12).xalign(0.0).build());
            let exts = doc.extensions.join(", ");
            row.append(&Label::builder().label(&exts).width_chars(16).xalign(0.0).build());
            let size = doc.size_hint.map(human_size).unwrap_or_default();
            row.append(&Label::builder().label(&size).width_chars(10).xalign(0.0).build());

            // Действия: «📂 Открыть» (если уже скачан) + «↻ Перекачать».
            let actions_box = GtkBox::new(Orientation::Horizontal, 4);
            if let Some(info) = &downloaded_info {
                let path = info.file_path.clone();
                let open_btn = Button::builder()
                    .label("📂")
                    .tooltip_text("Открыть файл")
                    .build();
                open_btn.connect_clicked(move |_| {
                    let _ = open_file(&path);
                });
                actions_box.append(&open_btn);
            }
            // Перекачать — переотправить Download с одним документом.
            let sel = DocumentSel {
                id: doc.id.clone(),
                name: Some(doc.display_name.clone()),
                extension: doc.extensions.first().cloned(),
            };
            let redownload_btn = Button::builder()
                .label("↻")
                .tooltip_text("Перекачать (с заменой)")
                .build();
            redownload_btn.connect_clicked(move |_| {
                let Some((pid, pname, rtype)) = current_target() else {
                    notify("Магазин или отчёт не выбраны.");
                    return;
                };
                let Some(cs) = CMD.with(|c| c.borrow().clone()) else {
                    return;
                };
                cs.send(crate::channels::UiCommand::Download {
                    provider_id: pid,
                    profile_name: pname,
                    report_type: rtype,
                    documents: vec![sel.clone()],
                    params: ReportParams::new(),
                });
                notify("Перекачивание документа…");
            });
            actions_box.append(&redownload_btn);
            row.append(&actions_box);

            CHECKS.with(|c| {
                c.borrow_mut().push((
                    DocumentSel {
                        id: doc.id.clone(),
                        // display_name — человекочитаемое имя (поле name из WB);
                        // станет базовым именем файла на диске.
                        name: Some(doc.display_name.clone()),
                        // Первый доступный формат — предпочтительный для скачивания.
                        extension: doc.extensions.first().cloned(),
                    },
                    cb,
                ));
            });
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

/// Эмодзи по типу файла (П.5). Регистронезависимо: WB отдаёт расширения как есть
/// из ответа API, регистр явно не гарантирован — нормализуем.
fn ext_emoji(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        // Электронные таблицы.
        "xlsx" | "xls" | "csv" => "📊",
        // Архивы.
        "zip" | "rar" | "7z" | "gz" | "tar" => "📦",
        // Прочее (pdf, xml, txt, json, …) — как документ.
        _ => "📄",
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

/// Открывает файл ассоциированным приложением (напр. Excel — для .xlsx).
/// Если файл не существует — возвращает ошибку (UI предложит «Перекачать»).
fn open_file(path: &str) -> std::io::Result<()> {
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
