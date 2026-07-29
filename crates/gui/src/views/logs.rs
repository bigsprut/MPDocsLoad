//! Вкладка «Журнал» (каркас; наполнение — позже).

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation};

pub fn build(_cs: &crate::channels::CommandSender) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.set_margin_top(16);
    root.set_margin_bottom(16);

    root.append(&Label::builder()
        .label("Журнал")
        .css_classes(["title-2"])
        .halign(gtk4::Align::Start)
        .build());

    root.append(&Label::builder()
        .label("События приложения и логи выгрузок будут отображаться здесь.")
        .css_classes(["dim-label"])
        .halign(gtk4::Align::Start)
        .build());

    root
}
