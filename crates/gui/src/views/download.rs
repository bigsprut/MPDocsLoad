//! Вкладка «Загрузка» — ядро интерактивного цикла:
//! фильтры → «Список документов» → выбор чекбоксами → «Скачать выбранные».
//!
//! Поддерживает оба режима (спец. AcquisitionMode):
//!  * Browsable: список → выбор → скачивание выбранных.
//!  * Period: период → генерация отчёта.

use std::cell::RefCell;
use std::rc::Rc;

use chrono::NaiveDate;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, CheckButton, ComboBoxText, Entry, Label, ListBox, Orientation, ScrolledWindow,
};

use mdwf_core::{DocumentEntry, DocumentFilter, DownloadedFile, ReportParams};

use crate::channels::{CommandSender, ProviderInfo};

thread_local! {
    static REPORT_TYPE: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    static DOCS: Rc<RefCell<Vec<DocumentEntry>>> = Rc::new(RefCell::new(Vec::new()));
    static LIST_WIDGET: Rc<RefCell<Option<ListBox>>> = Rc::new(RefCell::new(None));
    static RESULT_WIDGET: Rc<RefCell<Option<Label>>> = Rc::new(RefCell::new(None));
    static CHECKS: Rc<RefCell<Vec<(String, CheckButton)>>> = Rc::new(RefCell::new(Vec::new()));
    static PROFILE_COMBO: Rc<RefCell<Option<ComboBoxText>>> = Rc::new(RefCell::new(None));
}

/// Хук: провайдеры загружены (пока no-op; combo профилей обновляется отдельно).
pub fn on_providers_loaded(_providers: &[ProviderInfo]) {}

/// Хук: профили загружены — обновляем combo выбора профиля.
pub fn on_profiles_loaded(profiles: &[mdwf_core::Profile]) {
    PROFILE_COMBO.with(|pc| {
        if let Some(combo) = pc.borrow().as_ref() {
            combo.remove_all();
            for p in profiles {
                combo.append_text(&format!("{} [{}]", p.name, p.provider_id));
            }
        }
    });
}

/// Установить текущий тип отчёта (из вкладки «Отчёты»).
pub fn set_report_type(type_id: &str) {
    REPORT_TYPE.with(|r| *r.borrow_mut() = type_id.to_string());
    RESULT_WIDGET.with(|rw| {
        if let Some(l) = rw.borrow().as_ref() {
            l.set_text(&format!("Выбран отчёт: {type_id}. Перейдите к фильтрам ниже."));
        }
    });
}

pub fn build(cs: &CommandSender) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.set_margin_top(16);
    root.set_margin_bottom(16);

    let title = Label::builder()
        .label("Загрузка документов")
        .css_classes(["title-2"])
        .halign(gtk4::Align::Start)
        .build();
    root.append(&title);

    root.append(&Label::builder()
        .label("Выберите отчёт во вкладке «Отчёты», задайте фильтры и нажмите «Список документов» (Browsable) или «Скачать по периоду» (Period).")
        .css_classes(["dim-label"])
        .halign(gtk4::Align::Start)
        .wrap(true)
        .build());

    // --- Панель фильтров ---
    let filters = GtkBox::new(Orientation::Horizontal, 8);

    let profile_combo = ComboBoxText::new();
    profile_combo.set_tooltip_text(Some("Профиль (создаётся во вкладке «Профили»)"));
    let placeholder_profile = Label::new(Some("Профиль:"));
    filters.append(&placeholder_profile);
    filters.append(&profile_combo);

    let category_entry = Entry::builder().placeholder_text("категория (напр. upd)").width_chars(18).build();
    filters.append(&Label::new(Some("Категория:")));
    filters.append(&category_entry);

    let date_from = Entry::builder().placeholder_text("с YYYY-MM-DD").width_chars(12).build();
    let date_to = Entry::builder().placeholder_text("по YYYY-MM-DD").width_chars(12).build();
    filters.append(&Label::new(Some("Период:")));
    filters.append(&date_from);
    filters.append(&date_to);

    let limit_entry = Entry::builder().placeholder_text("лимит").width_chars(6).build();
    filters.append(&Label::new(Some("Лимит:")));
    filters.append(&limit_entry);

    root.append(&filters);

    // --- Кнопки действий ---
    let actions = GtkBox::new(Orientation::Horizontal, 8);
    let list_btn = Button::builder().label("📋 Список документов").build();
    let download_btn = Button::builder().label("⬇ Скачать выбранные").css_classes(["suggested-action"]).build();
    let period_btn = Button::builder().label("📅 Скачать по периоду").build();
    actions.append(&list_btn);
    actions.append(&download_btn);
    actions.append(&period_btn);
    root.append(&actions);

    // --- Список документов (с чекбоксами) ---
    let list_box = ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::None);
    list_box.set_vexpand(true);
    let scroll = ScrolledWindow::builder()
        .child(&list_box)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .build();
    root.append(&scroll);

    // --- Результат ---
    let result_label = Label::builder()
        .label("Готов к работе.")
        .halign(gtk4::Align::Start)
        .css_classes(["dim-label"])
        .wrap(true)
        .build();
    root.append(&result_label);

    // Сохраняем виджеты в thread_local для обработчиков событий.
    LIST_WIDGET.with(|lw| *lw.borrow_mut() = Some(list_box.clone()));
    RESULT_WIDGET.with(|rw| *rw.borrow_mut() = Some(result_label.clone()));
    PROFILE_COMBO.with(|pc| *pc.borrow_mut() = Some(profile_combo.clone()));

    // --- Обработчики ---

    // «Список документов».
    let cs_list = cs.clone();
    let pf = profile_combo.clone();
    let cat = category_entry.clone();
    let df = date_from.clone();
    let dt = date_to.clone();
    let lim = limit_entry.clone();
    list_btn.connect_clicked(move |_| {
        let (provider_id, profile_name, report_type) = match current_target(&pf) {
            Some(t) => t,
            None => {
                notify("Сначала выберите профиль и отчёт.");
                return;
            }
        };
        let filter = build_filter(&cat, &df, &dt, &lim);
        cs_list.send(crate::channels::UiCommand::ListDocuments {
            provider_id,
            profile_name,
            report_type,
            filter,
        });
        notify("Запрос списка документов…");
    });

    // «Скачать выбранные» (Browsable).
    let cs_dl = cs.clone();
    let pf2 = profile_combo.clone();
    download_btn.connect_clicked(move |_| {
        let (provider_id, profile_name, report_type) = match current_target(&pf2) {
            Some(t) => t,
            None => {
                notify("Сначала выберите профиль и отчёт.");
                return;
            }
        };
        let ids: Vec<String> = CHECKS
            .with(|c| {
                c.borrow()
                    .iter()
                    .filter(|(_, cb)| cb.is_active())
                    .map(|(id, _)| id.clone())
                    .collect()
            });
        if ids.is_empty() {
            notify("Не выбран ни один документ.");
            return;
        }
        let count = ids.len();
        cs_dl.send(crate::channels::UiCommand::Download {
            provider_id,
            profile_name,
            report_type,
            document_ids: ids,
            params: ReportParams::new(),
        });
        notify(&format!("Скачивание {count} документов…"));
    });

    // «Скачать по периоду» (Period).
    let cs_per = cs.clone();
    let pf3 = profile_combo.clone();
    let dfp = date_from.clone();
    dtp_handler(&period_btn, cs_per, pf3, dfp);

    root
}

