//! Вкладка «Журнал» — лента событий приложения (выгрузки, ошибки, запуски
//! расписаний). Хранит последние 500 записей (вытеснение старых).
//!
//! Источник записей — `UiEvent::Log(LogEntry)`, эмитится из app-loop (старт/
//! успех/ошибка выгрузки, запуск расписания). Маршрутизация — в main_window.rs.
//! История персистится в SQLite (таблица `journal`): при старте лента
//! восстанавливается из БД (`UiEvent::JournalLoaded`), «Очистить» чистит и БД.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use chrono::{DateTime, Utc};
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, Label, ListBox, ListBoxRow, Orientation, PolicyType, ScrolledWindow,
};

use crate::channels::{CommandSender, LogEntry, LogKind, UiCommand};

/// Кап на число записей в ленте (защита от безграничного роста памяти).
/// Синхронизирован с БД: каталожный кап = тот же JOURNAL_KEEP.
const MAX_ENTRIES: usize = mdwf_storage::JOURNAL_KEEP;

thread_local! {
    static W_LIST: Rc<RefCell<Option<ListBox>>> = Rc::new(RefCell::new(None));
    /// Строки журнала; front = самая свежая. Храним ListBoxRow, чтобы удалять
    /// старые при превышении MAX_ENTRIES.
    static ROWS: Rc<RefCell<VecDeque<ListBoxRow>>> = Rc::new(RefCell::new(VecDeque::new()));
}

pub fn build(cs: &CommandSender) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.set_margin_top(16);
    root.set_margin_bottom(16);

    // Заголовок + кнопка «Очистить» (чистит и БД — через команду).
    let header = GtkBox::new(Orientation::Horizontal, 12);
    header.append(
        &crate::widgets::tab_help::title_row_with_help("Журнал", "title-2", LOGS_HELP),
    );
    let clear_btn = Button::with_label("Очистить");
    clear_btn.add_css_class("destructive-action");
    {
        let cs = cs.clone();
        clear_btn.connect_clicked(move |_| {
            cs.send(UiCommand::ClearJournal);
        });
    }
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

/// Хук: заменить ленту историей из БД (UiEvent::JournalLoaded).
/// `entries` — свежие первыми (как отдаёт Catalog::list_journal).
pub fn set_entries(entries: Vec<LogEntry>) {
    clear();
    // Идём от старых к новым и prepend-им: свежая окажется сверху — как в append.
    for entry in entries.into_iter().rev() {
        append(entry);
    }
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

/// Локальное отображение времени записи: сегодня — «ЧЧ:ММ:СС», старше —
/// «ДД.ММ.ГГГГ ЧЧ:ММ» (журнал персистится, записи бывают прошлых дней).
fn fmt_local_time(created_at: DateTime<Utc>) -> String {
    let local = created_at.with_timezone(&chrono::Local);
    if local.date_naive() == chrono::Local::now().date_naive() {
        local.format("%H:%M:%S").to_string()
    } else {
        local.format("%d.%m.%Y %H:%M").to_string()
    }
}

/// Создаёт строку ленты: время | значок-уровня | сообщение.
fn make_row(entry: &LogEntry) -> ListBoxRow {
    let row = ListBoxRow::builder().selectable(false).build();
    let box_ = GtkBox::new(Orientation::Horizontal, 10);
    box_.set_margin_start(8);
    box_.set_margin_end(8);
    box_.set_margin_top(4);
    box_.set_margin_bottom(4);

    let time = Label::builder()
        .label(fmt_local_time(entry.created_at))
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
    crate::widgets::tab_help::HelpBlock::T("Лента событий: скачивание (успех/ошибка), запуски расписаний, сбои."),
    crate::widgets::tab_help::HelpBlock::B(&[
        "Хранятся последние 500 записей — и в этом сеансе, и между запусками (в БД).",
        "«Очистить» — удаляет историю целиком (ленту и БД).",
        "Подробные логи-файлы: %APPDATA%\\mdwf\\logs.",
    ]),
];
