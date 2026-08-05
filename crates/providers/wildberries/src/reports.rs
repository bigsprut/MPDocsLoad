//! Описания отчётов WB (сверено с официальной документацией dev.wildberries.ru).
//!
//! Разделы документации (все домены подтверждены):
//! - Баланс:           GET  finance-api      /api/v1/account/balance
//! - Финансы:          POST finance-api      /api/finance/v1/sales-reports/* , /acquiring/*
//! - Документы:        GET  documents-api    /api/v1/documents/*
//! - Отчёты (стат):    GET  statistics-api   /api/v1/supplier/orders, /sales
//! - Аналитика/данные: POST seller-analytics /api/analytics/v3/*, GET /api/analytics/v1/*
//! - Удержания:        GET  seller-analytics /api/analytics/v1/{deductions,measurement-penalties,...}
//! - Возвраты:         GET  seller-analytics /api/v1/analytics/goods-return

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use mdwf_core::{
    capabilities::{AuthField, AuthFieldKind, AuthType},
    AcquisitionMode, Capabilities, CoreError, CoreResult, CancelToken, DocumentEntry,
    DocumentFilter, DownloadedFile, DownloaderKind, ProgressCallbackRef, Report, ReportCategory,
    ReportDescriptor, ReportParameter, ReportParameterKind, ReportParams, ReportRef,
};

use crate::client::{WbDomain, WbHttpClient};
use crate::date_format;

// =========================================================================
// Capabilities
// =========================================================================

/// Возвращает Capabilities WB: тип авторизации + поле токена + отчёты.
#[must_use]
pub fn capabilities() -> Capabilities {
    Capabilities {
        auth_type: AuthType::BearerToken,
        auth_fields: vec![AuthField {
            id: "token".into(),
            label: "API-токен".into(),
            kind: AuthFieldKind::Password,
            required: true,
            placeholder: None,
            help_text: Some(
                "Токен доступа (тип Personal). Личный кабинет → Профиль → Доступ к API.".into(),
            ),
            secret: true,
        }],
        reports: all_report_descriptors(),
    }
}

/// Все дескрипторы отчётов WB (по официальной документации).
#[must_use]
pub fn all_report_descriptors() -> Vec<ReportDescriptor> {
    vec![
        // --- Баланс (finance-api, GET) ---
        desc_period("wb.balance", "Баланс продавца", ReportCategory::Finance),
        // --- Финансы (finance-api, POST) ---
        desc_period(
            "wb.sales_reports_list",
            "Реестр реализации (список)",
            ReportCategory::Finance,
        ),
        desc_period(
            "wb.sales_reports_detailed",
            "Детализация реализации (за период)",
            ReportCategory::Finance,
        ),
        desc_period(
            "wb.acquiring_list",
            "Эквайринг (список)",
            ReportCategory::Finance,
        ),
        desc_period(
            "wb.acquiring_detailed",
            "Эквайринг (детализация)",
            ReportCategory::Finance,
        ),
        // --- Документы (documents-api, GET) ---
        desc_browsable(
            "wb.documents",
            "Документы (УПД/УКД/акты) — по категории",
            ReportCategory::Documents,
        ),
        // --- Статистика (statistics-api, GET) ---
        desc_browsable("wb.orders", "Заказы", ReportCategory::Operational),
        desc_browsable("wb.sales", "Продажи", ReportCategory::Operational),
        // --- Удержания (seller-analytics-api, GET) ---
        desc_browsable(
            "wb.deductions",
            "Штрафы за подмены",
            ReportCategory::Penalties,
        ),
        desc_browsable(
            "wb.measurement_penalties",
            "Штрафы за габариты",
            ReportCategory::Penalties,
        ),
        desc_browsable(
            "wb.antifraud",
            "Самовыкупы (антифрод)",
            ReportCategory::Penalties,
        ),
        // --- Возвраты (claims) — спека §2.2.2 ---
        desc_browsable(
            "wb.claims",
            "Возвраты (claims)",
            ReportCategory::Returns,
        ),
        // --- Async-отчёт приёмки (спека §2.2.2) ---
        desc_period(
            "wb.acceptance_report",
            "Аналитический отчёт приёмки (async)",
            ReportCategory::Finance,
        ),
    ]
}

// --- Хелперы построения дескрипторов ---

