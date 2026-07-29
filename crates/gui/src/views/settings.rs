//! Вкладка «Настройки» (каркас; полное наполнение — ЭТАП 7).

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation};

pub fn build(_cs: &crate::channels::CommandSender) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.set_margin_top(16);
    root.set_margin_bottom(16);

    root.append(&Label::builder()
        .label("Настройки")
        .css_classes(["title-2"])
        .halign(gtk4::Align::Start)
        .build());

    root.append(&Label::builder()
        .label("config.toml + сохранённые фильтры будут доступны здесь на ЭТАПЕ 7.\nПараметры: папка выгрузки, шаблон имён, тема, лимиты сети, rate-limit retry.")
        .css_classes(["dim-label"])
        .halign(gtk4::Align::Start)
        .wrap(true)
        .build());

    root
}
