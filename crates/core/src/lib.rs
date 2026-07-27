//! # mdwf-core
//!
//! Ядро фреймворка Marketplace Downloader Framework (MDWF).
//!
//! Содержит провайдер-агностичные трейты и типы (спец. §2.3, гл. 09).
//! Никаких упоминаний конкретных маркетплейсов (Ozon/Wildberries) быть не должно —
//! принцип «Framework First» (спец. §1.3 п.1, гл. 02).
//!
//! Полные трейты и реализации модулей появятся на ЭТАПЕ 2.

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

/// Версия ядра (совпадает с версией спецификации).
pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod capabilities;
pub mod error;
pub mod pagination;
pub mod params;
pub mod secret;

pub use capabilities::{AuthField, AuthFieldKind, Capabilities, ReportDescriptor};
pub use error::{CoreError, CoreResult};
pub use pagination::{PagePagination, Pagination};
pub use params::{ReportParams, ReportParameter, ReportParameterKind};
pub use secret::SecretString;

// Трейты (Provider/Auth/Report/Downloader) подключаются на ЭТАПЕ 2.
