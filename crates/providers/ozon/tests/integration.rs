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
async fn browsable_list_extracts_entries() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/finance/accrual/postings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {
                "rows": [
                    {"posting_number": "234-1-1", "amount": 100.0},
                    {"posting_number": "234-1-2", "amount": 200.0}
                ]
            }
        })))
        .mount(&server)
        .await;

    let provider = OzonProvider::with_base_url(&server.uri()).unwrap();
    let profile = Profile::new("Ozon-1", "ozon")
        .with_metadata("client_id", "1")
        .with_metadata("api_key", "k");
    let auth = provider.authenticator(&profile).await.unwrap();
    let report = provider.report("ozon.accrual_postings").await.unwrap();

    assert_eq!(report.acquisition_mode(), mdwf_core::AcquisitionMode::Browsable);
    let entries = report
        .list(
            auth.as_ref(),
            &mdwf_core::DocumentFilter::default(),
            std::sync::Arc::new(mdwf_core::NoopProgress) as std::sync::Arc<dyn mdwf_core::ProgressCallback>,
            mdwf_core::CancelToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].id, "234-1-1");
    assert_eq!(entries[1].display_name, "234-1-2");
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
