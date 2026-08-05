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

        // Переход на keyring-only: сбрасываем старые профили (с секретами в БД).
        // Пользователь создаст их заново, секреты уйдут в keyring. Только при
        // use_keychain (dev-режим InMemory не трогаем).
        if prov.raw.security.use_keychain {
            if let Err(e) = catalog.clear_profiles() {
                eprintln!("предупреждение: не удалось сбросить профили: {e}");
            }
        }

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

    /// Читает профиль по имени и подмешивает секреты из keyring (для передачи
    /// в `provider.authenticator`). Секреты хранятся только в keyring, в БД их
    /// нет — поэтому перед вызовом провайдера их нужно достать и вставить в
    /// auth_metadata in-memory.
    pub async fn profile_with_secrets(&self, name: &str) -> Result<mdwf_core::Profile> {
        let profile = self
            .catalog
            .get_profile_by_name(name)?
            .ok_or_else(|| anyhow::anyhow!("профиль '{name}' не найден"))?;
        let provider = self.registry.require(&profile.provider_id)?;
        let caps = provider.capabilities();
        let secret_fields = mdwf_secrets::secret_field_ids(caps);
        Ok(
            mdwf_secrets::load_profile_secrets(profile, &secret_fields, self.secrets.as_ref())
                .await?,
        )
    }
}
