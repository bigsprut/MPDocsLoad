//! Реализация `SecretStore` через крейт `keyring` (спец. §2.4: `os_keychain.rs`).
//!
//! На Windows использует Windows Credential Manager, на macOS — Keychain,
//! на Linux — Secret Service (libsecret).

use async_trait::async_trait;
use keyring::Entry;
use tracing::warn;

use mdwf_core::{CoreError, CoreResult};

use crate::keychain::{KEYCHAIN_SERVICE, SecretStore};

/// OS keychain wrapper.
#[derive(Default, Clone, Copy)]
pub struct OsKeychain;

impl OsKeychain {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn entry(account: &str) -> CoreResult<Entry> {
        Entry::new(KEYCHAIN_SERVICE, account)
            .map_err(|e| CoreError::Internal(format!("keyring entry: {e}")))
    }
}

#[async_trait]
impl SecretStore for OsKeychain {
    // Каждая операция keyring — синхронный syscall ОС (Credential Manager).
    // Оборачиваем в spawn_blocking, чтобы не блокировать tokio-worker
    // (иначе при медленном/keychain-дауне вставал весь рантайм).
    async fn set(&self, account: &str, secret: &str) -> CoreResult<()> {
        let account = account.to_string();
        let secret = secret.to_string();
        tokio::task::spawn_blocking(move || {
            let entry = Self::entry(&account)?;
            entry
                .set_password(&secret)
                .map_err(|e| CoreError::Internal(format!("keyring set: {e}")))
        })
        .await
        .map_err(|e| CoreError::Internal(format!("keyring join: {e}")))?
    }

    async fn get(&self, account: &str) -> CoreResult<Option<String>> {
        let account = account.to_string();
        tokio::task::spawn_blocking(move || {
            let entry = Self::entry(&account)?;
            match entry.get_password() {
                Ok(s) => Ok(Some(s)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => {
                    warn!(error = %e, account, "keyring get failed");
                    Err(CoreError::Internal(format!("keyring get: {e}")))
                }
            }
        })
        .await
        .map_err(|e| CoreError::Internal(format!("keyring join: {e}")))?
    }

    async fn delete(&self, account: &str) -> CoreResult<()> {
        let account = account.to_string();
        tokio::task::spawn_blocking(move || {
            let entry = Self::entry(&account)?;
            match entry.delete_password() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(CoreError::Internal(format!("keyring delete: {e}"))),
            }
        })
        .await
        .map_err(|e| CoreError::Internal(format!("keyring join: {e}")))?
    }
}
