//! Вкладка «Отчёты»: список доступных отчётов активного магазина.
//!
//! Магазин (маркетплейс + профиль) выбирается во вкладке «Магазин» — здесь
//! только read-only индикатор и список отчётов активного провайдера. Клик по
//! отчёту — подсказка перейти во вкладку «Загрузка» для выгрузки.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, ListBox, Orientation};

use crate::channels::{CommandSender, ReportInfo};

thread_local! {
    static REPORTS: Rc<RefCell<Vec<ReportInfo>>> = Rc::new(RefCell::new(Vec::new()));
    static LIST_WIDGET: Rc<RefCell<Option<ListBox>>> = Rc::new(RefCell::new(None));
    static W_SHOP_LABEL: Rc<RefCell<Option<Label>>> = Rc::new(RefCell::new(None));
    /// Активный провайдер (для reload по кнопке «Обновить»).
    static ACTIVE_PROVIDER: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    static CMD: Rc<RefCell<Option<CommandSender>>> = Rc::new(RefCell::new(None));
}

pub fn build(cs: &CommandSender) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.set_margin_top(16);
    root.set_margin_bottom(16);

    root.append(&crate::widgets::tab_help::title_row_with_help(
        "Доступные отчёты",
        "title-2",
        REPORTS_HELP,
    ));

    root.append(&Label::builder()
        .label("Магазин выбирается во вкладке «Магазин». Здесь — список отчётов активного маркетплейса. Клик по отчёту переносит его во вкладку «Загрузка».")
        .css_classes(["dim-label"])
        .halign(gtk4::Align::Start)
        .wrap(true)
        .build());

    // Read-only индикатор активного магазина + кнопка обновления.
    let row = GtkBox::new(Orientation::Horizontal, 8);
    let shop_label = Label::builder()
        .label("Магазин: не выбран — выберите во вкладке «Магазин».")
        .css_classes(["heading"])
        .halign(gtk4::Align::Start)
        .wrap(true)
        .build();
    let load_btn = super::icon_button("Обновить", "view-refresh-symbolic");
    load_btn.set_tooltip_text(Some("Перезагрузить список отчётов активного маркетплейса"));
    row.append(&shop_label);
    row.append(&load_btn);
    root.append(&row);

    // Список отчётов.
    let list_box = ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::Single);
    list_box.set_vexpand(true);
    root.append(&list_box);

    W_SHOP_LABEL.with(|w| *w.borrow_mut() = Some(shop_label.clone()));
    LIST_WIDGET.with(|lw| *lw.borrow_mut() = Some(list_box.clone()));
    CMD.with(|c| *c.borrow_mut() = Some(cs.clone()));

    // «Обновить» — перезагрузить отчёты активного провайдера.
    let cs1 = cs.clone();
    load_btn.connect_clicked(move |_| {
        let pid = ACTIVE_PROVIDER.with(|p| p.borrow().clone());
        if let Some(pid) = pid {
            // Очищаем список + индикатор загрузки.
            if let Some(lb) = LIST_WIDGET.with(|lw| lw.borrow().clone()) {
                while let Some(child) = lb.first_child() {
                    lb.remove(&child);
                }
                lb.append(&Label::new(Some("Загрузка…")));
            }
            cs1.send(crate::channels::UiCommand::LoadReports(pid));
        }
    });

    // Клик по отчёту → предвыбрать его в «Загрузке» и переключиться на эту вкладку
    // (как обещает строка помощи). Индекс строки совпадает с индексом в REPORTS.
    list_box.connect_selected_rows_changed(move |lb| {
        let Some(row) = lb.selected_row() else { return };
        let idx = row.index();
        if idx < 0 {
            return;
        }
        let Some(rtype) =
            REPORTS.with(|r| r.borrow().get(idx as usize).map(|ri| ri.type_id.clone()))
        else {
            return;
        };
        crate::views::download::set_pending_report(&rtype);
        // Переключаем вкладку: поднимаемся по дереву до Stack и меняем видимый ребёнок.
        if let Some(stack_w) = lb.ancestor(gtk4::Stack::static_type()) {
            if let Ok(stack) = stack_w.downcast::<gtk4::Stack>() {
                stack.set_visible_child_name(crate::channels::ViewId::Download.as_str());
            }
        }
    });

    root
}

