//! `OzonProvider` — реализация трейта `MarketplaceProvider` (спец. §2.9, §2.9.4).

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use tracing::debug;

use mdwf_core::{
    days_until_expiry, Authenticator, Capabilities, CoreError, CoreResult,
    EXPIRY_DEGRADED_DAYS, EXPIRY_WARN_DAYS, HealthStatus,
    MarketplaceProvider, Profile, ReportRef, SecretString,
};

use crate::auth::{OzonAuthenticator, API_KEY_TTL_DAYS};
use crate::client::{OzonHttpClient, RetryPolicy};
use crate::reports;

/// Провайдер Ozon Seller API.
pub struct OzonProvider {
    client: OzonHttpClient,
    capabilities: Capabilities,
}

impl OzonProvider {
    /// Создаёт провайдера с настройками по умолчанию (базовый URL, retry policy).
    pub fn new() -> CoreResult<Self> {
        let client = OzonHttpClient::new(None, RetryPolicy::default())?;
        Ok(Self {
            client,
            capabilities: reports::capabilities(),
        })
    }

    /// Создаёт провайдера с кастомным базовым URL (для тестов/mocking).
    pub fn with_base_url(base_url: &str) -> CoreResult<Self> {
        let client = OzonHttpClient::new(Some(base_url), RetryPolicy::default())?;
        Ok(Self {
            client,
            capabilities: reports::capabilities(),
        })
    }

    /// Создаёт провайдера с кастомным базовым URL и retry policy (для тестов:
    /// крошечные задержки, чтобы breaker-сценарии не спали по экспоненте).
    pub fn with_base_url_and_retry(base_url: &str, retry: RetryPolicy) -> CoreResult<Self> {
        let client = OzonHttpClient::new(Some(base_url), retry)?;
        Ok(Self {
            client,
            capabilities: reports::capabilities(),
        })
    }
}

impl Default for OzonProvider {
    fn default() -> Self {
        Self::new().expect("OzonProvider::new")
    }
}

#[async_trait]
impl MarketplaceProvider for OzonProvider {
    fn id(&self) -> &'static str {
        "ozon"
    }

    fn display_name(&self) -> &'static str {
        "Ozon"
    }

    fn version(&self) -> &'static str {
        "1.4.0"
    }

    fn docs_url(&self) -> &'static str {
        "https://dev.ozon.ru/"
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    async fn authenticator(&self, profile: &Profile) -> CoreResult<Arc<dyn Authenticator>> {
        let client_id_str = profile
            .metadata("client_id")
            .ok_or_else(|| CoreError::InvalidParameter("profile missing 'client_id'".into()))?;
        let client_id: i64 = client_id_str
            .parse()
            .map_err(|_| CoreError::InvalidParameter(format!("client_id not a number: {client_id_str}")))?;
        let api_key = profile
            .metadata("api_key")
            .ok_or_else(|| CoreError::InvalidParameter("profile missing 'api_key'".into()))?;
        let api_key = SecretString::new(api_key);
        Ok(Arc::new(OzonAuthenticator::new(client_id, api_key, Utc::now())))
    }

    async fn report(&self, report_type: &str) -> CoreResult<ReportRef> {
        // Полные реализации каждого отчёта будут добавлены инкрементально.
        // Пока возвращаем заглушку-отчёт, который делегирует в client.
        crate::reports::make_report(report_type, self.client.clone())
    }

    async fn reports(&self) -> CoreResult<Vec<ReportRef>> {
        let mut out = Vec::new();
        for desc in &self.capabilities.reports {
            if let Ok(r) =
                crate::reports::make_report(&desc.type_id, self.client.clone())
            {
                out.push(r);
            }
        }
        Ok(out)
    }

    async fn health_check(&self, auth: &dyn Authenticator) -> CoreResult<HealthStatus> {
        debug!("Ozon health check via /v1/finance/balance");
        // Дока: /v1/finance/balance требует date_from + date_to (макс. 30 дней).
        let today = Utc::now();
        let month_ago = today - chrono::Duration::days(29);
        let body = json!({
            "date_from": month_ago.format("%Y-%m-%d").to_string(),
            "date_to": today.format("%Y-%m-%d").to_string(),
        });
        match self.client.post("/v1/finance/balance", &body, auth).await {
            Ok(_) => {
                // Проверяем срок действия ключа.
                let days = days_until_expiry(auth.expires_at()).unwrap_or(i64::MAX);
                if days < EXPIRY_DEGRADED_DAYS {
                    return Ok(HealthStatus::down(format!(
                        "API key expires in {days} days (< {EXPIRY_DEGRADED_DAYS})"
                    )));
                }
                if days < EXPIRY_WARN_DAYS {
                    return Ok(HealthStatus::degraded(format!(
                        "API key expires in {days} days (< {EXPIRY_WARN_DAYS}; TTL {API_KEY_TTL_DAYS}d)"
                    )));
                }
                Ok(HealthStatus::ok())
            }
            Err(CoreError::Network(e)) if e.is_timeout() => {
                Ok(HealthStatus::down(format!("network timeout: {e}")))
            }
            Err(CoreError::Network(_)) => Ok(HealthStatus::down("network error")),
            Err(e) if e.is_auth_failure() => {
                Ok(HealthStatus::down(format!("auth failed: {e}")))
            }
            Err(e) if e.is_rate_limited() => {
                Ok(HealthStatus::down(format!("rate limited: {e}")))
            }
            Err(e) if e.is_transient() => {
                // 5xx — серверная ошибка, может восстановиться → Degraded.
                Ok(HealthStatus::degraded(format!("server error: {e}")))
            }
            Err(e) => Ok(HealthStatus::down(e.to_string())),
        }
    }

    /// Имя продавца из Ozon `/v1/seller/info` → `company.legal_name`.
    ///
    /// Сверено с `docs/ozon-seller-api-reference.md`: POST с пустым телом,
    /// заголовки `Client-Id` + `Api-Key` ставит аутентификатор. Ответ —
    /// `company` на верхнем уровне (без обёртки `result`, как у report-API).
    /// Берём `legal_name` (полное юридическое наименование, напр.
    /// «Общество с ограниченной ответственностью 'Ромашка'») — оно точнее
    /// идентифицирует продавца, чем краткое `name`.
    /// При ошибке сети/парсинга возвращаем `None` (заголовок покажет имя профиля).
    async fn account_display_name(
        &self,
        auth: &dyn Authenticator,
    ) -> CoreResult<Option<String>> {
        debug!("Ozon account name via /v1/seller/info");
        let resp = self.client.post("/v1/seller/info", &json!({}), auth).await?;
        Ok(resp["company"]["legal_name"].as_str().map(str::to_string))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_id_and_meta() {
        let p = OzonProvider::new().unwrap();
        assert_eq!(p.id(), "ozon");
        assert_eq!(p.display_name(), "Ozon");
        assert_eq!(p.docs_url(), "https://dev.ozon.ru/");
    }

    #[test]
    fn authenticator_requires_client_id_and_api_key() {
        let p = OzonProvider::new().unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        // Без client_id -> ошибка.
        let profile = Profile::new("x", "ozon");
        let result = rt.block_on(p.authenticator(&profile));
        assert!(matches!(
            result,
            Err(CoreError::InvalidParameter(_))
        ));

        // С валидными полями -> успех.
        let profile = Profile::new("x", "ozon")
            .with_metadata("client_id", "1234567")
            .with_metadata("api_key", "secret");
        let auth = rt.block_on(p.authenticator(&profile)).unwrap();
        assert_eq!(auth.auth_type(), mdwf_core::capabilities::AuthType::ApiKey);
    }
}
