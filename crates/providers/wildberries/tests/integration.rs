//! Интеграционный тест WildberriesProvider против mock-сервера (wiremock).
//!
//! Использует переменные окружения MDWF_WB_BASE_* для перенаправления доменов
//! на mock-сервер.

use base64::Engine;
use mdwf_core::{MarketplaceProvider, Profile};
use mdwf_providers_wildberries::WildberriesProvider;
use serial_test::serial;
use wiremock::matchers::{body_partial_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Прогресс-заглушка для вызовов list/download в тестах.
fn noop_progress() -> std::sync::Arc<dyn mdwf_core::ProgressCallback> {
    std::sync::Arc::new(mdwf_core::NoopProgress)
}

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
    // Поля сверены со схемой GetListDataDocumentsInner.
    Mock::given(method("GET"))
        .and(path("/api/v1/documents/list"))
        .and(query_param("category", "upd"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {"documents": [
                {"serviceName": "upd-1001", "name": "УПД №1001", "category": "upd",
                 "extensions": ["xml","zip"], "creationTime": "2026-07-01T10:00:00Z"},
                {"serviceName": "upd-1002", "name": "УПД №1002", "category": "upd",
                 "extensions": ["xml"], "creationTime": "2026-07-02T12:00:00Z"}
            ]}
        })))
        .mount(&server)
        .await;

    // Шаг 3: /download (GET, поштучно) возвращает base64-контент.
    // Формат ответа: data.{fileName, extension, document(base64)}.
    // fileName — осмысленное имя от WB (как в реальном ответе, напр.
    // «Акт №...pdf»). Именно оно становится базой имени файла на диске.
    let content = b"<upd>test</upd>";
    let b64 = base64::engine::general_purpose::STANDARD.encode(content);
    Mock::given(method("GET"))
        .and(path("/api/v1/documents/download"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {"fileName": "УПД №1001 от 01.07.2026.xml", "extension": "xml", "document": b64}
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
        .list(auth.as_ref(), &filter, std::sync::Arc::new(mdwf_core::NoopProgress) as std::sync::Arc<dyn mdwf_core::ProgressCallback>, mdwf_core::CancelToken::new())
        .await
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].id, "upd-1001");
    assert_eq!(entries[0].category, "upd");
    // display_name берётся из поля name.
    assert_eq!(entries[0].display_name, "УПД №1001");

    // Download шаг (поштучно через /download). Передаём doc_meta, чтобы
    // провайдер знал человекочитаемые имена (становятся source_id).
    let params = mdwf_core::ReportParams::new()
        .with("ids", "upd-1001,upd-1002")
        .with("doc_meta", r#"[{"id":"upd-1001","name":"УПД №1001","extension":"xml"},{"id":"upd-1002","name":"УПД №1002","extension":"xml"}]"#);
    let files = report
        .download(
            auth.as_ref(),
            &params,
            std::sync::Arc::new(mdwf_core::NoopProgress) as std::sync::Arc<dyn mdwf_core::ProgressCallback>,
            mdwf_core::CancelToken::new(),
        )
        .await
        .unwrap();
    // Поштучно: один файл на документ.
    assert_eq!(files.len(), 2);
    // Реальное расширение из ответа WB.
    assert_eq!(files[0].extension, "xml");
    // source_id = fileName из ответа /download (приоритет над name из меты UI),
    // с отрезанным расширением — оно добавляется шаблоном через {ext}.
    assert_eq!(files[0].source_id.as_deref(), Some("УПД №1001 от 01.07.2026"));
    // Проверяем, что контент декодирован правильно.
    assert_eq!(files[0].content.as_ref().unwrap(), content);
}

