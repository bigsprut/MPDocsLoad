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

    #[error("operation cancelled")]
    Cancelled,

    #[error("internal error: {0}")]
    Internal(String),
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
}
