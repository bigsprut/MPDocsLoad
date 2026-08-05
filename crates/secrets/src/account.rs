//! Хелперы для выноса секретов профиля в OS keychain.
//!
//! Модель: `auth_metadata` профиля хранит только **несекретные** поля
//! (напр. `client_id`). Секретные поля (`AuthField.secret == true`: ozon
//! `api_key`, wb `token`) хранятся **только в keyring** под детерминированным
//! ключом `mdwf:{provider_id}:{profile_name}:{field_id}`. Перед вызовом
//! `provider.authenticator(&profile)` секрет подмешивается в `auth_metadata`
//! in-memory — провайдеры не меняются.
//!
//! Сброс профилей при старте (после перехода на keyring-only) делается в
//! app/CLI слоях через `Catalog::clear_profiles`; миграции нет — пользователь
//! создаёт профили заново.

use mdwf_core::{Capabilities, CoreResult, Profile};

use crate::keychain::SecretStore;

/// Формирует детерминированный account-ключ для keyring.
///
/// Формат: `mdwf:{provider_id}:{profile_name}:{field_id}`.
/// По этому ключу секрет сохраняется/читается/удаляется в keyring. Использование
/// детерминированного ключа (а не хранимого `keychain_id`) позволяет не вести
/// отдельное состояние и поддерживает любое количество секретных полей.
#[must_use]
pub fn account_key(provider_id: &str, profile_name: &str, field_id: &str) -> String {
    format!("mdwf:{provider_id}:{profile_name}:{field_id}")
}

/// Возвращает `field_id` всех секретных полей из capabilities провайдера
/// (`AuthField.secret == true`). Используется, чтобы знать, какие ключи
/// `auth_metadata` являются секретами — их нужно выносить в keyring.
#[must_use]
pub fn secret_field_ids(caps: &Capabilities) -> Vec<String> {
    caps.auth_fields
        .iter()
        .filter(|f| f.secret)
        .map(|f| f.id.clone())
        .collect()
}

/// Выносит секреты из `profile.auth_metadata` в keyring и удаляет их из
/// `auth_metadata` (in-place). Несекретные поля остаются.
///
/// После вызова профиль можно безопасно записать в SQLite — секретов там не будет.
pub async fn store_profile_secrets(
    profile: &mut Profile,
    secret_fields: &[String],
    secrets: &dyn SecretStore,
) -> CoreResult<()> {
    for field_id in secret_fields {
        // Вынимаем секрет из metadata (если есть) и кладём в keyring.
        if let Some(value) = profile.auth_metadata.remove(field_id) {
            if !value.is_empty() {
                let key = account_key(&profile.provider_id, &profile.name, field_id);
                secrets.set(&key, &value).await?;
            }
        }
    }
    Ok(())
}

/// Подмешивает секреты из keyring в копию `profile.auth_metadata` (in-memory).
/// Возвращает профиль, пригодный для передачи в `provider.authenticator` —
/// провайдеры читают секрет из `auth_metadata` как раньше.
///
/// Если секрета в keyring нет — поле не добавляется (провайдер вернёт свою ошибку
/// «profile missing ...»).
pub async fn load_profile_secrets(
    mut profile: Profile,
    secret_fields: &[String],
    secrets: &dyn SecretStore,
) -> CoreResult<Profile> {
    for field_id in secret_fields {
        let key = account_key(&profile.provider_id, &profile.name, field_id);
        match secrets.get(&key).await {
            Ok(Some(value)) => {
                profile.auth_metadata.insert(field_id.clone(), value);
            }
            Ok(None) => { /* секрета нет — оставляем провайдеру сообщить об ошибке */ }
            Err(e) => return Err(e),
        }
    }
    Ok(profile)
}

