//! Вкладка «Отчёты»: выбор провайдера, список доступных отчётов,
//! подгрузка списка отчётов провайдера.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, ComboBoxText, Label, ListBox, Orientation,
};

use crate::channels::{CommandSender, ProviderInfo, ReportInfo};
use crate::views::download::set_report_type;

thread_local! {
    static REPORTS: Rc<RefCell<Vec<ReportInfo>>> = Rc::new(RefCell::new(Vec::new()));
}

/// Хук: провайдеры загружены (для обновления combo, если нужно).
pub fn on_providers_loaded(_providers: &[ProviderInfo]) {
    // На ЭТАПЕ 6 свяжем с реальным combo провайдеров.
}

pub fn build(cs: &CommandSender) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.set_margin_top(16);
    root.set_margin_bottom(16);

    let title = Label::builder()
        .label("Доступные отчёты")
        .css_classes(["title-2"])
        .halign(gtk4::Align::Start)
        .build();
    root.append(&title);

    root.append(&Label::builder()
        .label("Выберите провайдера, чтобы увидеть список отчётов. Клик по отчёту переносит его во вкладку «Загрузка».")
        .css_classes(["dim-label"])
        .halign(gtk4::Align::Start)
        .wrap(true)
        .build());

    // Выбор провайдера.
    let row = GtkBox::new(Orientation::Horizontal, 8);
    let provider_combo = ComboBoxText::new();
    provider_combo.append_text("test");
    provider_combo.append_text("ozon");
    provider_combo.append_text("wildberries");
    provider_combo.set_active(Some(0));
    let load_btn = Button::builder().label("Загрузить список отчётов").build();
    row.append(&Label::new(Some("Провайдер:")));
    row.append(&provider_combo);
    row.append(&load_btn);
    root.append(&row);

    // Список отчётов.
    let list_box = ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::Single);
    list_box.set_vexpand(true);
    root.append(&list_box);

    let cs1 = cs.clone();
    let list_box1 = list_box.clone();
    load_btn.connect_clicked(move |_| {
        // очищаем список
        while let Some(child) = list_box1.first_child() {
            list_box1.remove(&child);
        }
        list_box1.append(&Label::new(Some("Загрузка…")));
        let pid = provider_combo
            .active_text()
            .map(|s| s.to_string())
            .unwrap_or_default();
        cs1.send(crate::channels::UiCommand::LoadReports(pid));
    });

    // Клик по отчёту -> переносим его во вкладку «Загрузка».
    list_box.connect_selected_rows_changed(move |lb| {
        if let Some(row) = lb.selected_row() {
            if let Some(label) = row.child().and_then(|c| c.downcast::<Label>().ok()) {
                let text = label.text().to_string();
                // text имеет вид "type_id — display_name"
                let type_id = text.split(" — ").next().unwrap_or("").to_string();
                if !type_id.is_empty() {
                    set_report_type(&type_id);
                }
            }
        }
    });

    root
}

/// Обработчик «отчёты загружены» — перерисовывает список.
pub fn on_reports_loaded(res: &Result<Vec<ReportInfo>, String>) {
    REPORTS.with(|r| {
        *r.borrow_mut() = res.as_ref().cloned().unwrap_or_default();
    });
    // Полная перерисовка списка будет через сигнал; пока сохраняем во thread_local.
    // (На ЭТАПЕ 6 свяжем с GObject-моделью для автообновления.)
}