/// Пагинация /documents/list: WB отдаёт максимум 50 за запрос, поля total нет.
/// Провайдер должен перебирать страницы, пока не получит неполную (признак конца),
/// и обрезать по filter.limit (потолок общего числа).
#[tokio::test]
#[serial(wb_env)]
async fn documents_api_paginates_list() {
    let server = MockServer::start().await;
    set_wb_base_urls(&server.uri());

    // /categories — категория upd существует.
    Mock::given(method("GET"))
        .and(path("/api/v1/documents/categories"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {"categories": [{"name": "upd"}]}
        })))
        .mount(&server)
        .await;

    // Страница offset=0: 2 документа (полная «страница» с т.з. ceiling test — мало,
    // но для проверки truncate достаточно). Используем limit=1 в фильтре, чтобы
    // убедиться, что потолок обрезает результат до 1.
    Mock::given(method("GET"))
        .and(path("/api/v1/documents/list"))
        .and(query_param("offset", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {"documents": [
                {"serviceName": "upd-1", "name": "УПД №1", "category": "upd", "extensions": ["xml"]},
                {"serviceName": "upd-2", "name": "УПД №2", "category": "upd", "extensions": ["xml"]}
            ]}
        })))
        .mount(&server)
        .await;

    let provider = WildberriesProvider::new().unwrap();
    let profile = Profile::new("WB-1", "wildberries").with_metadata("token", "wb-token");
    let auth = provider.authenticator(&profile).await.unwrap();
    let report = provider.report("wb.documents").await.unwrap();

    // Потолок limit=1: несмотря на 2 документа в ответе, должны получить 1.
    let filter = mdwf_core::DocumentFilter {
        category: Some("upd".into()),
        limit: Some(1),
        ..Default::default()
    };
    let entries = report
        .list(auth.as_ref(), &filter, std::sync::Arc::new(mdwf_core::NoopProgress) as std::sync::Arc<dyn mdwf_core::ProgressCallback>, mdwf_core::CancelToken::new())
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, "upd-1");

    // Теперь без потолка (limit=None): WB вернул < PAGE_SIZE → одна страница,
    // получаем оба документа.
    let filter_all = mdwf_core::DocumentFilter {
        category: Some("upd".into()),
        limit: None,
        ..Default::default()
    };
    let entries_all = report
        .list(auth.as_ref(), &filter_all, std::sync::Arc::new(mdwf_core::NoopProgress) as std::sync::Arc<dyn mdwf_core::ProgressCallback>, mdwf_core::CancelToken::new())
        .await
        .unwrap();
    assert_eq!(entries_all.len(), 2);
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
        .list(auth.as_ref(), &mdwf_core::DocumentFilter::default(), std::sync::Arc::new(mdwf_core::NoopProgress) as std::sync::Arc<dyn mdwf_core::ProgressCallback>, mdwf_core::CancelToken::new())
        .await
        .unwrap();
    assert_eq!(cats.len(), 3);
    assert!(cats.iter().any(|c| c.id == "upd"));
    assert!(cats.iter().any(|c| c.id == "sale-to-le-signed"));
}

// =========================================================================
// Аудит 2026-08-14: пагинация и параметры по спеке (eslazarev/wildberries-sdk)
// =========================================================================

/// Полный обход detailed-финансов: курсор rrdId (последняя строка страницы),
/// конец данных — 204 No Content (клиент обязан вернуть пустой массив, а не
/// ошибку протокола). Раньше выгружалась только первая тысяча строк.
#[tokio::test]
#[serial(wb_env)]
async fn finance_detailed_paginates_rrdid_until_204() {
    let server = MockServer::start().await;
    set_wb_base_urls(&server.uri());

    // Страница 1: rrdId=0 → 2 строки (курсор — rrdId последней = 20).
    Mock::given(method("POST"))
        .and(path("/api/finance/v1/sales-reports/detailed"))
        .and(body_partial_json(serde_json::json!({ "rrdId": 0 })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"rrdId": 10, "supplierArticle": "A"},
            {"rrdId": 20, "supplierArticle": "B"}
        ])))
        .mount(&server)
        .await;

    // Страница 2: rrdId=20 → 204 (данных больше нет).
    Mock::given(method("POST"))
        .and(path("/api/finance/v1/sales-reports/detailed"))
        .and(body_partial_json(serde_json::json!({ "rrdId": 20 })))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let provider = WildberriesProvider::new().unwrap();
    let profile = Profile::new("WB-1", "wildberries").with_metadata("token", "wb-token");
    let auth = provider.authenticator(&profile).await.unwrap();
    let report = provider.report("wb.sales_reports_detailed").await.unwrap();

    let params = mdwf_core::ReportParams::new()
        .with("date_from", "2026-07-01")
        .with("date_to", "2026-07-31");
    let files = report
        .download(auth.as_ref(), &params, noop_progress(), mdwf_core::CancelToken::new())
        .await
        .unwrap();
    assert_eq!(files.len(), 1);
    // Непустой результат конвертируется в Excel: расширение xlsx, magic PK (ZIP).
    assert_eq!(files[0].extension, "xlsx");
    let bytes = files[0].content.as_ref().unwrap();
    assert_eq!(&bytes[..2], b"PK", "xlsx должен быть ZIP");
    // Запросов ровно два: страница 1 + страница 2 (204 — конец).
    let hits = server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|r| r.url.as_str().contains("/sales-reports/detailed"))
        .count();
    assert_eq!(hits, 2);
}

