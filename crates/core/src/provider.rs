//! Корневой трейт провайдера (спец. §2.3.2).

use std::sync::Arc;

use async_trait::async_trait;

use crate::auth::Authenticator;
use crate::capabilities::Capabilities;
use crate::error::CoreResult;
use crate::health::HealthStatus;
use crate::profile::Profile;
use crate::report::ReportRef;

/// Корневой трейт провайдера маркетплейса (спец. §2.3.2).
///
/// Реализации (`OzonProvider`, `WildberriesProvider`, ...) находятся
/// в `crates/providers/<name>/` и НЕ должны упоминаться в ядре.
#[async_trait]
pub trait MarketplaceProvider: Send + Sync + 'static {
    /// Стабильный идентификатор провайдера ("ozon", "wildberries").
    fn id(&self) -> &'static str;

    /// Человекочитаемое имя.
    fn display_name(&self) -> &'static str;

    /// Версия реализации провайдера.
    fn version(&self) -> &'static str;

    /// URL официальной документации API.
    fn docs_url(&self) -> &'static str;

    /// Самоописание: тип авторизации, поля формы, список отчётов.
    fn capabilities(&self) -> &Capabilities;

    /// Создаёт аутентификатор из данных профиля.
    async fn authenticator(&self, profile: &Profile) -> CoreResult<Arc<dyn Authenticator>>;

    /// Возвращает отчёт по идентификатору типа.
    async fn report(&self, report_type: &str) -> CoreResult<ReportRef>;

    /// Список всех поддерживаемых отчётов (arc-ссылки).
    async fn reports(&self) -> CoreResult<Vec<ReportRef>>;

    /// Проверка работоспособности API (auth + network + истечение ключа).
    async fn health_check(&self, auth: &dyn Authenticator) -> CoreResult<HealthStatus>;
}

/// Тип-псевдоним для arc-ссылки на провайдера.
pub type ProviderRef = Arc<dyn MarketplaceProvider>;