#[allow(clippy::needless_pass_by_value)]
fn desc_period(
    type_id: &str,
    display_name: &str,
    category: ReportCategory,
) -> ReportDescriptor {
    ReportDescriptor {
        type_id: type_id.into(),
        display_name: display_name.into(),
        category,
        acquisition_mode: AcquisitionMode::Period,
        downloader_kind: DownloaderKind::Api,
        parameters: vec![param_date_range()],
    }
}

#[allow(clippy::needless_pass_by_value)]
fn desc_browsable(
    type_id: &str,
    display_name: &str,
    category: ReportCategory,
) -> ReportDescriptor {
    ReportDescriptor {
        type_id: type_id.into(),
        display_name: display_name.into(),
        category,
        acquisition_mode: AcquisitionMode::Browsable,
        downloader_kind: DownloaderKind::Api,
        parameters: vec![param_date_range()],
    }
}

fn param_date_range() -> ReportParameter {
    ReportParameter {
        id: "date_range".into(),
        label: "Период (с..по)".into(),
        kind: ReportParameterKind::DateRange,
        required: false,
        default: None,
    }
}

// =========================================================================
// Фабрика отчётов
// =========================================================================

/// HTTP-метод для запроса.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HttpMethod {
    Get,
    Post,
}

/// Способ извлечения массива из ответа WB.
/// WB использует разные обёртки в зависимости от эндпоинта.
#[derive(Clone, Copy)]
pub(crate) enum ResponseShape {
    /// Прямой массив `[...]` (orders, sales).
    Array,
    /// `{data: [...]}` (старый формат).
    DataArray,
    /// `{data: {reports: [...], total: N}}` (deductions, measurement-penalties).
    DataReports,
    /// `{data: {documents: [...]}}` (documents list — обрабатывается отдельно).
    DataDocuments,
    /// `{details: [...]}` (antifraud).
    Details,
    /// `{report: [...]}` (region-sale, goods-return, goods-labeling).
    Report,
}

