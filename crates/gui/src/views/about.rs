//! Вкладка «О программе»: название, версия, автор, лицензия, исходный код,
//! благодарности и оговорка о товарных знаках.

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, LinkButton, Orientation};

pub fn build() -> GtkBox {
    let root = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .margin_start(16)
        .margin_end(16)
        .margin_top(24)
        .margin_bottom(16)
        .build();

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
        .label("Автоматизированная выгрузка финансовых документов с маркетплейсов через официальные API.")
        .halign(gtk4::Align::Center)
        .wrap(true)
        .justify(gtk4::Justification::Center)
        .build());

    // --- Авторство и лицензия ---
    root.append(&section(&[
        "Автор: PrS",
        "© 2026 PrS. Все права защищены.",
        "Лицензия: MIT или Apache License 2.0 (на выбор пользователя).",
    ]));

    // --- Исходный код ---
    let repo = LinkButton::builder()
        .uri("https://github.com/bigsprut/MPDocsLoad")
        .label("Исходный код — github.com/bigsprut/MPDocsLoad")
        .halign(gtk4::Align::Center)
        .build();
    root.append(&repo);

    // --- Благодарности (см. NOTICE в репозитории) ---
    root.append(&section(&[
        "Собрано на Rust; интерфейс — GTK4 + libadwaita.",
        "Иконки типов файлов — vscode-icons (MIT). Установщик — Inno Setup.",
        "Благодарим проекты Rust, GTK и всех авторов используемых библиотек.",
    ]));

    // --- Товарные знаки ---
    root.append(&Label::builder()
        .label("Ozon и Wildberries — торговые марки соответствующих правообладателей.\nMDWF не аффилирован с ними и использует только официальные публичные API.")
        .halign(gtk4::Align::Center)
        .wrap(true)
        .justify(gtk4::Justification::Center)
        .css_classes(["dim-label"])
        .build());

    root
}

/// Центрированный блок строк с тонким разделителем сверху.
fn section(lines: &[&str]) -> GtkBox {
    let sep = gtk4::Separator::builder()
        .orientation(Orientation::Horizontal)
        .margin_top(8)
        .margin_bottom(4)
        .css_classes(["dim-label"])
        .build();
    let b = GtkBox::new(Orientation::Vertical, 4);
    b.append(&sep);
    for l in lines {
        b.append(&Label::builder()
            .label(*l)
            .halign(gtk4::Align::Center)
            .wrap(true)
            .build());
    }
    b
}
