//! In-memory mock `SecretStore` для тестов (спец. §2.4: `memory.rs`).

use std::collections::HashMap;

use async_trait::async_trait;
use parking_lot::Mutex;

use mdwf_core::CoreResult;

use crate::keychain::SecretStore;

/// In-memory реализация. Потокобезопасная через Mutex.
#[derive(Default)]
pub struct InMemorySecretStore {
    secrets: Mutex<HashMap<String, String>>,
}

impl InMemorySecretStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SecretStore for InMemorySecretStore {
    async fn set(&self, account: &str, secret: &str) -> CoreResult<()> {
        self.secrets
            .lock()
            .insert(account.to_string(), secret.to_string());
        Ok(())
    }

    async fn get(&self, account: &str) -> CoreResult<Option<String>> {
        Ok(self.secrets.lock().get(account).cloned())
    }

    async fn delete(&self, account: &str) -> CoreResult<()> {
        self.secrets.lock().remove(account);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn set_get_delete() {
        let store = InMemorySecretStore::new();
        assert_eq!(store.get("acc").await.unwrap(), None);
        store.set("acc", "secret").await.unwrap();
        assert_eq!(store.get("acc").await.unwrap(), Some("secret".into()));
        store.delete("acc").await.unwrap();
        assert_eq!(store.get("acc").await.unwrap(), None);
    }
}
