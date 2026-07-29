//! Интеграционный тест WildberriesProvider против mock-сервера (wiremock).
//!
//! Использует переменные окружения MDWF_WB_BASE_* для перенаправления доменов
//! на mock-сервер.

use base64::Engine;
use mdwf_core::{MarketplaceProvider, Profile};
use mdwf_providers_wildberries::WildberriesProvider;
use serial_test::serial;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn set_wb_base_urls(server_uri: &str) {
    let uri = server_uri.trim_end_matches('/');
    std::env::set_var("MDWF_WB_BASE_MARKETPLACE", uri);
    std::env::set_var("MDWF_WB_BASE_DOCUMENTS", uri);
    std::env::set_var("MDWF_WB_BASE_FINANCE", uri);
    std::env::set_var("MDWF_WB_BASE_STATISTICS", uri);
    std::env::set_var("MDWF_WB_BASE_ANALYTICS", uri);
    std::env::set_var("MDWF_WB_BASE_RETURNS", uri);
    std::env::set_var("MDWF_WB_NO_RATELIMIT", "1");
}

#[tokio::test]
#[serial(wb_env)]
async fn health_check_balance_ok() {
    let server = MockServer::start().await;
    set_wb_base_urls(&server.uri());

    Mock::given(method("GET"))
        .and(path("/api/v1/account/balance"))
        .and(header("Authorization", "wb-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {"balance": 1000.0}
        })))
        .mount(&server)
        .await;

    let provider = WildberriesProvider::new().unwrap();
    let profile = Profile::new("WB-1", "wildberries").with_metadata("token", "wb-token");
    let auth = provider.authenticator(&profile).await.unwrap();
    let status = provider.health_check(auth.as_ref()).await.unwrap();
    assert_eq!(status.level, mdwf_core::HealthLevel::Ok);
}

#[tokio::test]
#[serial(wb_env)]
async fn documents_api_three_step_pattern() {
    let server = MockServer::start().await;
    set_wb_base_urls(&server.uri());

    // Шаг 1: /categories возвращает категорию upd.
    Mock::given(method("GET"))
        .and(path("/api/v1/documents/categories"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {"categories": [{"name": "upd"}, {"name": "act-income-mp"}]}
        })))
        .mount(&server)
        .await;

    // Шаг 2: /list возвращает 2 документа (формат дока: data.documents).
    Mock::given(method("GET"))
        .and(path("/api/v1/documents/list"))
        .and(query_param("category", "upd"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {"documents": [
                {"serviceName": "upd-1001", "category": "upd", "amount": 512.0, "date": "2026-07-01"},
                {"serviceName": "upd-1002", "category": "upd", "amount": 1024.0, "date": "2026-07-02"}
            ]}
        })))
        .mount(&server)
        .await;

    // Шаг 3: /download/all возвращает base64-контент (формат дока: data.document).
    let content = b"<upd>test</upd>";
    let b64 = base64::engine::general_purpose::STANDARD.encode(content);
    Mock::given(method("POST"))
        .and(path("/api/v1/documents/download/all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {"fileName": "documents.zip", "extension": "zip", "document": b64}
        })))
        .mount(&server)
        .await;

    let provider = WildberriesProvider::new().unwrap();
    let profile = Profile::new("WB-1", "wildberries").with_metadata("token", "wb-token");
    let auth = provider.authenticator(&profile).await.unwrap();
    let report = provider.report("wb.documents").await.unwrap();

    // List шаг.
    let filter = mdwf_core::DocumentFilter {
        category: Some("upd".into()),
        ..Default::default()
    };
    let entries = report
        .list(auth.as_ref(), &filter, mdwf_core::CancelToken::new())
        .await
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].id, "upd-1001");
    assert_eq!(entries[0].category, "upd");

    // Download шаг (батч через /download/all).
    let params = mdwf_core::ReportParams::new().with("ids", "upd-1001,upd-1002");
    let files = report
        .download(
            auth.as_ref(),
            &params,
            std::sync::Arc::new(mdwf_core::NoopProgress) as std::sync::Arc<dyn mdwf_core::ProgressCallback>,
            mdwf_core::CancelToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].extension, "zip");
    // Проверяем, что контент декодирован правильно.
    assert_eq!(files[0].content.as_ref().unwrap(), content);
}

#[tokio::test]
#[serial(wb_env)]
async fn categories_report_lists_categories() {
    let server = MockServer::start().await;
    set_wb_base_urls(&server.uri());

    Mock::given(method("GET"))
        .and(path("/api/v1/documents/categories"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {"categories": [
                {"name": "upd"},
                {"name": "sale-to-le-signed"},
                {"name": "act-income-mp"}
            ]}
        })))
        .mount(&server)
        .await;

    let provider = WildberriesProvider::new().unwrap();
    let profile = Profile::new("WB-1", "wildberries").with_metadata("token", "wb-token");
    let auth = provider.authenticator(&profile).await.unwrap();
    let report = provider.report("wb.documents_categories").await.unwrap();

    let cats = report
        .list(auth.as_ref(), &mdwf_core::DocumentFilter::default(), mdwf_core::CancelToken::new())
        .await
        .unwrap();
    assert_eq!(cats.len(), 3);
    assert!(cats.iter().any(|c| c.id == "upd"));
    assert!(cats.iter().any(|c| c.id == "sale-to-le-signed"));
}
