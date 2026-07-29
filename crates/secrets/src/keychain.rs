//! Трейт хранилища секретов (спец. §2.4: `keychain.rs`).

use async_trait::async_trait;

use mdwf_core::CoreResult;

/// Имя сервиса в keychain (общее для всех секретов MDWF).
pub const KEYCHAIN_SERVICE: &str = "dev.mdwf.MDWF";

/// Трейт хранилища секретов.
///
/// Реализации: `OsKeychain` (Windows Credential Manager / macOS Keychain / Linux Secret Service),
/// `InMemorySecretStore` (для тестов).
#[async_trait]
pub trait SecretStore: Send + Sync {
    /// Сохраняет секрет под указанным именем пользователя (`account`).
    async fn set(&self, account: &str, secret: &str) -> CoreResult<()>;

    /// Читает секрет. Возвращает `None`, если не найден.
    async fn get(&self, account: &str) -> CoreResult<Option<String>>;

    /// Удаляет секрет. Если не существует — Ok(()).
    async fn delete(&self, account: &str) -> CoreResult<()>;
}
