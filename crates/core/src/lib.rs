//! # mdwf-core
//!
//! Ядро фреймворка Marketplace Downloader Framework (MDWF).
//!
//! Содержит провайдер-агностичные трейты и типы (спец. §2.3, гл. 09).
//! Никаких упоминаний конкретных маркетплейсов (Ozon/Wildberries) быть не должно —
//! принцип «Framework First» (спец. §1.3 п.1, гл. 02).

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
// doc_markdown слишком шумный для русскоязычных комментариев с техническими
// терминами (Ozon, Wildberries, OAuth2, SHA-256 и т.д.). Backticks пишем вручную
// там, где это улучшает читаемость.
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]

/// Версия ядра (совпадает с версией спецификации).
pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod auth;
pub mod capabilities;
pub mod downloader;
pub mod error;
pub mod health;
pub mod pagination;
pub mod params;
pub mod profile;
pub mod progress;
pub mod provider;
pub mod registry;
pub mod report;
pub mod secret;

// ----- Реэкспорт ключевых типов -----

pub use auth::{
    days_until_expiry, ensure_not_expired, Authenticator,
    EXPIRY_DEGRADED_DAYS, EXPIRY_WARN_DAYS,
};
pub use capabilities::{
    AuthField, AuthFieldKind, AuthType, Capabilities, PeriodKind, ReportDescriptor,
};
pub use downloader::{DownloadedFile, Downloader, DownloaderKind};
pub use error::{CoreError, CoreResult};
pub use health::{HealthLevel, HealthStatus};
pub use pagination::{PagePagination, Pagination};
pub use params::{ReportParameter, ReportParameterKind, ReportParams};
pub use profile::Profile;
pub use progress::{NoopProgress, ProgressCallback, ProgressCallbackRef, ProgressUpdate};
pub use provider::{MarketplaceProvider, ProviderRef};
pub use registry::{register_all_providers, register_provider, ProviderRegistry};
pub use report::{
    AcquisitionMode, CancelToken, DocumentEntry, DocumentFilter, Report, ReportCategory,
    ReportRef,
};
pub use secret::SecretString;