/// Фабрика отчётов: возвращает `ReportRef` по `type_id`.
/// Все пути/методы/форматы сверенs с официальной документацией WB.
pub fn make_report(type_id: &str, client: WbHttpClient) -> CoreResult<ReportRef> {
    let report: ReportRef = match type_id {
        // === Баланс === GET finance-api /api/v1/account/balance
        "wb.balance" => Arc::new(WbReport::new_get(
            "wb.balance",
            "Баланс продавца",
            ReportCategory::Finance,
            AcquisitionMode::Period,
            WbDomain::Finance,
            "/api/v1/account/balance",
            ResponseShape::DataArray,
            client,
        )),

        // === Финансы === POST finance-api
        "wb.sales_reports_list" => Arc::new(WbReport::new_post(
            "wb.sales_reports_list",
            "Реестр реализации (список)",
            ReportCategory::Finance,
            AcquisitionMode::Period,
            WbDomain::Finance,
            "/api/finance/v1/sales-reports/list",
            ResponseShape::Array,
            client,
        )),
        "wb.sales_reports_detailed" => Arc::new(WbReport::new_post(
            "wb.sales_reports_detailed",
            "Детализация реализации",
            ReportCategory::Finance,
            AcquisitionMode::Period,
            WbDomain::Finance,
            "/api/finance/v1/sales-reports/detailed",
            ResponseShape::Array,
            client,
        )),
        "wb.acquiring_list" => Arc::new(WbReport::new_post(
            "wb.acquiring_list",
            "Эквайринг (список)",
            ReportCategory::Finance,
            AcquisitionMode::Period,
            WbDomain::Finance,
            "/api/finance/v1/acquiring/list",
            ResponseShape::Array,
            client,
        )),
        "wb.acquiring_detailed" => Arc::new(WbReport::new_post(
            "wb.acquiring_detailed",
            "Эквайринг (детализация)",
            ReportCategory::Finance,
            AcquisitionMode::Period,
            WbDomain::Finance,
            "/api/finance/v1/acquiring/detailed",
            ResponseShape::Array,
            client,
        )),

        // === Документы === (отдельные реализации, documents-api GET)
        "wb.documents" => Arc::new(WbDocumentsReport::new(client)),
        "wb.documents_categories" => Arc::new(WbCategoriesReport::new(client)),

        // === Статистика === GET statistics-api, ответ — прямой массив
        "wb.orders" => Arc::new(WbReport::new_get(
            "wb.orders",
            "Заказы",
            ReportCategory::Operational,
            AcquisitionMode::Browsable,
            WbDomain::Statistics,
            "/api/v1/supplier/orders",
            ResponseShape::Array,
            client,
        )),
        "wb.sales" => Arc::new(WbReport::new_get(
            "wb.sales",
            "Продажи",
            ReportCategory::Operational,
            AcquisitionMode::Browsable,
            WbDomain::Statistics,
            "/api/v1/supplier/sales",
            ResponseShape::Array,
            client,
        )),

        // === Удержания === GET seller-analytics-api
        "wb.deductions" => Arc::new(WbReport::new_get(
            "wb.deductions",
            "Штрафы за подмены",
            ReportCategory::Penalties,
            AcquisitionMode::Browsable,
            WbDomain::Analytics,
            "/api/analytics/v1/deductions",
            ResponseShape::DataReports,
            client,
        )),
        "wb.measurement_penalties" => Arc::new(WbReport::new_get(
            "wb.measurement_penalties",
            "Штрафы за габариты",
            ReportCategory::Penalties,
            AcquisitionMode::Browsable,
            WbDomain::Analytics,
            "/api/analytics/v1/measurement-penalties",
            ResponseShape::DataReports,
            client,
        )),
        "wb.antifraud" => Arc::new(WbReport::new_get(
            "wb.antifraud",
            "Самовыкупы (антифрод)",
            ReportCategory::Penalties,
            AcquisitionMode::Browsable,
            WbDomain::Analytics,
            "/api/v1/analytics/antifraud-details",
            ResponseShape::Details,
            client,
        )),

        // === Возвраты === спека §2.2.2: GET /api/v1/claims
        "wb.claims" => Arc::new(WbReport::new_get(
            "wb.claims",
            "Возвраты (claims)",
            ReportCategory::Returns,
            AcquisitionMode::Browsable,
            WbDomain::Returns,
            "/api/v1/claims",
            ResponseShape::Array,
            client,
        )),

        // === Аналитический отчёт приёмки === спека §2.2.2: async
        "wb.acceptance_report" => Arc::new(WbReport::new_post(
            "wb.acceptance_report",
            "Аналитический отчёт приёмки (async)",
            ReportCategory::Finance,
            AcquisitionMode::Period,
            WbDomain::Analytics,
            "/api/v1/acceptance_report",
            ResponseShape::DataArray,
            client,
        )),

        _ => return Err(CoreError::ReportTypeNotSupported(type_id.to_string())),
    };
    Ok(report)
}

// =========================================================================
// WbReport — универсальный отчёт (GET или POST)
// =========================================================================

/// Универсальный отчёт WB с настраиваемым HTTP-методом и форматом ответа.
pub struct WbReport {
    type_id: String,
    display_name: String,
    category: ReportCategory,
    mode: AcquisitionMode,
    domain: WbDomain,
    path: &'static str,
    method: HttpMethod,
    shape: ResponseShape,
    client: WbHttpClient,
}

impl WbReport {
    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
    pub fn new_get(
        type_id: &str,
        display_name: &str,
        category: ReportCategory,
        mode: AcquisitionMode,
        domain: WbDomain,
        path: &'static str,
        shape: ResponseShape,
        client: WbHttpClient,
    ) -> Self {
        Self {
            type_id: type_id.into(),
            display_name: display_name.into(),
            category,
            mode,
            domain,
            path,
            method: HttpMethod::Get,
            shape,
            client,
        }
    }

    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
    pub fn new_post(
        type_id: &str,
        display_name: &str,
        category: ReportCategory,
        mode: AcquisitionMode,
        domain: WbDomain,
        path: &'static str,
        shape: ResponseShape,
        client: WbHttpClient,
    ) -> Self {
        Self {
            type_id: type_id.into(),
            display_name: display_name.into(),
            category,
            mode,
            domain,
            path,
            method: HttpMethod::Post,
            shape,
            client,
        }
    }
}

