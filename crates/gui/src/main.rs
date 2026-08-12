//! # mdwf-gui
//!
//! GUI на GTK4 + libadwaita (спец. §2.5, гл. 04, ADR-002).
//!
//! Принцип «никакой бизнес-логики в UI» (спец. §2.5.2): окна только отображают
//! состояние и отправляют команды в доменный слой. Асинхронные задачи tokio
//! communiцируют с GTK через `glib::MainContext`.

// GUI-subsystem: НЕ аллоцировать консольное окно при запуске (иначе рядом с GUI
// появляется чёрное окно терминала). Только для GUI-бинаря; CLI (mdwf.exe)
// остаётся console-subsystem — ему терминал нужен. На non-Windows — no-op.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::new_without_default)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
// Pedantic-линты, избыточные для gtk-rs UI-кода (handlers длинные,
// API часто требует owned-значения и тривиальные обёртки):
#![allow(clippy::too_many_lines)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::unnecessary_lazy_evaluations)]
#![allow(clippy::trivially_copy_pass_by_ref)]
#![allow(clippy::unused_async)]
#![allow(clippy::while_let_loop)]
#![allow(clippy::manual_strip)]
#![allow(clippy::single_match_else)]
#![allow(clippy::similar_names)]
#![allow(clippy::struct_field_names)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::assigning_clones)]
// dead_code: GUI содержит каркасы вьюшек и хуки для будущих этапов (6/7/11).
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(deprecated)]
#![allow(unused_doc_comments)]

mod app;
mod channels;
mod theme;
mod views;
mod widgets;

use std::process::ExitCode;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

pub use app::App;

/// Relocatable-бандл: если рядом с exe есть `share/` (релизный дистрибутив),
/// настраиваем env так, чтобы GTK/libadwaita/gdk-pixbuf нашли иконки, схемы и
/// лоадеры рядом с приложением. На dev-машине (target/debug) — нет share/, no-op
/// (но GSK_RENDERER ниже применяется ВСЕГДА — и в dev, и в бандле).
fn setup_bundle_env() {
    // Рендерер GTK4. Дефолтный NGL на некоторых Windows-машинах с определёнными
    // GPU-драйверами даёт ЧЁРНОЕ окно (известная проблема GTK4-on-Windows).
    // `gl` (legacy GL-рендерер) совместим с практически любой GPU/драйвером и
    // полностью достаточен для business-app вроде MDWF (формы/данные, без
    // тяжёлой графики). Пользователь может переопределить через env (напр. cairo).
    if std::env::var_os("GSK_RENDERER").is_none() {
        std::env::set_var("GSK_RENDERER", "gl");
    }

    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(dir) = exe.parent() else { return };
    let share = dir.join("share");
    if !share.exists() {
        return; // не бандл — пропускаем (dev-сборка, MSYS2 в PATH)
    }
    // XDG_DATA_DIRS → share/ (иконки Adwaita/hicolor + glib-2.0/schemas).
    // Разделитель на Windows — «;».
    let prev = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    let xdg = if prev.is_empty() {
        share.display().to_string()
    } else {
        format!("{prev};{}", share.display())
    };
    std::env::set_var("XDG_DATA_DIRS", xdg);
    // gdk-pixbuf loaders.cache (если есть — регенерируется инсталлятором/скриптом).
    let loaders_cache = dir
        .join("lib")
        .join("gdk-pixbuf-2.0")
        .join("2.10.0")
        .join("loaders.cache");
    if loaders_cache.exists() {
        std::env::set_var("GDK_PIXBUF_MODULE_FILE", loaders_cache);
    }
    tracing::debug!(exe_dir = %dir.display(), "relocatable bundle env set");
}

fn main() -> Result<ExitCode> {
    // Relocatable GTK-бандл: настроить env на соседние share/lib ДО gtk::init,
    // чтобы иконки (Adwaita), gsettings-схемы и gdk-pixbuf-лоадеры находились
    // рядом с exe, а не по путям сборки (D:\msys64). На дев-машине (target/debug
    // без share/) — no-op, остаётся MSYS2 из PATH.
    setup_bundle_env();

    // Логирование (спец. §2.7.1 [logging]).
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,mdwf=debug"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    tracing::info!("MDWF GUI starting (v{})", env!("CARGO_PKG_VERSION"));

    // Регистрируем gresource-бандл (иконки маркетплейсов) ДО построения окна.
    // build.rs через glib-compile-resources собирает OUT_DIR/compiled.gresource.
    gio::resources_register_include!("compiled.gresource")
        .expect("failed to register gresource bundle");

    let app = App::new()?;
    Ok(app.run())
}
