//! Контекстная помощь по вкладке прямо на месте: кнопка «?» в строке заголовка
//! открывает popover с краткой инструкцией именно для этой вкладки.
//!
//! Использование: в build() вьюшки заменить простой заголовок на
//! `tab_help::title_row_with_help("Заголовок", &TAB_HELP_BLOCKS)`.

use gtk4::prelude::*;
use gtk4::{Align, Label, MenuButton, PolicyType, Popover, ScrolledWindow};

/// Блок справки: заголовок / абзац / маркированный список.
pub enum HelpBlock {
    /// Подзаголовок (жирный).
    H(&'static str),
    /// Абзац (перенос по словам).
    T(&'static str),
    /// Маркированный список.
    B(&'static [&'static str]),
}

/// Строка заголовка вкладки + кнопка «?» справа (контекстная помощь).
/// Заголовок растягивается, кнопка прижата вправо — выглядит как единый header.
pub fn title_row_with_help(title: &str, css: &str, blocks: &[HelpBlock]) -> gtk4::Box {
    let row = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(8)
        .build();
    row.append(&Label::builder()
        .label(title)
        .css_classes([css])
        .halign(Align::Start)
        .hexpand(true)
        .wrap(true)
        .xalign(0.0)
        .build());
    row.append(&help_button(blocks));
    row
}

/// Кнопка «?» — открывает popover со справкой по вкладке.
pub fn help_button(blocks: &[HelpBlock]) -> MenuButton {
    let btn = MenuButton::builder()
        .icon_name("dialog-question-symbolic")
        .tooltip_text("Помощь по этой вкладке")
        .build();

    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .height_request(460)
        .width_request(560)
        .build();
    let col = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(4)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    for b in blocks {
        let l = Label::new(None);
        l.set_wrap(true);
        l.set_xalign(0.0);
        l.set_halign(Align::Start);
        l.set_hexpand(true);
        l.set_width_request(520);
        match b {
            HelpBlock::H(t) => {
                l.set_markup(&format!("<b>{t}</b>"));
                l.set_margin_top(6);
            }
            HelpBlock::T(t) => {
                l.set_markup(t);
            }
            HelpBlock::B(items) => {
                l.set_margin_start(10);
                // Список склеиваем в один Label с переносами (пункт — строка).
                let text = items
                    .iter()
                    .map(|it| format!("• {it}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                l.set_markup(&text);
            }
        }
        col.append(&l);
    }
    scroll.set_child(Some(&col));
    let pop = Popover::new();
    pop.set_child(Some(&scroll));
    btn.set_popover(Some(&pop));
    btn
}
