//! Общий доменный контекст для CLI (registry + catalog + config).

use std::sync::Arc;

use anyhow::{Context as _, Result};

use mdwf_config::ProvisionedConfig;
use mdwf_core::{ProviderRef, ProviderRegistry};
use mdwf_providers_ozon::OzonProvider;
use mdwf_providers_wildberries::WildberriesProvider;
use mdwf_secrets::{OsKeychain, SecretStore};
use mdwf_storage::{Catalog, FileStore, FileStoreConfig, FolderStructure};
use mdwf_test_provider::TestProvider;

/// Контекст выполнения CLI (общий доменный слой).
pub struct Context {
    pub registry: ProviderRegistry,
    pub catalog: Catalog,
    pub secrets: Arc<dyn SecretStore>,
    pub config: ProvisionedConfig,
    pub file_store: FileStore,
}

impl Context {
    /// Инициализирует контекст: провижнит конфиг, открывает каталог, регистрирует провайдеров.
    pub fn new() -> Result<Self> {
        let prov = ProvisionedConfig::load_standard().context("load config")?;
        std::fs::create_dir_all(&prov.data_dir).ok();
        let catalog = Catalog::open(&prov.db_path).context("open catalog")?;

        let secrets: Arc<dyn SecretStore> = if prov.raw.security.use_keychain {
            Arc::new(OsKeychain::new())
        } else {
            Arc::new(mdwf_secrets::InMemorySecretStore::new())
        };

        let file_store = FileStore::new(FileStoreConfig {
            output_dir: prov.output_dir.clone(),
            file_name_template: prov.raw.storage.file_name_template.clone(),
            folder_structure: match prov.raw.storage.folder_structure.as_str() {
                "flat" => FolderStructure::Flat,
                "by_provider_profile_period" => FolderStructure::ByProviderProfilePeriod,
                _ => FolderStructure::ByProviderPeriod,
            },
            compute_hash: prov.raw.storage.compute_hash,
        });

        let registry = ProviderRegistry::new();
        registry
            .register(Arc::new(TestProvider::new()) as ProviderRef)
            .ok();
        registry
            .register(Arc::new(OzonProvider::new()?) as ProviderRef)
            .ok();
        registry
            .register(Arc::new(WildberriesProvider::new()?) as ProviderRef)
            .ok();

        Ok(Self {
            registry,
            catalog,
            secrets,
            config: prov,
            file_store,
        })
    }

    /// Сохраняет профиль в каталог.
    pub fn save_profile(&self, profile: &mdwf_core::Profile) -> Result<i64> {
        Ok(self.catalog.upsert_profile(profile)?)
    }
}
