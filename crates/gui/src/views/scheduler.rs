//! Вкладка «Планировщик» (каркас; наполнение — ЭТАП 11).

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation};

pub fn build(_cs: &crate::channels::CommandSender) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.set_margin_top(16);
    root.set_margin_bottom(16);

    root.append(&Label::builder()
        .label("Планировщик (cron)")
        .css_classes(["title-2"])
        .halign(gtk4::Align::Start)
        .build());

    root.append(&Label::builder()
        .label("Ежемесячная/ежедневная автозагрузка по cron — будет добавлена на ЭТАПЕ 11.\nШаблоны: 0 2 1 * * (1-го числа), 0 9 * * * (ежедневно).")
        .css_classes(["dim-label"])
        .halign(gtk4::Align::Start)
        .wrap(true)
        .build());

    root
}
