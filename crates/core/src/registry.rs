//! Реестр провайдеров (спец. §2.3.3).
//!
//! Потокобезопасный словарь `id -> Arc<dyn MarketplaceProvider>`.

use std::sync::Arc;

use parking_lot::RwLock;
use tracing::warn;

use crate::error::{CoreError, CoreResult};
use crate::provider::{MarketplaceProvider, ProviderRef};

/// Реестр провайдеров. Доступен из GUI/CLI/scheduler.
#[derive(Default)]
pub struct ProviderRegistry {
    providers: RwLock<std::collections::HashMap<String, ProviderRef>>,
}

impl ProviderRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Регистрирует провайдера. Если `id` уже занят — перезаписывает с предупреждением.
    pub fn register(&self, provider: ProviderRef) -> CoreResult<()> {
        let id = provider.id().to_string();
        let mut map = self.providers.write();
        if map.contains_key(&id) {
            warn!(provider = %id, "provider already registered, overwriting");
        }
        map.insert(id, provider);
        Ok(())
    }

    /// Возвращает провайдера по id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<ProviderRef> {
        self.providers.read().get(id).cloned()
    }

    /// Возвращает провайдера по id или ошибку `ProviderNotFound`.
    pub fn require(&self, id: &str) -> CoreResult<ProviderRef> {
        self.get(id)
            .ok_or_else(|| CoreError::ProviderNotFound(id.to_string()))
    }

    /// Список всех зарегистрированных провайдеров.
    #[must_use]
    pub fn list(&self) -> Vec<ProviderRef> {
        let map = self.providers.read();
        let mut v: Vec<_> = map.values().cloned().collect();
        v.sort_by(|a, b| a.id().cmp(b.id()));
        v
    }

    /// Количество зарегистрированных провайдеров.
    #[must_use]
    pub fn len(&self) -> usize {
        self.providers.read().len()
    }

    /// Пуст ли реестр.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.providers.read().is_empty()
    }
}

/// Регистрирует все встроенные провайдеры (спец. §2.3.3).
///
/// Реальные провайдеры регистрируются здесь через feature-флаги.
/// Mock-провайдер (`TestProvider`) подключается в тестах.
pub async fn register_all_providers(_registry: &ProviderRegistry) -> CoreResult<()> {
    // OzonProvider и WildberriesProvider регистрируются из крейтов-провайдеров
    // на ЭТАПАХ 8/9, чтобы сохранить принцип "Framework First" (ядро не зависит
    // от конкретных маркетплейсов). Здесь — только каркас.
    //
    // Пример вызова (в mdwf-cli / mdwf-gui):
    //   registry.register(Arc::new(OzonProvider::new()?))?;
    //   registry.register(Arc::new(WildberriesProvider::new()?))?;
    Ok(())
}

/// Вспомогательная обёртка для регистрации через arc-аргумент.
pub fn register_provider(registry: &ProviderRegistry, provider: Arc<dyn MarketplaceProvider>) -> CoreResult<()> {
    registry.register(provider)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use crate::capabilities::Capabilities;

    struct FakeProvider {
        id: &'static str,
    }

    #[async_trait]
    impl MarketplaceProvider for FakeProvider {
        fn id(&self) -> &'static str { self.id }
        fn display_name(&self) -> &'static str { "Fake" }
        fn version(&self) -> &'static str { "0.0" }
        fn docs_url(&self) -> &'static str { "" }
        fn capabilities(&self) -> &Capabilities { unimplemented!() }
        async fn authenticator(&self, _: &crate::profile::Profile) -> CoreResult<Arc<dyn crate::auth::Authenticator>> { unimplemented!() }
        async fn report(&self, _: &str) -> CoreResult<crate::report::ReportRef> { unimplemented!() }
        async fn reports(&self) -> CoreResult<Vec<crate::report::ReportRef>> { Ok(vec![]) }
        async fn health_check(&self, _: &dyn crate::auth::Authenticator) -> CoreResult<crate::health::HealthStatus> { Ok(crate::health::HealthStatus::ok()) }
    }

    #[test]
    fn registry_register_and_get() {
        let reg = ProviderRegistry::new();
        assert!(reg.is_empty());
        reg.register(Arc::new(FakeProvider { id: "fake" })).unwrap();
        assert_eq!(reg.len(), 1);
        assert!(reg.get("fake").is_some());
        assert!(reg.get("missing").is_none());
        assert!(matches!(reg.require("missing"), Err(CoreError::ProviderNotFound(_))));
    }
}
