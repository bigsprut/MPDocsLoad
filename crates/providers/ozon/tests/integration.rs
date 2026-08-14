//! Интеграционный тест OzonProvider против mock-сервера (wiremock).
//!
//! Проверяет авторизацию (заголовки Client-Id/Api-Key), retry policy
//! и парсинг ответа /v1/finance/balance.

use mdwf_core::{MarketplaceProvider, Profile};
use mdwf_providers_ozon::{OzonProvider, RetryPolicy};
use std::time::Duration;
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

/// Circuit breaker: после порога отказов (5×500) post() быстро падает с
/// «circuit breaker open», не вырабатывая весь лимит ретраев. Доказывает, что
/// `post_url` консультирует `breaker.check()` (прежний баг: breaker только
/// «считал» отказы, но не размыкал запрос — check() не вызывался).
#[tokio::test]
async fn breaker_blocks_after_threshold() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/finance/balance"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    // Крошечные задержки + много ретраев: breaker (threshold 5) открывается за
    // миллисекунды, тест не спит по экспоненте (500мс→…→8с).
    let retry = RetryPolicy {
        max_retries: 20,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(1),
    };
    let provider = OzonProvider::with_base_url_and_retry(&server.uri(), retry).unwrap();
    let profile = Profile::new("x", "ozon")
        .with_metadata("client_id", "1")
        .with_metadata("api_key", "k");
    let auth = provider.authenticator(&profile).await.unwrap();

    // health_check дёргает client.post(/v1/finance/balance) — там всегда 500.
    // Без фикса: 20 ретраев 500 → Api(500) → Degraded «server error».
    // С фиксом: после 5 отказов breaker открывается → Unavailable
    // «временно недоступно: circuit breaker open …» → Degraded (transient).
    let status = provider.health_check(auth.as_ref()).await.unwrap();
    assert!(
        status.message.contains("circuit breaker open"),
        "ожидали «circuit breaker open», получили: {}",
        status.message
    );
    // Unavailable — transient: health_check классифицирует как Degraded, не Down.
    assert_eq!(status.level, mdwf_core::HealthLevel::Degraded);
}

// =========================================================================
// N/A-разбор 2026-08-14: b2b friendly-error, postings retry+fallback (/v3)
// =========================================================================

/// b2b_sales: gRPC NotFound «finance document not found» на create →
/// человекочитаемая ошибка «кабинет не продаёт юрлицам», не сырая трасса.
#[tokio::test]
async fn b2b_no_document_maps_to_friendly_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/finance/document-b2b-sales"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "code": 5,
            "message": "service.CreateDocumentB2BSalesReport: createMetazonMarketplaceSSRS: getFinanceDocumentID: rpc error: code = NotFound desc = finance document not found"
        })))
        .mount(&server)
        .await;

    let provider = OzonProvider::with_base_url_and_retry(
        &server.uri(),
        RetryPolicy { max_retries: 0, ..Default::default() },
    )
    .unwrap();
    let profile = Profile::new("Ozon-1", "ozon")
        .with_metadata("client_id", "1")
        .with_metadata("api_key", "k");
    let auth = provider.authenticator(&profile).await.unwrap();
    let report = provider.report("ozon.b2b_sales").await.unwrap();

    let err = report
        .download(auth.as_ref(), &mdwf_core::ReportParams::new().with("date_from", "2026-07-01").with("date_to", "2026-07-31"),
            std::sync::Arc::new(mdwf_core::NoopProgress) as std::sync::Arc<dyn mdwf_core::ProgressCallback>,
            mdwf_core::CancelToken::new())
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("не продаёт юрлицам"), "ожидали friendly-текст, получили: {msg}");
    assert!(msg.contains("B2B"), "в сообщении должна быть расшифровка: {msg}");
}

/// postings: серверная генерация стабильно падает («Failed to build report») →
/// после ретраев фоллбэк на /v3/posting/fbo/list → xlsx. Create вызывается
/// 1+POSTINGS_BUILD_RETRIES раз; пауза ретрая через env — миллисекунды.
#[tokio::test]
async fn postings_build_failure_falls_back_to_fbo_list() {
    std::env::set_var("MDWF_OZON_BUILD_RETRY_MS", "1");
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/report/postings/create"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {"code": "c-1"}
        })))
        .expect(3) // 1 + 2 ретрая
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/report/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {"status": "failed", "error": "Failed to build report. Try again later."}
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v3/posting/fbo/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "has_next": false,
            "cursor": "",
            "postings": [
                {
                    "posting_number": "123-1",
                    "order_number": "123",
                    "status": "delivered",
                    "substatus": "posting_received",
                    "products": [
                        {"offer_id": "A-1", "sku": 42, "name": "Товар", "quantity": 2,
                         "price": {"amount": "199", "currency": "RUB"}}
                    ],
                    "analytics_data": {"warehouse_name": "СКЛАД-1", "city": "Москва", "delivery_type": "PVZ"},
                    "financial_data": {"products": [{"product_id": 42, "old_price": 249}]}
                }
            ]
        })))
        .mount(&server)
        .await;

    let provider = OzonProvider::with_base_url_and_retry(
        &server.uri(),
        RetryPolicy { max_retries: 0, ..Default::default() },
    )
    .unwrap();
    let profile = Profile::new("Ozon-1", "ozon")
        .with_metadata("client_id", "1")
        .with_metadata("api_key", "k");
    let auth = provider.authenticator(&profile).await.unwrap();
    let report = provider.report("ozon.postings").await.unwrap();

    let files = report
        .download(auth.as_ref(), &mdwf_core::ReportParams::new().with("date_from", "2026-07-01").with("date_to", "2026-07-31"),
            std::sync::Arc::new(mdwf_core::NoopProgress) as std::sync::Arc<dyn mdwf_core::ProgressCallback>,
            mdwf_core::CancelToken::new())
        .await
        .unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].extension, "xlsx");
    let bytes = files[0].content.as_ref().unwrap();
    assert_eq!(&bytes[..2], b"PK", "xlsx = ZIP");
    // Пометка пользователю: файл собран программой (сервер не смог).
    let note = files[0].note.as_deref().unwrap_or_default();
    assert!(note.contains("собрала его сама"), "note: {note}");
    std::env::remove_var("MDWF_OZON_BUILD_RETRY_MS");
}