fn dtp_handler(btn: &Button, cs: CommandSender, pf: ComboBoxText, date_from: Entry) {
    btn.connect_clicked(move |_| {
        let (provider_id, profile_name, report_type) = match current_target(&pf) {
            Some(t) => t,
            None => {
                notify("Сначала выберите профиль и отчёт.");
                return;
            }
        };
        let period = date_from.text().to_string();
        let params = ReportParams {
            period: Some(period.clone()),
            ..Default::default()
        };
        cs.send(crate::channels::UiCommand::Download {
            provider_id,
            profile_name,
            report_type,
            document_ids: Vec::new(),
            params,
        });
        notify(&format!("Генерация отчёта за период {period}…"));
    });
}

/// Возвращает (provider_id, profile_name, report_type) из выбранного профиля
/// и текущего типа отчёта. provider_id определяется по префиксу report_type.
///
/// Формат записи в combo: `name [provider]`.
fn current_target(profile_combo: &ComboBoxText) -> Option<(String, String, String)> {
    let report_type = REPORT_TYPE.with(|r| r.borrow().clone());
    if report_type.is_empty() {
        return None;
    }
    let raw = profile_combo.active_text()?.to_string();
    // "Ozon-1 [ozon]" -> name="Ozon-1", provider из report_type.
    let profile_name = raw.split(" [").next()?.to_string();
    let provider_id = report_type.split('.').next()?.to_string();
    Some((provider_id, profile_name, report_type))
}

fn build_filter(
    category: &Entry,
    date_from: &Entry,
    date_to: &Entry,
    limit: &Entry,
) -> DocumentFilter {
    let mut f = DocumentFilter::default();
    let cat = category.text().to_string();
    if !cat.is_empty() {
        f.category = Some(cat);
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

fn notify(msg: &str) {
    RESULT_WIDGET.with(|rw| {
        if let Some(l) = rw.borrow().as_ref() {
            l.set_text(msg);
        }
    });
}

/// Обработчик: список документов получен — рисуем строки с чекбоксами.
pub fn on_documents_listed(res: &Result<Vec<DocumentEntry>, String>) {
    match res {
        Err(e) => {
            notify(&format!("Ошибка: {e}"));
        }
        Ok(docs) => {
            DOCS.with(|d| *d.borrow_mut() = docs.clone());
            render_list(docs);
            notify(&format!("Получено документов: {}", docs.len()));
        }
    }
}

fn render_list(docs: &[DocumentEntry]) {
    LIST_WIDGET.with(|lw| {
        let list_box = match lw.borrow().as_ref() {
            Some(lb) => lb.clone(),
            None => return,
        };
        // очищаем
        while let Some(child) = list_box.first_child() {
            list_box.remove(&child);
        }
        CHECKS.with(|c| c.borrow_mut().clear());

        if docs.is_empty() {
            list_box.append(&Label::new(Some("Документы не найдены.")));
            return;
        }

        // Заголовок.
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

/// Обработчик: скачивание успешно завершено.
pub fn on_download_finished(files: &[DownloadedFile]) {
    notify(&format!("Скачано файлов: {}. (Запись на диск — ЭТАП 7.)", files.len()));
}

/// Обработчик: ошибка скачивания.
pub fn on_download_error(err: &str) {
    notify(&format!("Ошибка выгрузки: {err}"));
}
