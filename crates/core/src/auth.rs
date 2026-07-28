//! Авторизация — трейт `Authenticator` (спец. §2.3.2).
//!
//! Абстракция над способами авторизации маркетплейсов.
//! `apply` принимает `reqwest::RequestBuilder` напрямую (как в спец. §2.3.2).

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::capabilities::AuthType;
use crate::error::{CoreError, CoreResult};

/// Трейт авторизации (спец. §2.3.2).
#[async_trait]
pub trait Authenticator: Send + Sync {
    /// Применяет учётные данные к HTTP-запросу.
    fn apply(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder;

    /// Момент истечения действия секрета (если применимо).
    fn expires_at(&self) -> Option<DateTime<Utc>>;

    /// Обновляет секрет (для OAuth2 и т.п.). Возвращает `true`, если обновлено.
    async fn refresh(&self) -> CoreResult<bool> {
        Ok(false)
    }

    /// Тип авторизации.
    fn auth_type(&self) -> AuthType;

    /// Человекочитаемое описание (для логов/UI), без секрета.
    fn describe(&self) -> String;
}

/// Утилита: вычислить дни до истечения секрета.
#[must_use]
pub fn days_until_expiry(expires_at: Option<DateTime<Utc>>) -> Option<i64> {
    expires_at.map(|t| (t - Utc::now()).num_days())
}

/// Пороги предупреждений об истечении (спец. §2.9.1: 14 дней, §2.9.4: 3 дня degraded).
pub const EXPIRY_WARN_DAYS: i64 = 14;
pub const EXPIRY_DEGRADED_DAYS: i64 = 3;

/// Проверяет, что срок действия не истёк, иначе возвращает ошибку.
pub fn ensure_not_expired(expires_at: Option<DateTime<Utc>>) -> CoreResult<()> {
    if let Some(t) = expires_at {
        if t <= Utc::now() {
            return Err(CoreError::InvalidParameter(format!(
                "secret expired at {t}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn days_until_future() {
        let future = Utc::now() + Duration::days(10);
        // num_days() округляет вниз, поэтому для 10-дневного буфера
        // ожидаем значение в диапазоне [9, 10].
        let days = days_until_expiry(Some(future)).unwrap_or(-1);
        assert!((9..=10).contains(&days), "expected 9..=10, got {days}");
    }

    #[test]
    fn expired_returns_error() {
        let past = Utc::now() - Duration::days(1);
        assert!(ensure_not_expired(Some(past)).is_err());
    }

    #[test]
    fn not_expired_ok() {
        let future = Utc::now() + Duration::days(5);
        assert!(ensure_not_expired(Some(future)).is_ok());
        assert!(ensure_not_expired(None).is_ok());
    }
}
