//! Авторизация Wildberries: заголовок `Authorization` **БЕЗ** префикса `Bearer`
//! (спец. §2.10.1, news/148 — 4 типа токенов).

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};

use mdwf_core::{
    capabilities::AuthType, error::CoreResult, Authenticator, SecretString,
};

/// TTL токена WB в днях (спец. §2.10.1).
pub const TOKEN_TTL_DAYS: i64 = 180;

/// Тип токена WB (спец. §2.10.1, news/148).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WbTokenType {
    /// Основной тип для продавцов.
    Personal,
    /// Облачные сервисы из каталога WB.
    Service,
    /// Ограниченный доступ, низкие rate limits.
    Base,
    /// Sandbox.
    Test,
}

impl WbTokenType {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Personal => "Personal",
            Self::Service => "Service",
            Self::Base => "Base",
            Self::Test => "Test",
        }
    }
}

/// Авторизатор Wildberries.
///
/// **КРИТИЧЕСКИ**: заголовок `Authorization` содержит голый токен БЕЗ префикса
/// `Bearer ` (спец. §2.10.1). Добавление `Bearer ` приводит к 401.
pub struct WbAuthenticator {
    token: SecretString,
    token_type: WbTokenType,
    created_at: DateTime<Utc>,
}

impl WbAuthenticator {
    #[must_use]
    pub fn new(token: SecretString, token_type: WbTokenType, created_at: DateTime<Utc>) -> Self {
        Self {
            token,
            token_type,
            created_at,
        }
    }

    /// Тип токена.
    #[must_use]
    pub fn token_type(&self) -> WbTokenType {
        self.token_type
    }
}

#[async_trait]
impl Authenticator for WbAuthenticator {
    fn apply(
        &self,
        req: reqwest::RequestBuilder,
    ) -> reqwest::RequestBuilder {
        // КРИТИЧЕСКИ: БЕЗ префикса "Bearer "!
        req.header("Authorization", self.token.expose_secret())
    }

    fn expires_at(&self) -> Option<DateTime<Utc>> {
        Some(self.created_at + Duration::days(TOKEN_TTL_DAYS))
    }

    async fn refresh(&self) -> CoreResult<bool> {
        // У WB нет refresh; пользователь обновляет токен вручную.
        Ok(false)
    }

    fn auth_type(&self) -> AuthType {
        AuthType::BearerToken
    }

    fn describe(&self) -> String {
        format!("Wildberries ({} token: ***)", self.token_type.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_masks_token_and_shows_type() {
        let auth = WbAuthenticator::new(
            SecretString::new("wb-secret-token"),
            WbTokenType::Personal,
            Utc::now(),
        );
        let desc = auth.describe();
        assert!(desc.contains("Personal"));
        assert!(desc.contains("***"));
        assert!(!desc.contains("wb-secret-token"));
    }

    #[test]
    fn expiry_is_180_days() {
        let created = Utc::now();
        let auth = WbAuthenticator::new(SecretString::new("t"), WbTokenType::Personal, created);
        let diff = auth.expires_at().unwrap() - created;
        assert_eq!(diff.num_days(), 180);
    }

    #[test]
    fn auth_type_is_bearer() {
        let auth = WbAuthenticator::new(SecretString::new("t"), WbTokenType::Base, Utc::now());
        assert_eq!(auth.auth_type(), AuthType::BearerToken);
    }
}
