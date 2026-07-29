//! # mdwf-config
//!
//! Загрузка/сохранение config.toml (спец. §2.7.1) и пути к данным приложения.
//! Используется GUI и CLI.

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]

pub mod settings;

pub use settings::{
    data_dir, expand_tilde, AppConfig, AppSection, ConfigError, LoggingSection, NetworkSection,
    ProviderConfigTable, ProvisionedConfig, SchedulerSection, SecretMode, SecuritySection,
    StorageSection, SCHEMA_VERSION,
};
