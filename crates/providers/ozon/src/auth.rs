//! Авторизация Ozon: заголовки `Client-Id` + `Api-Key` (спец. §2.9.1).
//!
//! TTL API-ключа: 180 дней (спец. §2.9.1, news/649).

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};

use mdwf_core::{
    auth::Authenticator, capabilities::AuthType, error::CoreResult, SecretString,
};

/// Базовый URL Ozon Seller API (спец. §2.7.1 [providers.ozon]).
pub const DEFAULT_BASE_URL: &str = "https://api-seller.ozon.ru";

/// TTL API-ключа Ozon в днях (спец. §2.9.1).
pub const API_KEY_TTL_DAYS: i64 = 180;

/// Авторизатор Ozon: добавляет заголовки `Client-Id` и `Api-Key`.
pub struct OzonAuthenticator {
    /// Числовой Client-Id продавца.
    client_id: i64,
    /// API-ключ (секрет).
    api_key: SecretString,
    /// Момент создания ключа (для расчёта истечения).
    key_created_at: DateTime<Utc>,
}

impl OzonAuthenticator {
    /// Создаёт авторизатор.
    /// `key_created_at` обычно = `Utc::now()` при первичном вводе ключа.
    #[must_use]
    pub fn new(client_id: i64, api_key: SecretString, key_created_at: DateTime<Utc>) -> Self {
        Self {
            client_id,
            api_key,
            key_created_at,
        }
    }

    /// Client-Id продавца.
    #[must_use]
    pub fn client_id(&self) -> i64 {
        self.client_id
    }
}

#[async_trait]
impl Authenticator for OzonAuthenticator {
    fn apply(
        &self,
        req: reqwest::RequestBuilder,
    ) -> reqwest::RequestBuilder {
        req.header("Client-Id", self.client_id.to_string())
            .header("Api-Key", self.api_key.expose_secret())
            .header("Content-Type", "application/json")
    }

    fn expires_at(&self) -> Option<DateTime<Utc>> {
        Some(self.key_created_at + Duration::days(API_KEY_TTL_DAYS))
    }

    async fn refresh(&self) -> CoreResult<bool> {
        // У Ozon нет refresh для API-ключа; пользователь обновляет вручную.
        Ok(false)
    }

    fn auth_type(&self) -> AuthType {
        AuthType::ApiKey
    }

    fn describe(&self) -> String {
        format!("Ozon (Client-Id={}, Api-Key=***)", self.client_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn expiry_is_180_days() {
        let created = Utc::now();
        let auth = OzonAuthenticator::new(123, SecretString::new("k"), created);
        let expires = auth.expires_at().unwrap();
        let diff = expires - created;
        assert_eq!(diff.num_days(), 180);
    }

    #[test]
    fn describe_masks_key() {
        let auth = OzonAuthenticator::new(42, SecretString::new("secret-key"), Utc::now());
        let desc = auth.describe();
        assert!(desc.contains("Client-Id=42"));
        assert!(!desc.contains("secret-key"));
        assert!(desc.contains("***"));
    }

    #[test]
    fn auth_type_is_api_key() {
        let auth = OzonAuthenticator::new(1, SecretString::new("k"), Utc::now());
        assert_eq!(auth.auth_type(), AuthType::ApiKey);
    }
}
