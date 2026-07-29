//! Вкладка «Настройки»: редактирование config.toml (спец. §2.7.1, гл. 06).
//!
//! Загружает текущий AppConfig, даёт редактировать ключевые поля, сохраняет.
//! Полный набор секций доступен через прямой доступ к config.toml в data_dir.

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Entry, Label, Orientation, SpinButton};
use libadwaita as adw;

use mdwf_config::AppConfig;

use crate::channels::CommandSender;

thread_local! {
    static CFG: std::rc::Rc<std::cell::RefCell<Option<AppConfig>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
}

pub fn build(cs: &CommandSender) -> GtkBox {
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

    // Загружаем текущий конфиг (по стандартному пути).
    let prov = mdwf_config::ProvisionedConfig::load_standard();
    let mut cfg = AppConfig::default();
    if let Ok(p) = &prov {
        cfg = p.raw.clone();
        CFG.with(|c| *c.borrow_mut() = Some(p.raw.clone()));
        root.append(&Label::builder()
            .label(format!("Файл конфигурации: {}", p.config_path.display()))
            .css_classes(["dim-label"])
            .halign(gtk4::Align::Start)
            .wrap(true)
            .build());
        root.append(&Label::builder()
            .label(format!("Папка данных: {}", p.data_dir.display()))
            .css_classes(["dim-label"])
            .halign(gtk4::Align::Start)
            .wrap(true)
            .build());
        root.append(&Label::builder()
            .label(format!("Каталог SQLite: {}", p.db_path.display()))
            .css_classes(["dim-label"])
            .halign(gtk4::Align::Start)
            .wrap(true)
            .build());
    }

    // --- Секция Storage ---
    root.append(&section_header("Хранилище"));

    let output_dir = labeled_entry("Папка выгрузки:", &cfg.storage.output_dir);
    root.append(&output_dir.row);

    let template = labeled_entry("Шаблон имени файла:", &cfg.storage.file_name_template);
    root.append(&template.row);

    let compute_hash = gtk4::CheckButton::builder()
        .label("Вычислять SHA-256 (для дедупликации)")
        .active(cfg.storage.compute_hash)
        .halign(gtk4::Align::Start)
        .build();
    root.append(&compute_hash);

    // --- Секция Network ---
    root.append(&section_header("Сеть"));

    let timeout = labeled_spin("Таймаут запроса (с):", f64::from(cfg.network.request_timeout_seconds));
    root.append(&timeout.row);

    let retries = labeled_spin("Макс. повторов:", f64::from(cfg.network.max_retries));
    root.append(&retries.row);

    let concurrency = labeled_spin(
        "Макс. параллельных запросов на провайдера:",
        f64::from(cfg.network.max_concurrency_per_provider),
    );
    root.append(&concurrency.row);

    // --- Секция Scheduler ---
    root.append(&section_header("Планировщик"));

    let parallel_jobs = labeled_spin(
        "Макс. параллельных задач:",
        f64::from(cfg.scheduler.max_parallel_jobs),
    );
    root.append(&parallel_jobs.row);

    let autostart = gtk4::CheckButton::builder()
        .label("Автозапуск с Windows")
        .active(cfg.scheduler.autostart_with_os)
        .halign(gtk4::Align::Start)
        .build();
    root.append(&autostart);

    // --- Секция Security ---
    root.append(&section_header("Безопасность"));

    let use_keychain = gtk4::CheckButton::builder()
        .label("Хранить секреты в OS keychain (иначе — in-memory)")
        .active(cfg.security.use_keychain)
        .halign(gtk4::Align::Start)
        .build();
    root.append(&use_keychain);

    let log_retention = labeled_spin(
        "Хранить логи (дней):",
        f64::from(cfg.security.log_retention_days),
    );
    root.append(&log_retention.row);

    // --- Кнопка сохранения ---
    let save_btn = Button::builder()
        .label("💾 Сохранить настройки")
        .css_classes(["suggested-action"])
        .halign(gtk4::Align::End)
        .margin_top(8)
        .build();

    let status_label = Label::builder()
        .label("")
        .css_classes(["dim-label"])
        .halign(gtk4::Align::Start)
        .wrap(true)
        .build();
    root.append(&status_label.clone());
    root.append(&save_btn.clone());

    // Обработчик сохранения.
    // gtk-виджеты клонируются дёшево (внутренний Arc), поэтому берём refs вручную.
    let output_dir_e = output_dir.entry.clone();
    let template_e = template.entry.clone();
    let compute_hash_cb = compute_hash.clone();
    let timeout_s = timeout.spin.clone();
    let retries_s = retries.spin.clone();
    let concurrency_s = concurrency.spin.clone();
    let parallel_jobs_s = parallel_jobs.spin.clone();
    let autostart_cb = autostart.clone();
    let use_keychain_cb = use_keychain.clone();
    let log_retention_s = log_retention.spin.clone();
    let status_lbl = status_label.clone();
    save_btn.connect_clicked(move |_| {
        let mut config = CFG.with(|c| c.borrow().clone()).unwrap_or_default();
        config.storage.output_dir = output_dir_e.text().to_string();
        config.storage.file_name_template = template_e.text().to_string();
        config.storage.compute_hash = compute_hash_cb.is_active();
        config.network.request_timeout_seconds = timeout_s.value() as u32;
        config.network.max_retries = retries_s.value() as u32;
        config.network.max_concurrency_per_provider = concurrency_s.value() as u32;
        config.scheduler.max_parallel_jobs = parallel_jobs_s.value() as u32;
        config.scheduler.autostart_with_os = autostart_cb.is_active();
        config.security.use_keychain = use_keychain_cb.is_active();
        config.security.log_retention_days = log_retention_s.value() as u32;

        let path = mdwf_config::data_dir().join("config.toml");
        match config.save(&path) {
            Ok(()) => {
                status_lbl.set_text("Настройки сохранены. Изменения применятся при перезапуске.");
                CFG.with(|c| *c.borrow_mut() = Some(config));
            }
            Err(e) => status_lbl.set_text(&format!("Ошибка сохранения: {e}")),
        }
    });

    // Подавляем неиспользуемое cs (настройки применяются через файл).
    let _ = cs;
    root
}

fn section_header(text: &str) -> Label {
    Label::builder()
        .label(text)
        .css_classes(["heading"])
        .halign(gtk4::Align::Start)
        .margin_top(8)
        .build()
}

struct LabeledEntry {
    row: GtkBox,
    entry: Entry,
}

fn labeled_entry(label: &str, value: &str) -> LabeledEntry {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.append(&Label::builder().label(label).width_chars(34).xalign(0.0).hexpand(true).build());
    let entry = Entry::builder().text(value).hexpand(true).build();
    row.append(&entry);
    LabeledEntry { row, entry }
}

struct LabeledSpin {
    row: GtkBox,
    spin: SpinButton,
}

fn labeled_spin(label: &str, value: f64) -> LabeledSpin {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.append(&Label::builder().label(label).width_chars(34).xalign(0.0).hexpand(true).build());
    let spin = SpinButton::with_range(1.0, 100_000.0, 1.0);
    spin.set_value(value);
    row.append(&spin);
    LabeledSpin { row, spin }
}
