//! Вкладка «Журнал» — лента событий приложения (выгрузки, ошибки, запуски
//! расписаний). Хранит последние 500 записей (вытеснение старых).
//!
//! Источник записей — `UiEvent::Log(LogEntry)`, эмитится из app-loop (старт/
//! успех/ошибка выгрузки, запуск расписания). Маршрутизация — в main_window.rs.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, Label, ListBox, ListBoxRow, Orientation, PolicyType, ScrolledWindow,
};

use crate::channels::{LogEntry, LogKind};

/// Кап на число записей в ленте (защита от безграничного роста памяти).
const MAX_ENTRIES: usize = 500;

thread_local! {
    static W_LIST: Rc<RefCell<Option<ListBox>>> = Rc::new(RefCell::new(None));
    /// Строки журнала; front = самая свежая. Храним ListBoxRow, чтобы удалять
    /// старые при превышении MAX_ENTRIES.
    static ROWS: Rc<RefCell<VecDeque<ListBoxRow>>> = Rc::new(RefCell::new(VecDeque::new()));
}

pub fn build(_cs: &crate::channels::CommandSender) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.set_margin_top(16);
    root.set_margin_bottom(16);

    // Заголовок + кнопка «Очистить».
    let header = GtkBox::new(Orientation::Horizontal, 12);
    header.append(
        &crate::widgets::tab_help::title_row_with_help("Журнал", "title-2", &LOGS_HELP),
    );
    let clear_btn = Button::with_label("Очистить");
    clear_btn.add_css_class("destructive-action");
    clear_btn.connect_clicked(move |_| clear());
    header.append(&clear_btn);
    root.append(&header);

    // Лента: ScrolledWindow + ListBox.
    let list = ListBox::builder()
        .selection_mode(gtk4::SelectionMode::None)
        .show_separators(true)
        .build();
    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .vexpand(true)
        .child(&list)
        .build();
    root.append(&scroll);

    // Placeholder, пока журнал пуст.
    list.set_placeholder(
        Some(
            &Label::builder()
                .label("События выгрузок будут отображаться здесь.")
                .css_classes(["dim-label"])
                .margin_top(24)
                .build(),
        ),
    );

    W_LIST.with(|w| *w.borrow_mut() = Some(list));
    ROWS.with(|r| r.borrow_mut().clear());
    root
}

/// Хук: добавить запись в ленту (свежая — сверху). Капается на MAX_ENTRIES.
pub fn append(entry: LogEntry) {
    W_LIST.with(|w| {
        let list = match w.borrow().clone() {
            Some(l) => l,
            None => return,
        };
        let row = make_row(&entry);
        list.prepend(&row);
        ROWS.with(|r| {
            let mut rows = r.borrow_mut();
            rows.push_front(row);
            // Вытеснение самых старых (с конца).
            while rows.len() > MAX_ENTRIES {
                if let Some(old) = rows.pop_back() {
                    list.remove(&old);
                } else {
                    break;
                }
            }
        });
    });
}

/// Очищает ленту (кнопка «Очистить»).
pub fn clear() {
    W_LIST.with(|w| {
        if let Some(list) = w.borrow().as_ref() {
            // Удаляем известные строки.
            ROWS.with(|r| {
                for row in r.borrow().iter() {
                    list.remove(row);
                }
                r.borrow_mut().clear();
            });
        }
    });
}

/// Создаёт строку ленты: время | значок-уровень | сообщение.
fn make_row(entry: &LogEntry) -> ListBoxRow {
    let row = ListBoxRow::builder().selectable(false).build();
    let box_ = GtkBox::new(Orientation::Horizontal, 10);
    box_.set_margin_start(8);
    box_.set_margin_end(8);
    box_.set_margin_top(4);
    box_.set_margin_bottom(4);

    let time = Label::builder()
        .label(&entry.timestamp)
        .css_classes(["dim-label", "monospace"])
        .xalign(0.0)
        .build();
    let (icon, css) = match entry.kind {
        LogKind::Info => ("ℹ", "dim-label"),
        LogKind::Success => ("✓", "success"),
        LogKind::Error => ("✕", "error"),
    };
    let kind_lbl = Label::builder()
        .label(icon)
        .css_classes([css])
        .xalign(0.0)
        .build();
    let msg = Label::builder()
        .label(&entry.message)
        .xalign(0.0)
        .hexpand(true)
        .wrap(true)
        .build();
    box_.append(&time);
    box_.append(&kind_lbl);
    box_.append(&msg);
    row.set_child(Some(&box_));
    row
}

/// Контекстная помощь вкладки «Журнал» (кнопка «?» в заголовке).
const LOGS_HELP: &[crate::widgets::tab_help::HelpBlock] = &[
    crate::widgets::tab_help::HelpBlock::H("Что здесь"),
    crate::widgets::tab_help::HelpBlock::T("Лента событий: выгрузки (успех/ошибка), запуски расписаний, сбои."),
    crate::widgets::tab_help::HelpBlock::B(&[
        "Хранятся последние 500 записей.",
        "«Очистить» — очищает экран (не трогает историю в БД).",
        "Подробные логи-файлы: %APPDATA%\\mdwf\\logs.",
    ]),
];