/// Пустой результат (нет данных за период) остаётся честным JSON `[]`,
/// а не пустым xlsx.
#[tokio::test]
#[serial(wb_env)]
async fn empty_detailed_result_stays_json() {
    let server = MockServer::start().await;
    set_wb_base_urls(&server.uri());

    Mock::given(method("POST"))
        .and(path("/api/finance/v1/sales-reports/detailed"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;

    let provider = WildberriesProvider::new().unwrap();
    let profile = Profile::new("WB-1", "wildberries").with_metadata("token", "wb-token");
    let auth = provider.authenticator(&profile).await.unwrap();
    let report = provider.report("wb.sales_reports_detailed").await.unwrap();

    let params = mdwf_core::ReportParams::new()
        .with("date_from", "2026-07-01")
        .with("date_to", "2026-07-31");
    let files = report
        .download(auth.as_ref(), &params, noop_progress(), mdwf_core::CancelToken::new())
        .await
        .unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].extension, "json");
    assert_eq!(files[0].content.as_ref().unwrap(), b"[]");
}

/// Claims: обязательный is_archive, limit=200, offset-пагинация по total,
/// локальная фильтрация по dt в выбранном периоде.
#[tokio::test]
#[serial(wb_env)]
async fn claims_sweeps_active_and_archived_with_local_date_filter() {
    let server = MockServer::start().await;
    set_wb_base_urls(&server.uri());

    Mock::given(method("GET"))
        .and(path("/api/v1/claims"))
        .and(query_param("is_archive", "false"))
        .and(query_param("limit", "200"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "claims": [
                {"id": "c-active", "imt_name": "Кроссовки", "dt": "2026-07-02T10:00:00"},
                {"id": "c-active-out", "imt_name": "Шапка", "dt": "2026-08-01"}
            ],
            "total": 2
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/claims"))
        .and(query_param("is_archive", "true"))
        .and(query_param("limit", "200"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "claims": [
                {"id": "c-arch", "imt_name": "Футболка", "dt": "2026-07-15"}
            ],
            "total": 1
        })))
        .mount(&server)
        .await;

    let provider = WildberriesProvider::new().unwrap();
    let profile = Profile::new("WB-1", "wildberries").with_metadata("token", "wb-token");
    let auth = provider.authenticator(&profile).await.unwrap();
    let report = provider.report("wb.claims").await.unwrap();

    let filter = mdwf_core::DocumentFilter {
        date_from: chrono::NaiveDate::from_ymd_opt(2026, 7, 1),
        date_to: chrono::NaiveDate::from_ymd_opt(2026, 7, 31),
        ..Default::default()
    };
    let entries = report
        .list(auth.as_ref(), &filter, noop_progress(), mdwf_core::CancelToken::new())
        .await
        .unwrap();
    // Архивная + активная за июль; заявка августа отфильтрована локально.
    let mut ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, vec!["c-active", "c-arch"]);
    // Дата заявки протащена в DocumentEntry (для {doc_date}/архива).
    let with_date = entries.iter().find(|e| e.id == "c-arch").unwrap();
    assert_eq!(with_date.date, chrono::NaiveDate::from_ymd_opt(2026, 7, 15));
}

/// Удержания: обязательные dateTo/limit в query, разбор {data:{reports,total}},
/// конец по неполной странице.
#[tokio::test]
#[serial(wb_env)]
async fn penalties_send_required_params() {
    let server = MockServer::start().await;
    set_wb_base_urls(&server.uri());

    Mock::given(method("GET"))
        .and(path("/api/analytics/v1/deductions"))
        .and(query_param("dateTo", "2026-07-31T23:59:59+03:00"))
        .and(query_param("limit", "1000"))
        .and(query_param("offset", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {"reports": [{"nmId": 1, "subjectName": "S1"}], "total": 1}
        })))
        .mount(&server)
        .await;

    let provider = WildberriesProvider::new().unwrap();
    let profile = Profile::new("WB-1", "wildberries").with_metadata("token", "wb-token");
    let auth = provider.authenticator(&profile).await.unwrap();
    let report = provider.report("wb.deductions").await.unwrap();

    let filter = mdwf_core::DocumentFilter {
        date_from: chrono::NaiveDate::from_ymd_opt(2026, 7, 1),
        date_to: chrono::NaiveDate::from_ymd_opt(2026, 7, 31),
        ..Default::default()
    };
    let entries = report
        .list(auth.as_ref(), &filter, noop_progress(), mdwf_core::CancelToken::new())
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, "1");
    assert_eq!(entries[0].display_name, "S1");
}

