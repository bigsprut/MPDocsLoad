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

    root.append(&crate::widgets::tab_help::title_row_with_help(
        "Настройки",
        "title-2",
        SETTINGS_HELP,
    ));

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
    // Кнопка выбора папки через системный диалог (FileChooser).
    {
        let entry = output_dir.entry.clone();
        let browse_btn = Button::builder()
            .icon_name("folder-open-symbolic")
            .tooltip_text("Выбрать папку в диалоге")
            .build();
        browse_btn.connect_clicked(move |_| {
            let dlg = gtk4::FileChooserDialog::builder()
                .title("Выберите папку выгрузки")
                .action(gtk4::FileChooserAction::SelectFolder)
                .modal(true)
                .build();
            dlg.add_button("Отмена", gtk4::ResponseType::Cancel);
            dlg.add_button("Выбрать", gtk4::ResponseType::Accept);
            // Стартовая папка — текущее значение поля (если существует).
            if let Some(cur) = std::path::Path::new(&entry.text()).parent() {
                let _ = dlg.set_current_folder(Some(&gtk4::gio::File::for_path(cur)));
            } else if let Some(doc) = std::env::var_os("USERPROFILE") {
                let doc = std::path::Path::new(&doc).join("Documents");
                let _ = dlg.set_current_folder(Some(&gtk4::gio::File::for_path(doc)));
            }
            let entry_for_dlg = entry.clone();
            dlg.connect_response(move |d, resp| {
                if resp == gtk4::ResponseType::Accept {
                    if let Some(file) = d.file() {
                        if let Some(path) = file.path() {
                            entry_for_dlg.set_text(&path.display().to_string());
                        }
                    }
                }
                d.destroy();
            });
            dlg.show();
        });
        // Вставляем кнопку после поля (в конец строки).
        output_dir.row.append(&browse_btn);
    }
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

    let timeout = labeled_spin(
        "Таймаут запроса (с):",
        f64::from(cfg.network.request_timeout_seconds),
        5.0,
        300.0,
        1.0,
    );
    root.append(&timeout.row);

    let retries = labeled_spin("Макс. повторов:", f64::from(cfg.network.max_retries), 0.0, 20.0, 1.0);
    root.append(&retries.row);

    let concurrency = labeled_spin(
        "Макс. параллельных запросов на провайдера:",
        f64::from(cfg.network.max_concurrency_per_provider),
        1.0,
        20.0,
        1.0,
    );
    root.append(&concurrency.row);

    // --- Секция Scheduler ---
    root.append(&section_header("Расписания"));

    let parallel_jobs = labeled_spin(
        "Макс. параллельных задач:",
        f64::from(cfg.scheduler.max_parallel_jobs),
        1.0,
        20.0,
        1.0,
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
        1.0,
        365.0,
        1.0,
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
        let template_text = template_e.text().to_string();
        if template_text.trim().is_empty() {
            status_lbl.set_text("Шаблон имени файла не может быть пустым — оставьте плейсхолдеры (напр. {report}_{period}.{ext}).");
            return;
        }
        let mut config = CFG.with(|c| c.borrow().clone()).unwrap_or_default();
        config.storage.output_dir = output_dir_e.text().to_string();
        config.storage.file_name_template = template_text;
        config.storage.compute_hash = compute_hash_cb.is_active();
        config.network.request_timeout_seconds = timeout_s.value() as u32;
        config.network.max_retries = retries_s.value() as u32;
        config.network.max_concurrency_per_provider = concurrency_s.value() as u32;
        config.scheduler.max_parallel_jobs = parallel_jobs_s.value() as u32;
        config.scheduler.autostart_with_os = autostart_cb.is_active();
        config.security.use_keychain = use_keychain_cb.is_active();
        config.security.log_retention_days = log_retention_s.value() as u32;

        let path = mdwf_config::data_dir().join("config.toml");
        // Предупредим, если в шаблоне нет {ext} — файлы могут остаться без расширения.
        let has_ext = config.storage.file_name_template.contains("{ext}");
        match config.save(&path) {
            Ok(()) => {
                status_lbl.set_text(if has_ext {
                    "Настройки сохранены. Изменения применятся при перезапуске."
                } else {
                    "Сохранено. Внимание: в шаблоне нет {ext} — файлы могут остаться без расширения. Применится при перезапуске."
                });
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

fn labeled_spin(label: &str, value: f64, min: f64, max: f64, step: f64) -> LabeledSpin {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.append(&Label::builder().label(label).width_chars(34).xalign(0.0).hexpand(true).build());
    let spin = SpinButton::with_range(min, max, step);
    spin.set_value(value);
    row.append(&spin);
    LabeledSpin { row, spin }
}

/// Контекстная помощь вкладки «Настройки» (кнопка «?» в заголовке).
const SETTINGS_HELP: &[crate::widgets::tab_help::HelpBlock] = &[
    crate::widgets::tab_help::HelpBlock::H("Что здесь"),
    crate::widgets::tab_help::HelpBlock::T("Основные параметры сохранения скачанных файлов. Применяются к <b>новым</b> скачиваниям после кнопки «Сохранить»."),
    crate::widgets::tab_help::HelpBlock::H("Поля"),
    crate::widgets::tab_help::HelpBlock::B(&[
        "Папка выгрузки — куда сохранять файлы; кнопка 📁 выбирает папку в диалоге.",
        "Шаблон имени файла — плейсхолдеры {provider} {profile} {report} {period} {doc_id} {doc_date}.",
        "Структура папок — например, по маркетплейсу и году.",
        "SHA-256 — дедупликация повторных выгрузок.",
    ]),
    crate::widgets::tab_help::HelpBlock::T("Файлы складываются в <папку>\\{маркетплейс}\\{год}\\. Полный конфиг: %APPDATA%\\mdwf\\config.toml."),
];
