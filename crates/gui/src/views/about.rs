//! Вкладка «О программе».

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation};

pub fn build() -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.set_margin_top(16);
    root.set_margin_bottom(16);

    root.append(&Label::builder()
        .label("Marketplace Downloader Framework")
        .css_classes(["title-1"])
        .halign(gtk4::Align::Center)
        .build());

    let version = format!("Версия {}", env!("CARGO_PKG_VERSION"));
    root.append(&Label::builder()
        .label(&version)
        .css_classes(["dim-label"])
        .halign(gtk4::Align::Center)
        .build());

    root.append(&Label::builder()
        .label("Автоматизированная выгрузка финансовых документов с маркетплейсов через официальные API.\n\nТолько официальные API • GTK4 + libadwaita • Rust")
        .halign(gtk4::Align::Center)
        .wrap(true)
        .justify(gtk4::Justification::Center)
        .build());

    root
}