#[async_trait]
impl Report for WbReport {
    fn type_id(&self) -> &str {
        &self.type_id
    }
    fn display_name(&self) -> &str {
        &self.display_name
    }
    fn category(&self) -> ReportCategory {
        self.category.clone()
    }
    fn acquisition_mode(&self) -> AcquisitionMode {
        self.mode
    }
    fn downloader_kind(&self) -> DownloaderKind {
        DownloaderKind::Api
    }
    fn parameters(&self) -> &[ReportParameter] {
        &[]
    }

    async fn list(
        &self,
        auth: &dyn mdwf_core::Authenticator,
        filter: &DocumentFilter,
        _cancel: CancelToken,
    ) -> CoreResult<Vec<DocumentEntry>> {
        let json = self.fetch(auth, filter).await?;
        Ok(extract_entries(&json, self.shape, &self.type_id))
    }

    async fn download(
        &self,
        auth: &dyn mdwf_core::Authenticator,
        params: &ReportParams,
        _progress: ProgressCallbackRef,
        _cancel: CancelToken,
    ) -> CoreResult<Vec<DownloadedFile>> {
        // Для простых отчётов — тот же запрос, результат сохраняем как JSON.
        let filter = filter_from_params(params);
        let json = self.fetch(auth, &filter).await?;
        let content = serde_json::to_vec_pretty(&json)
            .map_err(|e| CoreError::Internal(format!("serialize: {e}")))?;
        let period = params.period.clone().unwrap_or_else(|| "current".into());
        Ok(vec![DownloadedFile::with_content(
            format!("{}_{}.json", self.type_id, period),
            "json",
            content,
        )])
    }
}

impl WbReport {
    /// Выполняет запрос (GET или POST) в зависимости от метода.
    async fn fetch(
        &self,
        auth: &dyn mdwf_core::Authenticator,
        filter: &DocumentFilter,
    ) -> CoreResult<serde_json::Value> {
        match self.method {
            HttpMethod::Get => {
                let query = build_query_from_filter(filter);
                let query_ref: Vec<(&str, &str)> =
                    query.iter().map(|(k, v)| (*k, v.as_str())).collect();
                self.client
                    .get(self.domain, self.path, &query_ref, auth)
                    .await
            }
            HttpMethod::Post => {
                let body = build_body_from_filter(filter);
                self.client
                    .post(self.domain, self.path, &body, auth)
                    .await
            }
        }
    }
}

/// Извлекает DocumentEntry из ответа согласно формату (ResponseShape).
fn extract_entries(
    json: &serde_json::Value,
    shape: ResponseShape,
    type_id: &str,
) -> Vec<DocumentEntry> {
    let array = match shape {
        ResponseShape::Array => json, // orders/sales: прямой массив [...]
        ResponseShape::DataArray => json.get("data").unwrap_or(json),
        ResponseShape::DataReports => json
            .get("data")
            .and_then(|d| d.get("reports"))
            .unwrap_or(json),
        ResponseShape::DataDocuments => json
            .get("data")
            .and_then(|d| d.get("documents"))
            .unwrap_or(json),
        ResponseShape::Details => json.get("details").unwrap_or(json),
        ResponseShape::Report => json.get("report").unwrap_or(json),
    };

    let mut out = Vec::new();
    if let Some(arr) = array.as_array() {
        for (i, item) in arr.iter().enumerate() {
            // Универсальный ID: пробуем srid, orderId, nmId, reportId, rrdId, id.
            let id = item
                .get("srid")
                .or_else(|| item.get("orderId"))
                .or_else(|| item.get("nmId"))
                .or_else(|| item.get("reportId"))
                .or_else(|| item.get("rrdId"))
                .or_else(|| item.get("id"))
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| v.to_string())
                })
                .unwrap_or_else(|| format!("item-{i}"));
            let display = item
                .get("supplierArticle")
                .or_else(|| item.get("subject"))
                .or_else(|| item.get("category"))
                .or_else(|| item.get("brand"))
                .or_else(|| item.get("docTypeName"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("{type_id} #{i}"));
            let mut e = DocumentEntry::new(id, display);
            e.category = type_id.to_string();
            e.extensions = vec!["json".into()];
            // Дата: пробуем date, saleDt, orderDt, rrDate, createDate.
            for date_field in ["date", "saleDt", "orderDt", "rrDate", "createDate"] {
                if let Some(d) = item.get(date_field).and_then(|v| v.as_str()) {
                    e.date = parse_flexible_date(d);
                    if e.date.is_some() {
                        break;
                    }
                }
            }
            out.push(e);
        }
    }
    out
}

