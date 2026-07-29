//! Ошибки storage (ADR-006: thiserror для core; storage использует thiserror-надстройку).

use thiserror::Error;

use mdwf_core::CoreError;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid file name template")]
    InvalidTemplateName,

    #[error("output directory not configured")]
    NoOutputDir,

    #[error("storage internal error: {0}")]
    Internal(String),
}

impl From<StorageError> for CoreError {
    fn from(e: StorageError) -> Self {
        match e {
            StorageError::Io(io) => CoreError::Internal(format!("io: {io}")),
            other => CoreError::Internal(other.to_string()),
        }
    }
}

pub type StorageResult<T> = Result<T, StorageError>;
