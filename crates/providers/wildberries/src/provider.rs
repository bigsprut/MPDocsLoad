//! `WildberriesProvider` — реализация `MarketplaceProvider` (спец. §2.10).

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use tracing::debug;

use mdwf_core::{
    days_until_expiry, Authenticator, Capabilities, CoreError, CoreResult,
    EXPIRY_DEGRADED_DAYS, EXPIRY_WARN_DAYS, HealthStatus, MarketplaceProvider, Profile, ReportRef,
    SecretString,
};

use crate::auth::{WbAuthenticator, WbTokenType, TOKEN_TTL_DAYS};
use crate::client::{RetryPolicy, WbDomain, WbHttpClient};
use crate::reports;

/// Провайдер Wildberries OpenAPI.
pub struct WildberriesProvider {
    client: WbHttpClient,
    capabilities: Capabilities,
}

impl WildberriesProvider {
    /// Создаёт провайдера с настройками по умолчанию.
    pub fn new() -> CoreResult<Self> {
        let client = WbHttpClient::new(RetryPolicy::default())?;
        Ok(Self {
            client,
            capabilities: reports::capabilities(),
        })
    }

    /// Создаёт провайдера с переопределённым HTTP-клиентом (для тестов/mocking).
    /// Поскольку WbHttpClient жёстко привязан к доменам, для тестов используется
    /// перехват через мокирование DNS/прокси — см. integration tests.
    pub fn with_client(client: WbHttpClient) -> Self {
        Self {
            client,
            capabilities: reports::capabilities(),
        }
    }
}

impl Default for WildberriesProvider {
    fn default() -> Self {
        Self::new().expect("WildberriesProvider::new")
    }
}

#[async_trait]
impl MarketplaceProvider for WildberriesProvider {
    fn id(&self) -> &'static str {
        "wildberries"
    }

    fn display_name(&self) -> &'static str {
        "Wildberries"
    }

    fn version(&self) -> &'static str {
        "1.4.0"
    }

    fn docs_url(&self) -> &'static str {
        "https://dev.wildberries.ru/"
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    async fn authenticator(&self, profile: &Profile) -> CoreResult<Arc<dyn Authenticator>> {
        let token_str = profile
            .metadata("token")
            .ok_or_else(|| CoreError::InvalidParameter("profile missing 'token'".into()))?;
        // Тип токена из metadata (по умолчанию Personal).
        let token_type = match profile.metadata("token_type") {
            Some("Service") => WbTokenType::Service,
            Some("Base") => WbTokenType::Base,
            Some("Test") => WbTokenType::Test,
            _ => WbTokenType::Personal,
        };
        let token = SecretString::new(token_str);
        Ok(Arc::new(WbAuthenticator::new(token, token_type, Utc::now())))
    }

    async fn report(&self, report_type: &str) -> CoreResult<ReportRef> {
        reports::make_report(report_type, self.client.clone())
    }

    async fn reports(&self) -> CoreResult<Vec<ReportRef>> {
        let mut out = Vec::new();
        for desc in &self.capabilities.reports {
            if let Ok(r) = reports::make_report(&desc.type_id, self.client.clone()) {
                out.push(r);
            }
        }
        Ok(out)
    }

    async fn health_check(&self, auth: &dyn Authenticator) -> CoreResult<HealthStatus> {
        debug!("WB health check via /api/v1/account/balance");
        match self
            .client
            .get(WbDomain::Marketplace, "/api/v1/account/balance", &[], auth)
            .await
        {
            Ok(_) => {
                let days = days_until_expiry(auth.expires_at()).unwrap_or(i64::MAX);
                if days < EXPIRY_DEGRADED_DAYS {
                    return Ok(HealthStatus::down(format!(
                        "token expires in {days} days (< {EXPIRY_DEGRADED_DAYS})"
                    )));
                }
                if days < EXPIRY_WARN_DAYS {
                    return Ok(HealthStatus::degraded(format!(
                        "token expires in {days} days (< {EXPIRY_WARN_DAYS}; TTL {TOKEN_TTL_DAYS}d)"
                    )));
                }
                Ok(HealthStatus::ok())
            }
            Err(CoreError::Network(e)) if e.is_timeout() => {
                Ok(HealthStatus::down(format!("network timeout: {e}")))
            }
            Err(CoreError::Network(_)) => Ok(HealthStatus::down("network error")),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("401") || msg.contains("403") {
                    Ok(HealthStatus::down(format!("auth failed: {msg}")))
                } else {
                    Ok(HealthStatus::down(msg))
                }
            }
        }
    }
}

// Подавление неиспользуемого импорта json (оставлен для будущих health-check расширений).
#[allow(dead_code)]
fn _unused() {
    let _ = json!({});
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_meta() {
        let p = WildberriesProvider::new().unwrap();
        assert_eq!(p.id(), "wildberries");
        assert_eq!(p.display_name(), "Wildberries");
        assert_eq!(p.docs_url(), "https://dev.wildberries.ru/");
    }

    #[tokio::test]
    async fn authenticator_requires_token() {
        let p = WildberriesProvider::new().unwrap();
        let profile = Profile::new("x", "wildberries");
        let result = p.authenticator(&profile).await;
        assert!(matches!(result, Err(CoreError::InvalidParameter(_))));

        let profile = Profile::new("x", "wildberries").with_metadata("token", "wb-token");
        let auth = p.authenticator(&profile).await.unwrap();
        assert_eq!(auth.auth_type(), mdwf_core::capabilities::AuthType::BearerToken);
    }

    #[tokio::test]
    async fn authenticator_token_type_from_metadata() {
        let p = WildberriesProvider::new().unwrap();
        let profile = Profile::new("x", "wildberries")
            .with_metadata("token", "t")
            .with_metadata("token_type", "Base");
        let auth = p.authenticator(&profile).await.unwrap();
        assert!(auth.describe().contains("Base"));
    }
}