/// Парсит дату в нескольких форматах (YYYY-MM-DD, YYYY-MM-DDTHH:MM:SS, YYYY-MM-DDTHH:MM:SSZ).
fn parse_flexible_date(s: &str) -> Option<chrono::NaiveDate> {
    // Сначала пробуем полный datetime, потом дату.
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.date_naive());
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(dt.date());
    }
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

// =========================================================================
// Хелперы построения query/body
// =========================================================================

/// Строит query-параметры для GET-запросов из фильтра.
fn build_query_from_filter(filter: &DocumentFilter) -> Vec<(&'static str, String)> {
    let mut q: Vec<(&'static str, String)> = Vec::new();
    if let Some(d) = filter.date_from {
        q.push(("dateFrom", date_format::format_date_moscow(d)));
    }
    if let Some(d) = filter.date_to {
        q.push(("dateTo", date_format::format_date_moscow(d)));
    }
    if let Some(limit) = filter.limit {
        q.push(("limit", limit.to_string()));
    }
    q
}

/// Строит JSON-тело для POST-запросов (финансовые отчёты) из фильтра.
/// Дока: {dateFrom, dateTo, limit, offset, period}.
fn build_body_from_filter(filter: &DocumentFilter) -> serde_json::Value {
    let mut body = json!({
        "limit": filter.limit.unwrap_or(1000),
        "offset": 0,
    });
    if let Some(d) = filter.date_from {
        body["dateFrom"] = json!(date_format::format_date_moscow(d));
    }
    if let Some(d) = filter.date_to {
        body["dateTo"] = json!(date_format::format_date_moscow(d));
    }
    body
}

/// Преобразует ReportParams в DocumentFilter (для download).
fn filter_from_params(params: &ReportParams) -> DocumentFilter {
    let mut f = DocumentFilter::default();
    if let Some(d) = params.get("date_from") {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d") {
            f.date_from = Some(date);
        }
    }
    if let Some(d) = params.get("date_to") {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d") {
            f.date_to = Some(date);
        }
    }
    if let Some(p) = &params.period {
        // YYYY-MM -> первый день месяца.
        if p.len() == 7 {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(&format!("{p}-01"), "%Y-%m-%d") {
                f.date_from = Some(date);
                f.date_to = Some(date + chrono::Duration::days(30));
            }
        }
    }
    f
}

// =========================================================================
// WbDocumentsReport — Documents API (3-шаговый, documents-api GET)
// =========================================================================

/// Отчёт Documents API (3-шаговый паттерн, дока раздел «Документы»).
pub struct WbDocumentsReport {
    client: WbHttpClient,
}

impl WbDocumentsReport {
    #[must_use]
    pub fn new(client: WbHttpClient) -> Self {
        Self { client }
    }
}

/// Размер страницы для пагинации /documents/list (WB-максимум за запрос).
const WB_DOCS_PAGE_SIZE: u32 = 50;
/// Страховочный потолок числа страниц, чтобы не зациклиться при аномальном
/// ответе WB (напр. постоянно возвращает полные 50, но данные дублируются).
/// 200 страниц × 50 = 10 000 документов — заведомо больше любого реального случая.
const WB_DOCS_MAX_PAGES: u32 = 200;

#[async_trait]
impl Report for WbDocumentsReport {
    fn type_id(&self) -> &str {
        "wb.documents"
    }
    fn display_name(&self) -> &str {
        "Документы (УПД/УКД/акты) — по категории"
    }
    fn category(&self) -> ReportCategory {
        ReportCategory::Documents
    }
    fn acquisition_mode(&self) -> AcquisitionMode {
        AcquisitionMode::Browsable
    }
    fn downloader_kind(&self) -> DownloaderKind {
        DownloaderKind::Api
    }
    fn parameters(&self) -> &[ReportParameter] {
        &[]
    }

