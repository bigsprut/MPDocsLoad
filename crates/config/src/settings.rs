//! Настройки приложения (спец. §2.7.1: config.toml).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use mdwf_core::CoreError;

/// Версия схемы настроек (спец. `schema_version = 2`).
pub const SCHEMA_VERSION: u32 = 2;

/// Режим хранения секретов.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SecretMode {
    /// OS keychain (Windows Credential Manager / macOS Keychain / Secret Service).
    #[default]
    Keychain,
    /// In-memory (только для разработки/тестов; не переживает перезапуск).
    Memory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSection {
    #[serde(default = "default_ui_scale")]
    pub ui_scale: u32,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub start_minimized: bool,
    #[serde(default = "default_true")]
    pub confirm_exit_during_download: bool,
}

fn default_ui_scale() -> u32 {
    100
}
fn default_theme() -> String {
    "system".into()
}
fn default_language() -> String {
    "ru".into()
}
fn default_true() -> bool {
    true
}

impl Default for AppSection {
    fn default() -> Self {
        Self {
            ui_scale: default_ui_scale(),
            theme: default_theme(),
            language: default_language(),
            start_minimized: false,
            confirm_exit_during_download: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageSection {
    #[serde(default = "default_output_dir")]
    pub output_dir: String,
    #[serde(default = "default_file_name_template")]
    pub file_name_template: String,
    #[serde(default = "default_folder_structure")]
    pub folder_structure: String,
    #[serde(default = "default_true")]
    pub compute_hash: bool,
}

fn default_output_dir() -> String {
    "~/Documents/MDWF/downloads".into()
}
fn default_file_name_template() -> String {
    // {doc_id} включён по умолчанию: для Browsable-отчётов (документы WB)
    // это даёт уникальные осмысленные имена; для Period-отчётов сегмент
    // doc_id отсутствует и нормализацией вырезается (см. storage::naming).
    "{provider}_{profile}_{report}_{doc_id}_{period}.{ext}".into()
}
fn default_folder_structure() -> String {
    "by_provider_period".into()
}

impl Default for StorageSection {
    fn default() -> Self {
        Self {
            output_dir: default_output_dir(),
            file_name_template: default_file_name_template(),
            folder_structure: default_folder_structure(),
            compute_hash: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecuritySection {
    #[serde(default = "default_true")]
    pub use_keychain: bool,
    #[serde(default)]
    pub lock_timeout_minutes: u32,
    #[serde(default = "default_log_retention")]
    pub log_retention_days: u32,
}

fn default_log_retention() -> u32 {
    30
}

impl Default for SecuritySection {
    fn default() -> Self {
        Self {
            use_keychain: true,
            lock_timeout_minutes: 0,
            log_retention_days: default_log_retention(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSection {
    #[serde(default = "default_timeout")]
    pub request_timeout_seconds: u32,
    #[serde(default = "default_concurrency")]
    pub max_concurrency_per_provider: u32,
    #[serde(default = "default_true")]
    pub use_system_proxy: bool,
    #[serde(default = "default_retries")]
    pub max_retries: u32,
    #[serde(default = "default_retry_base")]
    pub retry_base_delay_ms: u64,
    #[serde(default = "default_retry_max")]
    pub retry_max_delay_ms: u64,
}

fn default_timeout() -> u32 {
    30
}
fn default_concurrency() -> u32 {
    3
}
fn default_retries() -> u32 {
    5
}
fn default_retry_base() -> u64 {
    500
}
fn default_retry_max() -> u64 {
    30_000
}

impl Default for NetworkSection {
    fn default() -> Self {
        Self {
            request_timeout_seconds: default_timeout(),
            max_concurrency_per_provider: default_concurrency(),
            use_system_proxy: true,
            max_retries: default_retries(),
            retry_base_delay_ms: default_retry_base(),
            retry_max_delay_ms: default_retry_max(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerSection {
    #[serde(default = "default_true")]
    pub enabled_on_start: bool,
    #[serde(default)]
    pub autostart_with_os: bool,
    #[serde(default = "default_parallel_jobs")]
    pub max_parallel_jobs: u32,
}

fn default_parallel_jobs() -> u32 {
    3
}

impl Default for SchedulerSection {
    fn default() -> Self {
        Self {
            enabled_on_start: true,
            autostart_with_os: false,
            max_parallel_jobs: default_parallel_jobs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSection {
    #[serde(default = "default_level")]
    pub level: String,
    #[serde(default = "default_log_dir")]
    pub dir: String,
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default = "default_rotation")]
    pub rotation: String,
    #[serde(default = "default_max_files")]
    pub max_files: u32,
}

fn default_level() -> String {
    "info".into()
}
fn default_log_dir() -> String {
    "~/.mdwf/logs".into()
}
fn default_format() -> String {
    "text".into()
}
fn default_rotation() -> String {
    "daily".into()
}
fn default_max_files() -> u32 {
    30
}

impl Default for LoggingSection {
    fn default() -> Self {
        Self {
            level: default_level(),
            dir: default_log_dir(),
            format: default_format(),
            rotation: default_rotation(),
            max_files: default_max_files(),
        }
    }
}

/// Конфигурация конкретного провайдера (произвольные пары ключ-значение).
pub type ProviderConfigTable = BTreeMap<String, toml::Value>;

/// Корневой конфиг (спец. §2.7.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub schema_version: u32,
    #[serde(default)]
    pub app: AppSection,
    #[serde(default)]
    pub storage: StorageSection,
    #[serde(default)]
    pub security: SecuritySection,
    #[serde(default)]
    pub network: NetworkSection,
    #[serde(default)]
    pub scheduler: SchedulerSection,
    #[serde(default)]
    pub logging: LoggingSection,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfigTable>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            app: AppSection::default(),
            storage: StorageSection::default(),
            security: SecuritySection::default(),
            network: NetworkSection::default(),
            scheduler: SchedulerSection::default(),
            logging: LoggingSection::default(),
            providers: BTreeMap::new(),
        }
    }
}

/// Конфиг с подготовленными путями (после раскрытия `~` и интерполяции).
#[derive(Debug, Clone)]
pub struct ProvisionedConfig {
    pub raw: AppConfig,
    /// Папка данных приложения (~/.mdwf или эквивалент ОС).
    pub data_dir: PathBuf,
    /// Путь к config.toml.
    pub config_path: PathBuf,
    /// Путь к SQLite-каталогу.
    pub db_path: PathBuf,
    /// Реальная папка выгрузки (после раскрытия ~).
    pub output_dir: PathBuf,
}

/// Ошибка настроек.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("TOML serialize error: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("home directory not found")]
    NoHome,
}

impl From<ConfigError> for CoreError {
    fn from(e: ConfigError) -> Self {
        CoreError::Internal(format!("config: {e}"))
    }
}

/// Возвращает директорию данных приложения (~/.mdwf или эквивалент ОС).
#[must_use]
pub fn data_dir() -> PathBuf {
    if let Some(base) = dirs::data_dir() {
        base.join("mdwf")
    } else if let Some(home) = dirs::home_dir() {
        home.join(".mdwf")
    } else {
        PathBuf::from(".mdwf")
    }
}

/// Расширяет `~/`-префикс в реальный домашний путь.
pub fn expand_tilde(path: &str) -> Result<PathBuf, ConfigError> {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = dirs::home_dir().ok_or(ConfigError::NoHome)?;
        Ok(home.join(rest))
    } else if path == "~" {
        Ok(dirs::home_dir().ok_or(ConfigError::NoHome)?)
    } else {
        Ok(PathBuf::from(path))
    }
}

impl AppConfig {
    /// Загружает конфиг из файла; если файл не существует — возвращает default.
    pub fn load_or_default(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)?;
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        let cfg: Self = toml::from_str(&text)?;
        Ok(cfg)
    }

    /// Сохраняет конфиг в файл (создаёт родительские директории).
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    /// Подготавливает пути (раскрывает `~`, определяет data_dir/db_path).
    pub fn provision(self) -> Result<ProvisionedConfig, ConfigError> {
        let data_dir = data_dir();
        let config_path = data_dir.join("config.toml");
        let db_path = data_dir.join("mdwf.db");
        let output_dir = expand_tilde(&self.storage.output_dir).unwrap_or_else(|_| PathBuf::from("downloads"));
        Ok(ProvisionedConfig {
            raw: self,
            data_dir,
            config_path,
            db_path,
            output_dir,
        })
    }
}

impl ProvisionedConfig {
    /// Загружает и провижинит конфиг по стандартному пути приложения.
    pub fn load_standard() -> Result<Self, ConfigError> {
        let data_dir = data_dir();
        let config_path = data_dir.join("config.toml");
        let cfg = AppConfig::load_or_default(&config_path)?;
        cfg.provision()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roundtrip() {
        let cfg = AppConfig::default();
        let text = toml::to_string(&cfg).unwrap();
        let parsed: AppConfig = toml::from_str(&text).unwrap();
        assert_eq!(parsed.schema_version, SCHEMA_VERSION);
        assert_eq!(parsed.app.language, "ru");
        assert_eq!(parsed.network.max_retries, 5);
    }

    #[test]
    fn load_missing_returns_default() {
        let cfg = AppConfig::load_or_default(Path::new("/nonexistent/path/config.toml")).unwrap();
        assert_eq!(cfg.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn save_and_load() {
        let dir = std::env::temp_dir().join(format!("mdwf-cfg-{}", std::process::id()));
        let path = dir.join("config.toml");
        let cfg = AppConfig::default();
        cfg.save(&path).unwrap();
        let loaded = AppConfig::load_or_default(&path).unwrap();
        assert_eq!(loaded.storage.file_name_template, cfg.storage.file_name_template);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
