//! Иерархия ошибок ядра (спец. §2.11.1).
//!
//! `CoreError` использует `thiserror` (ADR-006: thiserror для core, anyhow для app).

use thiserror::Error;

/// Ошибка ядра MDWF.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("provider not registered: {0}")]
    ProviderNotFound(String),

    #[error("profile not found: {0}")]
    ProfileNotFound(String),

    #[error("report type not supported: {0}")]
    ReportTypeNotSupported(String),

    #[error("invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("secret not found in keychain: {0}")]
    SecretNotFound(String),

    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    /// Протокольная ошибка: API ответил не так, как ожидает спецификация
    /// (2xx с некорректным JSON, отсутствие ожидаемого поля result/code и т.п.).
    /// Отличается от `Internal` (= баг в коде) — это рассинхрон с докой API.
    #[error("протокольная ошибка API: {0}")]
    Protocol(String),

    /// Временная недоступность из-за защиты самого клиента: circuit breaker
    /// разомкнут после серии отказов подряд. Повтор после cooldown
    /// (≈5 минут) может помочь — это transient, НЕ баг кода (в отличие от
    /// `Internal`, которым breaker ошибочно представляли прежде).
    #[error("временно недоступно: {0}")]
    Unavailable(String),

    #[error("operation cancelled")]
    Cancelled,

    /// Ошибка API маркетплейса с типизированным HTTP-статусом и
    /// человекочитаемым сообщением (распарсенным из тела ответа, не сырой JSON).
    /// `status` — код ответа (400/401/403/404/409/422/429/5xx);
    /// `message` — понятное описание (из поля message/errorText API);
    /// `retryable` — true для 429/5xx (transient), false для 4xx (клиентская).
    #[error("{message}")]
    Api {
        status: u16,
        message: String,
        retryable: bool,
    },

    #[error("internal error: {0}")]
    Internal(String),
}

impl CoreError {
    /// True для ошибок аутентификации (401 Unauthorized, 403 Forbidden) —
    /// ключ/токен невалиден или нет прав. Требует перевыпуска ключа.
    #[must_use]
    pub fn is_auth_failure(&self) -> bool {
        matches!(self, CoreError::Api { status: 401 | 403, .. } | CoreError::SecretNotFound(_))
    }

    /// True для rate-limit (429) — превышен лимит запросов, retry позже.
    #[must_use]
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, CoreError::Api { status: 429, .. })
    }

    /// True для transient-ошибок (429, 5xx, недоступность из-за breaker) —
    /// повтор запроса может помочь. Используется health_check для
    /// классификации как Degraded (не Down).
    #[must_use]
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            CoreError::Api { status: 429 | 500..=599, .. }
                | CoreError::Network(_)
                | CoreError::Unavailable(_)
        )
    }
}

/// Стандартный результат ядра.
pub type CoreResult<T> = Result<T, CoreError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_messages() {
        assert_eq!(
            CoreError::ProviderNotFound("x".into()).to_string(),
            "provider not registered: x"
        );
        assert_eq!(
            CoreError::ProfileNotFound("y".into()).to_string(),
            "profile not found: y"
        );
        assert_eq!(
            CoreError::ReportTypeNotSupported("z".into()).to_string(),
            "report type not supported: z"
        );
        assert_eq!(
            CoreError::InvalidParameter("bad".into()).to_string(),
            "invalid parameter: bad"
        );
        assert_eq!(
            CoreError::SecretNotFound("k".into()).to_string(),
            "secret not found in keychain: k"
        );
        assert_eq!(CoreError::Cancelled.to_string(), "operation cancelled");
        assert!(CoreError::Internal("boom".into())
            .to_string()
            .contains("boom"));
    }

    #[test]
    fn unavailable_is_transient() {
        // Breaker-open — временная недоступность: retry после cooldown поможет.
        let e = CoreError::Unavailable("circuit breaker open".into());
        assert!(e.is_transient());
        assert!(!e.is_auth_failure());
        assert!(!e.is_rate_limited());
        // Internal (баг кода) — НЕ transient.
        assert!(!CoreError::Internal("x".into()).is_transient());
    }

    #[test]
    fn api_error_display_is_clean_message() {
        // Display = чистое message, без «internal error: API 401: {json}».
        let e = CoreError::Api {
            status: 401,
            message: "Api-key is invalid or expired".into(),
            retryable: false,
        };
        assert_eq!(e.to_string(), "Api-key is invalid or expired");
    }

    #[test]
    fn helper_methods_classify_api_errors() {
        let auth = CoreError::Api { status: 401, message: "x".into(), retryable: false };
        let auth_forbidden = CoreError::Api { status: 403, message: "x".into(), retryable: false };
        let rate = CoreError::Api { status: 429, message: "x".into(), retryable: true };
        let server = CoreError::Api { status: 503, message: "x".into(), retryable: true };
        let client = CoreError::Api { status: 400, message: "x".into(), retryable: false };
        let not_found = CoreError::Api { status: 404, message: "x".into(), retryable: false };

        assert!(auth.is_auth_failure());
        assert!(auth_forbidden.is_auth_failure());
        assert!(!rate.is_auth_failure());
        assert!(rate.is_rate_limited());
        assert!(!auth.is_rate_limited());
        assert!(rate.is_transient());
        assert!(server.is_transient());
        assert!(!client.is_transient());
        assert!(!not_found.is_transient());
    }

    #[test]
    fn secret_not_found_is_auth_failure() {
        let e = CoreError::SecretNotFound("k".into());
        assert!(e.is_auth_failure());
    }
}