/// Удаляет все секреты профиля из keyring. Вызывается при удалении профиля,
/// чтобы не оставлять «висячие» секреты в Credential Manager.
pub async fn delete_profile_secrets(
    provider_id: &str,
    profile_name: &str,
    secret_fields: &[String],
    secrets: &dyn SecretStore,
) -> CoreResult<()> {
    for field_id in secret_fields {
        let key = account_key(provider_id, profile_name, field_id);
        // delete идемпотентен (Ok при NoEntry).
        secrets.delete(&key).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::InMemorySecretStore;

    fn profile_with(provider: &str, name: &str, meta: &[(&str, &str)]) -> Profile {
        let mut p = Profile::new(name, provider);
        for (k, v) in meta {
            p.auth_metadata.insert((*k).to_string(), (*v).to_string());
        }
        p
    }

    #[test]
    fn account_key_format() {
        assert_eq!(
            account_key("ozon", "shop1", "api_key"),
            "mdwf:ozon:shop1:api_key"
        );
        assert_eq!(account_key("wb", "p", "token"), "mdwf:wb:p:token");
    }

    #[tokio::test]
    async fn store_then_load_roundtrip() {
        let store = InMemorySecretStore::new();
        let mut p = profile_with(
            "ozon",
            "shop1",
            &[("client_id", "123"), ("api_key", "secret-value")],
        );
        let secret_fields = vec!["api_key".to_string()];

        // Выносим секрет → в metadata остаётся только client_id.
        store_profile_secrets(&mut p, &secret_fields, &store)
            .await
            .unwrap();
        assert_eq!(p.auth_metadata.get("api_key"), None);
        assert_eq!(p.auth_metadata.get("client_id").map(String::as_str), Some("123"));

        // Подмешиваем → клиент видит оба поля.
        let loaded = load_profile_secrets(p, &secret_fields, &store)
            .await
            .unwrap();
        assert_eq!(loaded.auth_metadata.get("client_id").map(String::as_str), Some("123"));
        assert_eq!(
            loaded.auth_metadata.get("api_key").map(String::as_str),
            Some("secret-value")
        );
    }

    #[tokio::test]
    async fn delete_removes_secret() {
        let store = InMemorySecretStore::new();
        let mut p = profile_with("wb", "p1", &[("token", "tok")]);
        let secret_fields = vec!["token".to_string()];
        store_profile_secrets(&mut p, &secret_fields, &store)
            .await
            .unwrap();
        // Секрет в keyring.
        let key = account_key("wb", "p1", "token");
        assert_eq!(store.get(&key).await.unwrap().as_deref(), Some("tok"));

        delete_profile_secrets("wb", "p1", &secret_fields, &store)
            .await
            .unwrap();
        assert_eq!(store.get(&key).await.unwrap(), None);
    }

    #[tokio::test]
    async fn store_empty_value_skipped() {
        // Пустое значение секрета не должно попадать в keyring.
        let store = InMemorySecretStore::new();
        let mut p = profile_with("ozon", "s", &[("api_key", "")]);
        store_profile_secrets(&mut p, &["api_key".to_string()], &store)
            .await
            .unwrap();
        let key = account_key("ozon", "s", "api_key");
        assert_eq!(store.get(&key).await.unwrap(), None);
    }

    #[tokio::test]
    async fn load_missing_secret_leaves_absent() {
        // Нет секрета в keyring → поле не добавляется (провайдер сам сообщит об ошибке).
        let store = InMemorySecretStore::new();
        let p = profile_with("ozon", "s", &[("client_id", "1")]);
        let loaded = load_profile_secrets(p, &["api_key".to_string()], &store)
            .await
            .unwrap();
        assert_eq!(loaded.auth_metadata.get("api_key"), None);
        assert_eq!(loaded.auth_metadata.get("client_id").map(String::as_str), Some("1"));
    }

    #[test]
    fn secret_field_ids_from_caps() {
        use mdwf_core::capabilities::{AuthField, AuthFieldKind};
        let caps = Capabilities {
            auth_type: mdwf_core::AuthType::ApiKey,
            auth_fields: vec![
                AuthField {
                    id: "client_id".into(),
                    label: "Client-Id".into(),
                    kind: AuthFieldKind::Number,
                    required: true,
                    placeholder: None,
                    help_text: None,
                    secret: false,
                },
                AuthField {
                    id: "api_key".into(),
                    label: "Api-Key".into(),
                    kind: AuthFieldKind::Password,
                    required: true,
                    placeholder: None,
                    help_text: None,
                    secret: true,
                },
            ],
            reports: vec![],
        };
        let ids = secret_field_ids(&caps);
        assert_eq!(ids, vec!["api_key".to_string()]);
    }
}
