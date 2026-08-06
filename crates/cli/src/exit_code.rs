//! Exit-коды CLI (спец. §2.6.2).
//!
//! Совпадают с кодами из спецификации. Используются как результат `main`.

#![allow(dead_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitCode {
    Success = 0,
    GenericError = 1,
    UsageError = 2,
    ConfigError = 3,
    AuthError = 4,
    NetworkError = 5,
    RateLimit = 6,
    ApiError = 7,
    StorageError = 8,
    NotFound = 9,
    DeprecatedMethod = 11,
    PartialSuccess = 12,
    Cancelled = 13,
    OutOfScope = 64,
}

impl ExitCode {
    #[must_use]
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

impl From<ExitCode> for std::process::ExitCode {
    fn from(code: ExitCode) -> Self {
        std::process::ExitCode::from(code.as_u8())
    }
}

/// Преобразует `mdwf_core::CoreError` в подходящий exit-код.
#[must_use]
pub fn from_core_error(e: &mdwf_core::CoreError) -> ExitCode {
    match e {
        mdwf_core::CoreError::ProviderNotFound(_) | mdwf_core::CoreError::ProfileNotFound(_) => {
            ExitCode::NotFound
        }
        mdwf_core::CoreError::ReportTypeNotSupported(_) => ExitCode::UsageError,
        mdwf_core::CoreError::InvalidParameter(_) => ExitCode::UsageError,
        // Отсутствие секрета в keychain — auth-проблема.
        mdwf_core::CoreError::SecretNotFound(_) => ExitCode::AuthError,
        mdwf_core::CoreError::Network(_) => ExitCode::NetworkError,
        mdwf_core::CoreError::Cancelled => ExitCode::Cancelled,
        // API-ошибки: классифицируем по HTTP-статусу.
        mdwf_core::CoreError::Api { status, .. } => match *status {
            401 | 403 => ExitCode::AuthError,
            404 => ExitCode::NotFound,
            429 => ExitCode::RateLimit,
            400 | 409 | 422 => ExitCode::UsageError,
            _ => ExitCode::ApiError, // 5xx и прочее
        },
        mdwf_core::CoreError::Internal(_) => ExitCode::ApiError,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdwf_core::CoreError;

    fn api(status: u16) -> CoreError {
        CoreError::Api {
            status,
            message: "x".into(),
            retryable: false,
        }
    }

    #[test]
    fn auth_errors_map_to_auth_exit() {
        // 401/403 → AuthError (раньше WB-401 уходил в ApiError).
        assert_eq!(from_core_error(&api(401)), ExitCode::AuthError);
        assert_eq!(from_core_error(&api(403)), ExitCode::AuthError);
        // SecretNotFound → AuthError.
        assert_eq!(
            from_core_error(&CoreError::SecretNotFound("k".into())),
            ExitCode::AuthError
        );
    }

    #[test]
    fn rate_limit_maps_to_rate_limit_exit() {
        assert_eq!(from_core_error(&api(429)), ExitCode::RateLimit);
    }

    #[test]
    fn client_errors_map_correctly() {
        assert_eq!(from_core_error(&api(400)), ExitCode::UsageError);
        assert_eq!(from_core_error(&api(404)), ExitCode::NotFound);
        assert_eq!(from_core_error(&api(409)), ExitCode::UsageError);
        assert_eq!(from_core_error(&api(422)), ExitCode::UsageError);
    }

    #[test]
    fn server_errors_map_to_api_error() {
        assert_eq!(from_core_error(&api(500)), ExitCode::ApiError);
        assert_eq!(from_core_error(&api(503)), ExitCode::ApiError);
    }
}
