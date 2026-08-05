//! # mdwf-gui
//!
//! GUI на GTK4 + libadwaita (спец. §2.5, гл. 04, ADR-002).
//!
//! Принцип «никакой бизнес-логики в UI» (спец. §2.5.2): окна только отображают
//! состояние и отправляют команды в доменный слой. Асинхронные задачи tokio
//! communiцируют с GTK через `glib::MainContext`.

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

fn main() -> Result<ExitCode> {
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
