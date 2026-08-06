//! Интеграционный тест OzonProvider против mock-сервера (wiremock).
//!
//! Проверяет авторизацию (заголовки Client-Id/Api-Key), retry policy
//! и парсинг ответа /v1/finance/balance.

use mdwf_core::{MarketplaceProvider, Profile};
use mdwf_providers_ozon::OzonProvider;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn health_check_success_against_mock() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/finance/balance"))
        .and(header("Client-Id", "1234567"))
        .and(header("Api-Key", "test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {"balance": 100.0}
        })))
        .mount(&server)
        .await;

    let provider = OzonProvider::with_base_url(&server.uri()).unwrap();
    let profile = Profile::new("Ozon-1", "ozon")
        .with_metadata("client_id", "1234567")
        .with_metadata("api_key", "test-key");
    let auth = provider.authenticator(&profile).await.unwrap();
    let status = provider.health_check(auth.as_ref()).await.unwrap();

    assert_eq!(status.level, mdwf_core::HealthLevel::Ok);
}

#[tokio::test]
async fn health_check_auth_failure() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/finance/balance"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;

    let provider = OzonProvider::with_base_url(&server.uri()).unwrap();
    let profile = Profile::new("Ozon-1", "ozon")
        .with_metadata("client_id", "1")
        .with_metadata("api_key", "bad");
    let auth = provider.authenticator(&profile).await.unwrap();
    let status = provider.health_check(auth.as_ref()).await.unwrap();

    assert!(status.is_down());
    assert!(status.message.contains("auth"));
}

#[tokio::test]
async fn retry_on_429_then_succeed() {
    let server = MockServer::start().await;

    // Первые 2 запроса -> 429, третий -> 200.
    Mock::given(method("POST"))
        .and(path("/v1/finance/balance"))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/finance/balance"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": {}})))
        .mount(&server)
        .await;

    let provider = OzonProvider::with_base_url(&server.uri()).unwrap();
    let profile = Profile::new("x", "ozon")
        .with_metadata("client_id", "1")
        .with_metadata("api_key", "k");
    let auth = provider.authenticator(&profile).await.unwrap();
    let status = provider.health_check(auth.as_ref()).await.unwrap();
    assert_eq!(status.level, mdwf_core::HealthLevel::Ok);
}

/// `/v1/seller/info` → `company.legal_name` (сверено с docs.ozon.ru).
#[tokio::test]
async fn account_display_name_from_seller_info() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/seller/info"))
        .and(header("Client-Id", "7707083"))
        .and(header("Api-Key", "k"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "company": {
                "country": "Россия",
                "inn": "7707083893",
                "name": "ООО 'Ромашка'",
                "legal_name": "Общество с ограниченной ответственностью 'Ромашка'"
            },
            "ratings": [],
            "subscription": {"is_premium": false}
        })))
        .mount(&server)
        .await;

    let provider = OzonProvider::with_base_url(&server.uri()).unwrap();
    let profile = Profile::new("Ozon-1", "ozon")
        .with_metadata("client_id", "7707083")
        .with_metadata("api_key", "k");
    let auth = provider.authenticator(&profile).await.unwrap();

    // Берём legal_name (полное юр. наименование), не краткое name.
    let name = provider.account_display_name(auth.as_ref()).await.unwrap();
    assert_eq!(
        name.as_deref(),
        Some("Общество с ограниченной ответственностью 'Ромашка'")
    );
}

/// `/v1/seller/info` без `company.name` → `None` (не падаем).
#[tokio::test]
async fn account_display_name_missing_field() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/seller/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "company": {}
        })))
        .mount(&server)
        .await;

    let provider = OzonProvider::with_base_url(&server.uri()).unwrap();
    let profile = Profile::new("Ozon-1", "ozon")
        .with_metadata("client_id", "1")
        .with_metadata("api_key", "k");
    let auth = provider.authenticator(&profile).await.unwrap();

    let name = provider.account_display_name(auth.as_ref()).await.unwrap();
    assert!(name.is_none());
}