    async fn list(
        &self,
        auth: &dyn mdwf_core::Authenticator,
        filter: &DocumentFilter,
        _cancel: CancelToken,
    ) -> CoreResult<Vec<DocumentEntry>> {
        let docs_client = crate::documents::DocumentsClient::new(&self.client);
        // Категория опциональна: если не указана, WB вернёт документы всех категорий.
        let category = filter.category.as_deref().unwrap_or("");

        // Проверяем категорию только если она указана.
        if !category.is_empty() {
            docs_client.ensure_category(auth, category).await?;
        }

        // Пагинация: WB /documents/list отдаёт максимум 50 документов за запрос
        // (поля total в ответе нет — дока/спека GetListData содержит только documents).
        // Поэтому перебираем страницы по PAGE_SIZE, пока не получим неполную страницу
        // (признак конца) или не наберём ceiling = filter.limit (None = без потолка,
        // выгружаем все). Запросы идут через per-domain rate-limiter (1 req/10с burst 5).
        let ceiling = filter.limit; // None = без ограничения.

        let mut all: Vec<crate::documents::WbDocument> = Vec::new();
        let mut offset: u32 = 0;
        for _ in 0..WB_DOCS_MAX_PAGES {
            // Если уже набрали ceiling — стоп.
            if let Some(max) = ceiling {
                if all.len() as u32 >= max {
                    break;
                }
            }
            let params = crate::documents::ListDocumentsParams {
                category: if category.is_empty() {
                    None
                } else {
                    Some(category.to_string())
                },
                date_from: filter.date_from,
                date_to: filter.date_to,
                limit: WB_DOCS_PAGE_SIZE,
                offset,
                ..Default::default()
            };
            let page = docs_client.list_documents(auth, &params).await?;
            let got = page.len();
            all.extend(page);
            // Неполная страница — это последняя (дока не возвращает total).
            if (got as u32) < WB_DOCS_PAGE_SIZE {
                break;
            }
            offset = offset.saturating_add(WB_DOCS_PAGE_SIZE);
        }
        // Обрезаем по ceiling, если набрали больше (последняя страница могла дать излишек).
        if let Some(max) = ceiling {
            all.truncate(max as usize);
        }

        Ok(all
            .iter()
            .map(|d| {
                // Используем категорию из самого документа, если есть.
                let cat = d.category.as_deref().unwrap_or(category);
                crate::documents::wb_document_to_entry(d, cat)
            })
            .collect())
    }

    async fn download(
        &self,
        auth: &dyn mdwf_core::Authenticator,
        params: &ReportParams,
        _progress: ProgressCallbackRef,
        _cancel: CancelToken,
    ) -> CoreResult<Vec<DownloadedFile>> {
        let ids_csv = params
            .get("ids")
            .ok_or_else(|| CoreError::InvalidParameter("wb.documents.download: требуется ids".into()))?;
        let ids: Vec<&str> = ids_csv.split(',').filter(|s| !s.is_empty()).collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        // Мета документов (id → name, extension), переданная из UI через
        // params.values["doc_meta"] (JSON-массив). Позволяет использовать
        // человекочитаемое имя как имя файла и предпочтительный формат.
        let meta: std::collections::HashMap<String, DocMeta> = params
            .get("doc_meta")
            .and_then(|json| serde_json::from_str::<Vec<DocMeta>>(json).ok())
            .map(|v| v.into_iter().map(|m| (m.id.clone(), m)).collect())
            .unwrap_or_default();

        let docs_client = crate::documents::DocumentsClient::new(&self.client);
        let mut files = Vec::new();
        for id in ids {
            // Предпочтительный формат: из меты UI, иначе "zip".
            let requested_ext = meta.get(id).and_then(|m| m.extension.as_deref())
                .filter(|s| !s.is_empty())
                .unwrap_or("zip");
            let downloaded = docs_client.download_one(auth, id, requested_ext).await?;
            // source_id = человекочитаемое имя (поле name из /list), если есть —
            // оно станет базовым именем файла на диске ({doc_id}).
            let source_id = meta.get(id).and_then(|m| m.name.clone());
            let mut f = DownloadedFile::with_content(
                // file_name здесь не важен — FileStore всё равно пересоберёт имя
                // из шаблона; оставим serviceName для отладки.
                format!("wb_doc_{id}"),
                downloaded.extension,
                downloaded.bytes,
            );
            f.source_id = source_id;
            files.push(f);
        }
        Ok(files)
    }
}

/// Метаданные выбранного документа из UI (десериализуются из params `doc_meta`).
#[derive(serde::Deserialize)]
struct DocMeta {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    extension: Option<String>,
}

// =========================================================================
// WbCategoriesReport — список категорий документов
// =========================================================================

