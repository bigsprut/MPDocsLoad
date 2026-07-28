//! Профиль учётных данных (спец. §1.2, §2.4: `profile.rs`).
//!
//! Один профиль = один продавец на одном маркетплейсе.

use serde::{Deserialize, Serialize};

/// Набор учётных данных для одного продавца на одном маркетплейсе.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    /// Локальный идентификатор профиля (для SQLite-каталога).
    pub id: Option<i64>,
    /// Уникальное имя профиля (например, "Ozon-1", "WB-основной").
    pub name: String,
    /// Идентификатор провайдера (например, "ozon", "wildberries").
    pub provider_id: String,
    /// Человекочитаемое описание.
    pub description: Option<String>,
    /// Не-секретные метаданные авторизации (например, client_id).
    /// Секреты хранятся отдельно в keychain через `keychain_id`.
    pub auth_metadata: std::collections::BTreeMap<String, String>,
    /// Ключ в OS keychain, по которому лежит секрет.
    pub keychain_id: Option<String>,
}

impl Profile {
    /// Создаёт новый профиль с заданными именем и провайдером.
    #[must_use]
    pub fn new(name: impl Into<String>, provider_id: impl Into<String>) -> Self {
        Self {
            id: None,
            name: name.into(),
            provider_id: provider_id.into(),
            description: None,
            auth_metadata: std::collections::BTreeMap::new(),
            keychain_id: None,
        }
    }

    /// Добавляет не-секретное metadata-поле (builder).
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.auth_metadata.insert(key.into(), value.into());
        self
    }

    /// Возвращает значение metadata-поля.
    #[must_use]
    pub fn metadata(&self, key: &str) -> Option<&str> {
        self.auth_metadata.get(key).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_builder() {
        let p = Profile::new("Ozon-1", "ozon")
            .with_metadata("client_id", "1234567")
            .with_metadata("description", "main");
        assert_eq!(p.name, "Ozon-1");
        assert_eq!(p.provider_id, "ozon");
        assert_eq!(p.metadata("client_id"), Some("1234567"));
    }
}