/// Async-отчёт приёмки: create (даты ГГГГ-ММ-ДД в query) → taskId →
/// poll status (done) → download (прямой массив строк).
#[tokio::test]
#[serial(wb_env)]
async fn acceptance_report_create_poll_download() {
    let server = MockServer::start().await;
    set_wb_base_urls(&server.uri());

    Mock::given(method("GET"))
        .and(path("/api/v1/acceptance_report"))
        .and(query_param("dateFrom", "2026-07-01"))
        .and(query_param("dateTo", "2026-07-31"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {"taskId": "t-42"}
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/acceptance_report/tasks/t-42/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {"id": "t-42", "status": "done"}
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/acceptance_report/tasks/t-42/download"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"count": 3, "incomeId": "111", "nmID": 7, "subjectName": "Товар"}
        ])))
        .mount(&server)
        .await;

    let provider = WildberriesProvider::new().unwrap();
    let profile = Profile::new("WB-1", "wildberries").with_metadata("token", "wb-token");
    let auth = provider.authenticator(&profile).await.unwrap();
    let report = provider.report("wb.acceptance_report").await.unwrap();

    let params = mdwf_core::ReportParams::new()
        .with("date_from", "2026-07-01")
        .with("date_to", "2026-07-31");
    let files = report
        .download(auth.as_ref(), &params, noop_progress(), mdwf_core::CancelToken::new())
        .await
        .unwrap();
    assert_eq!(files.len(), 1);
    // Непустой результат — Excel (ZIP magic).
    assert_eq!(files[0].extension, "xlsx");
    assert_eq!(&files[0].content.as_ref().unwrap()[..2], b"PK");
}

/// Отчёт приёмки с периодом > 31 дня — внятный отказ (спека: максимум 31 день).
#[tokio::test]
#[serial(wb_env)]
async fn acceptance_report_rejects_range_over_31_days() {
    let provider = WildberriesProvider::new().unwrap();
    let profile = Profile::new("WB-1", "wildberries").with_metadata("token", "wb-token");
    let auth = provider.authenticator(&profile).await.unwrap();
    let report = provider.report("wb.acceptance_report").await.unwrap();

    // Квартал: 01.07–30.09 = 92 дня.
    let params = mdwf_core::ReportParams::new()
        .with("date_from", "2026-07-01")
        .with("date_to", "2026-09-30");
    let err = report
        .download(auth.as_ref(), &params, noop_progress(), mdwf_core::CancelToken::new())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("31"), "got: {err}");
}

/// Статистика: спека допускает только dateFrom (по lastChangeDate) и flag —
/// dateTo/limit не существуют и не должны отправляться.
#[tokio::test]
#[serial(wb_env)]
async fn statistics_sends_datefrom_and_flag_only() {
    let server = MockServer::start().await;
    set_wb_base_urls(&server.uri());

    Mock::given(method("GET"))
        .and(path("/api/v1/supplier/orders"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"srid": "S1", "supplierArticle": "A", "lastChangeDate": "2026-07-02T00:00:00"}
        ])))
        .mount(&server)
        .await;

    let provider = WildberriesProvider::new().unwrap();
    let profile = Profile::new("WB-1", "wildberries").with_metadata("token", "wb-token");
    let auth = provider.authenticator(&profile).await.unwrap();
    let report = provider.report("wb.orders").await.unwrap();

    let filter = mdwf_core::DocumentFilter {
        date_from: chrono::NaiveDate::from_ymd_opt(2026, 7, 1),
        date_to: chrono::NaiveDate::from_ymd_opt(2026, 7, 31),
        ..Default::default()
    };
    let entries = report
        .list(auth.as_ref(), &filter, noop_progress(), mdwf_core::CancelToken::new())
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);

    // Проверяем фактический URL запроса: dateFrom и flag есть, dateTo — НЕТ.
    let reqs = server.received_requests().await.unwrap_or_default();
    let url = reqs
        .iter()
        .find(|r| r.url.as_str().contains("/supplier/orders"))
        .expect("запрос orders")
        .url
        .to_string();
    assert!(url.contains("dateFrom="), "url: {url}");
    assert!(url.contains("flag=0"), "url: {url}");
    assert!(!url.contains("dateTo"), "url: {url}");
    assert!(!url.contains("limit"), "url: {url}");
}