pub struct WbCategoriesReport {
    client: WbHttpClient,
}

impl WbCategoriesReport {
    #[must_use]
    pub fn new(client: WbHttpClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Report for WbCategoriesReport {
    fn type_id(&self) -> &str {
        "wb.documents_categories"
    }
    fn display_name(&self) -> &str {
        "Категории документов"
    }
    fn category(&self) -> ReportCategory {
        ReportCategory::Documents
    }
    fn acquisition_mode(&self) -> AcquisitionMode {
        AcquisitionMode::Browsable
    }
    fn downloader_kind(&self) -> DownloaderKind {
        DownloaderKind::Api
    }
    fn parameters(&self) -> &[ReportParameter] {
        &[]
    }

    async fn list(
        &self,
        auth: &dyn mdwf_core::Authenticator,
        _filter: &DocumentFilter,
        _cancel: CancelToken,
    ) -> CoreResult<Vec<DocumentEntry>> {
        let docs_client = crate::documents::DocumentsClient::new(&self.client);
        let cats = docs_client.list_categories(auth).await?;
        Ok(cats
            .iter()
            .map(|c| {
                // display_name = человекочитаемое название (title), иначе name.
                let label = c.title.clone().unwrap_or_else(|| c.name.clone());
                let mut e = DocumentEntry::new(c.name.clone(), label);
                e.category = c.name.clone();
                e
            })
            .collect())
    }

    async fn download(
        &self,
        _auth: &dyn mdwf_core::Authenticator,
        _params: &ReportParams,
        _progress: ProgressCallbackRef,
        _cancel: CancelToken,
    ) -> CoreResult<Vec<DownloadedFile>> {
        Err(CoreError::InvalidParameter(
            "categories report не поддерживает download".into(),
        ))
    }
}

// =========================================================================
// Out-of-scope
// =========================================================================

/// Документы WB, недоступные через API (out-of-scope, дока не содержит методов).
#[must_use]
pub fn out_of_scope() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Акты сверки", "Нет API. Запросите в личном кабинете WB."),
        ("Счета на оплату", "Нет API. Скачайте вручную."),
        ("Договоры", "Нет API. Обратитесь к менеджеру WB."),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_have_documents_and_finance() {
        let caps = capabilities();
        assert!(caps.reports.iter().any(|r| r.type_id == "wb.documents"));
        assert!(caps.reports.iter().any(|r| r.type_id == "wb.balance"));
        assert!(caps.reports.iter().any(|r| r.type_id == "wb.sales_reports_list"));
    }

    #[test]
    fn auth_field_is_token() {
        let caps = capabilities();
        assert_eq!(caps.auth_fields.len(), 1);
        assert_eq!(caps.auth_fields[0].id, "token");
        assert!(caps.auth_fields[0].secret);
    }

    #[test]
    fn out_of_scope_has_3() {
        assert_eq!(out_of_scope().len(), 3);
    }

    #[test]
    fn documents_descriptor_is_browsable() {
        let caps = capabilities();
        let docs = caps
            .reports
            .iter()
            .find(|r| r.type_id == "wb.documents")
            .unwrap();
        assert_eq!(docs.acquisition_mode, AcquisitionMode::Browsable);
    }

    #[test]
    fn finance_reports_are_period() {
        let caps = capabilities();
        for rid in [
            "wb.sales_reports_list",
            "wb.acquiring_list",
            "wb.balance",
        ] {
            let r = caps.reports.iter().find(|r| r.type_id == rid).unwrap();
            assert_eq!(r.acquisition_mode, AcquisitionMode::Period, "{rid}");
        }
    }

    #[test]
    fn extract_entries_array_shape() {
        let json = json!([
            {"srid": "S1", "supplierArticle": "A1", "date": "2026-07-01T10:00:00"},
            {"srid": "S2", "supplierArticle": "A2", "date": "2026-07-02"}
        ]);
        let entries = extract_entries(&json, ResponseShape::Array, "wb.orders");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "S1");
        assert_eq!(entries[1].display_name, "A2");
    }

    #[test]
    fn extract_entries_data_reports_shape() {
        let json = json!({
            "data": {"reports": [{"nmId": 123, "brand": "X"}], "total": 1}
        });
        let entries = extract_entries(&json, ResponseShape::DataReports, "wb.deductions");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "123");
    }
}