/// Хук: активный магазин изменён → обновляем read-only лейбл и перезагружаем
/// список отчётов нового провайдера.
pub fn on_active_shop_changed(
    provider_id: &str,
    provider_display_name: &str,
    seller_name: Option<&str>,
    profile_name: &str,
) {
    ACTIVE_PROVIDER.with(|p| *p.borrow_mut() = Some(provider_id.to_string()));
    W_SHOP_LABEL.with(|w| {
        if let Some(l) = w.borrow().as_ref() {
            let display = seller_name.unwrap_or(profile_name);
            l.set_text(&format!("Магазин: {provider_display_name} — {display}"));
        }
    });
    // Перезагружаем отчёты.
    if let Some(cs) = CMD.with(|c| c.borrow().clone()) {
        if let Some(lb) = LIST_WIDGET.with(|lw| lw.borrow().clone()) {
            while let Some(child) = lb.first_child() {
                lb.remove(&child);
            }
            lb.append(&Label::new(Some("Загрузка…")));
        }
        cs.send(crate::channels::UiCommand::LoadReports(provider_id.to_string()));
    }
}

/// Обработчик «отчёты загружены» — перерисовывает список.
pub fn on_reports_loaded(res: &Result<Vec<ReportInfo>, String>) {
    let list_box = LIST_WIDGET.with(|lw| lw.borrow().clone());
    let Some(list_box) = list_box else { return };

    // Очищаем список.
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }

    match res {
        Err(e) => {
            let lbl = Label::new(Some(&format!("Ошибка: {e}")));
            lbl.set_css_classes(&["error"]);
            list_box.append(&lbl);
        }
        Ok(reports) => {
            REPORTS.with(|r| *r.borrow_mut() = reports.clone());
            if reports.is_empty() {
                list_box.append(&Label::new(Some("Отчётов не найдено.")));
                return;
            }
            for r in reports {
                // Показываем только человекочитаемое имя; технический type_id —
                // в tooltip для справки (не в основном тексте).
                let row = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
                let label = Label::builder()
                    .label(&r.display_name)
                    .halign(gtk4::Align::Start)
                    .build();
                row.append(&label);
                if let Some(cab) = &r.cabinet_path {
                    let cab_label = Label::builder()
                        .label(format!("ЛК: {cab}"))
                        .halign(gtk4::Align::Start)
                        .css_classes(["dim-label"])
                        .build();
                    row.append(&cab_label);
                }
                let mode = if r.is_browsable {
                    "список документов"
                } else {
                    "по периоду"
                };
                let tooltip = match &r.cabinet_path {
                    Some(cab) => format!(
                        "{}
Категория: {}
Режим: {}
Идентификатор: {}
В ЛК: {}",
                        r.display_name, r.category, mode, r.type_id, cab
                    ),
                    None => format!(
                        "{}
Категория: {}
Режим: {}
Идентификатор: {}",
                        r.display_name, r.category, mode, r.type_id
                    ),
                };
                label.set_tooltip_text(Some(&tooltip));
                list_box.append(&row);
            }
        }
    }
}

/// Контекстная помощь вкладки «Отчёты» (кнопка «?» в заголовке).
const REPORTS_HELP: &[crate::widgets::tab_help::HelpBlock] = &[
    crate::widgets::tab_help::HelpBlock::H("Что здесь"),
    crate::widgets::tab_help::HelpBlock::T("Каталог отчётов активного магазина: описание, режим скачивания и тип периода. Клик по отчёту переносит его во вкладку «Загрузка»."),
    crate::widgets::tab_help::HelpBlock::H("Режимы скачивания"),
    crate::widgets::tab_help::HelpBlock::B(&[
        "«Список» — скачивание через вкладку «Загрузка»: «Список документов» → отметьте галочками → «Скачать выбранные».",
        "«Период» — кнопка «Скачать по периоду» во вкладке «Загрузка».",
    ]),
    crate::widgets::tab_help::HelpBlock::H("Тип периода (в описании отчёта)"),
    crate::widgets::tab_help::HelpBlock::B(&[
        "Месячный — выгружается по одному месяцу; за квартал/год программа пройдёт по месяцам автоматически.",
        "Диапазонный — один запрос за весь выбранный интервал дат.",
        "Без периода — остатки/справочники: дата не нужна.",
    ]),
];
