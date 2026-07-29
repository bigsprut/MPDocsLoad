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
        mdwf_core::CoreError::SecretNotFound(_) => ExitCode::AuthError,
        mdwf_core::CoreError::Network(_) => ExitCode::NetworkError,
        mdwf_core::CoreError::Cancelled => ExitCode::Cancelled,
        mdwf_core::CoreError::Internal(_) => ExitCode::ApiError,
    }
}
