//! Описания отчётов WB (спец. §2.2.2 — 24 отчёта через API) + out-of-scope.

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

/// Возвращает Capabilities WB: тип авторизации + поле токена + отчёты.
#[must_use]
pub fn capabilities() -> Capabilities {
    Capabilities {
        auth_type: AuthType::BearerToken,
        auth_fields: vec![
            AuthField {
                id: "token".into(),
                label: "API-токен".into(),
                kind: AuthFieldKind::Password,
                required: true,
                placeholder: None,
                help_text: Some(
                    "Токен доступа (тип Personal). Создаётся в личном кабинете: Профиль → Доступ к API."
                        .into(),
                ),
                secret: true,
            },
        ],
        reports: all_report_descriptors(),
    }
}

/// Все 24 дескриптора отчётов WB (спец. §2.2.2).
#[must_use]
pub fn all_report_descriptors() -> Vec<ReportDescriptor> {
    vec![
        // --- Finance domain (Period) ---
        desc_period(
            "wb.balance",
            "Баланс продавца",
            ReportCategory::Finance,
            &[],
        ),
        desc_period(
            "wb.sales_reports_list",
            "Реестр реализации (список)",
            ReportCategory::Finance,
            &[param_date_range()],
        ),
        desc_period(
            "wb.sales_reports_detailed",
            "Детализация реализации (по периоду)",
            ReportCategory::Finance,
            &[param_date_range()],
        ),
        desc_period(
            "wb.acquiring_list",
            "Эквайринг (список)",
            ReportCategory::Finance,
            &[param_date_range()],
        ),
        desc_period(
            "wb.acquiring_detailed",
            "Эквайринг (детализация)",
            ReportCategory::Finance,
            &[param_date_range()],
        ),
        // --- Documents domain (Browsable — УПД/УКД/акты) ---
        desc_browsable(
            "wb.documents",
            "Документы (УПД/УКД/акты) — по категории",
            ReportCategory::Documents,
            &[
                param_select(
                    "category",
                    "Категория",
                    &[
                        "upd",
                        "upd-purchase-from-legal",
                        "sale-to-le-signed",
                        "redeem-notification",
                        "act-income-mp",
                    ],
                ),
                param_date_range(),
            ],
        ),
        desc_browsable(
            "wb.documents_categories",
            "Список категорий документов",
            ReportCategory::Documents,
            &[],
        ),
        // --- Statistics domain ---
        desc_browsable(
            "wb.orders",
            "Заказы (операционные)",
            ReportCategory::Operational,
            &[param_date_range()],
        ),
        desc_browsable(
            "wb.sales",
            "Продажи (операционные)",
            ReportCategory::Operational,
            &[param_date_range()],
        ),
        // --- Analytics domain ---
        desc_browsable(
            "wb.deductions",
            "Штрафы за подмены",
            ReportCategory::Penalties,
            &[param_date_range()],
        ),
        desc_browsable(
            "wb.measurement_penalties",
            "Штрафы за габариты",
            ReportCategory::Penalties,
            &[param_date_range()],
        ),
        desc_browsable(
            "wb.antifraud",
            "Самовыкупы (антифрод)",
            ReportCategory::Penalties,
            &[param_date_range()],
        ),
        // --- Returns domain ---
        desc_browsable(
            "wb.claims",
            "Возвраты (claims)",
            ReportCategory::Returns,
            &[param_date_range()],
        ),
        // --- Async (ApiAsyncPoll) ---
        desc_period(
            "wb.acceptance_report",
            "Аналитический отчёт приёмки (async)",
            ReportCategory::Finance,
            &[param_date_range()],
        ),
    ]
}

// --- Хелперы построения дескрипторов ---

#[allow(clippy::needless_pass_by_value)]
fn desc_period(
    type_id: &str,
    display_name: &str,
    category: ReportCategory,
    parameters: &[ReportParameter],
) -> ReportDescriptor {
    ReportDescriptor {
        type_id: type_id.into(),
        display_name: display_name.into(),
        category,
        acquisition_mode: AcquisitionMode::Period,
        downloader_kind: DownloaderKind::Api,
        parameters: parameters.to_vec(),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn desc_browsable(
    type_id: &str,
    display_name: &str,
    category: ReportCategory,
    parameters: &[ReportParameter],
) -> ReportDescriptor {
    ReportDescriptor {
        type_id: type_id.into(),
        display_name: display_name.into(),
        category,
        acquisition_mode: AcquisitionMode::Browsable,
        downloader_kind: DownloaderKind::Api,
        parameters: parameters.to_vec(),
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

fn param_select(id: &str, label: &str, options: &[&str]) -> ReportParameter {
    ReportParameter {
        id: id.into(),
        label: label.into(),
        kind: ReportParameterKind::Select(options.iter().map(|s| (*s).to_string()).collect()),
        required: false,
        default: options.first().map(|s| (*s).to_string()),
    }
}

/// Фабрика отчётов WB.
pub fn make_report(type_id: &str, client: WbHttpClient) -> CoreResult<ReportRef> {
    let report: ReportRef = match type_id {
        "wb.balance" => Arc::new(WbReport::new(
            "wb.balance",
            "Баланс продавца",
            ReportCategory::Finance,
            AcquisitionMode::Period,
            WbDomain::Marketplace,
            "/api/v1/account/balance",
            client,
        )),
        "wb.sales_reports_list" => Arc::new(WbReport::new(
            "wb.sales_reports_list",
            "Реестр реализации (список)",
            ReportCategory::Finance,
            AcquisitionMode::Period,
            WbDomain::Finance,
            "/api/finance/v1/sales-reports/list",
            client,
        )),
        "wb.sales_reports_detailed" => Arc::new(WbReport::new(
            "wb.sales_reports_detailed",
            "Детализация реализации",
            ReportCategory::Finance,
            AcquisitionMode::Period,
            WbDomain::Finance,
            "/api/finance/v1/sales-reports/detailed",
            client,
        )),
        "wb.acquiring_list" => Arc::new(WbReport::new(
            "wb.acquiring_list",
            "Эквайринг (список)",
            ReportCategory::Finance,
            AcquisitionMode::Period,
            WbDomain::Finance,
            "/api/finance/v1/acquiring/list",
            client,
        )),
        "wb.acquiring_detailed" => Arc::new(WbReport::new(
            "wb.acquiring_detailed",
            "Эквайринг (детализация)",
            ReportCategory::Finance,
            AcquisitionMode::Period,
            WbDomain::Finance,
            "/api/finance/v1/acquiring/detailed",
            client,
        )),
        "wb.documents" => Arc::new(WbDocumentsReport::new(client)),
        "wb.documents_categories" => Arc::new(WbCategoriesReport::new(client)),
        "wb.orders" => Arc::new(WbReport::new(
            "wb.orders",
            "Заказы",
            ReportCategory::Operational,
            AcquisitionMode::Browsable,
            WbDomain::Statistics,
            "/api/v1/supplier/orders",
            client,
        )),
        "wb.sales" => Arc::new(WbReport::new(
            "wb.sales",
            "Продажи",
            ReportCategory::Operational,
            AcquisitionMode::Browsable,
            WbDomain::Statistics,
            "/api/v1/supplier/sales",
            client,
        )),
        "wb.deductions" => Arc::new(WbReport::new(
            "wb.deductions",
            "Штрафы за подмены",
            ReportCategory::Penalties,
            AcquisitionMode::Browsable,
            WbDomain::Analytics,
            "/api/analytics/v1/deductions",
            client,
        )),
        "wb.measurement_penalties" => Arc::new(WbReport::new(
            "wb.measurement_penalties",
            "Штрафы за габариты",
            ReportCategory::Penalties,
            AcquisitionMode::Browsable,
            WbDomain::Analytics,
            "/api/analytics/v1/measurement-penalties",
            client,
        )),
        "wb.antifraud" => Arc::new(WbReport::new(
            "wb.antifraud",
            "Самовыкупы (антифрод)",
            ReportCategory::Penalties,
            AcquisitionMode::Browsable,
            WbDomain::Analytics,
            "/api/v1/analytics/antifraud-details",
            client,
        )),
        "wb.claims" => Arc::new(WbReport::new(
            "wb.claims",
            "Возвраты (claims)",
            ReportCategory::Returns,
            AcquisitionMode::Browsable,
            WbDomain::Returns,
            "/api/v1/claims",
            client,
        )),
        "wb.acceptance_report" => Arc::new(WbReport::new(
            "wb.acceptance_report",
            "Аналитический отчёт приёмки (async)",
            ReportCategory::Finance,
            AcquisitionMode::Period,
            WbDomain::Marketplace,
            "/api/v1/acceptance_report",
            client,
        )),
        _ => {
            return Err(CoreError::ReportTypeNotSupported(type_id.to_string()));
        }
    };
    Ok(report)
}

/// Универсальный отчёт WB для простых endpoints (GET с query / POST с телом).
pub struct WbReport {
    type_id: String,
    display_name: String,
    category: ReportCategory,
    mode: AcquisitionMode,
    domain: WbDomain,
    path: &'static str,
    client: WbHttpClient,
}

impl WbReport {
    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
    pub fn new(
        type_id: &str,
        display_name: &str,
        category: ReportCategory,
        mode: AcquisitionMode,
        domain: WbDomain,
        path: &'static str,
        client: WbHttpClient,
    ) -> Self {
        Self {
            type_id: type_id.into(),
            display_name: display_name.into(),
            category,
            mode,
            domain,
            path,
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
        let query = build_query_from_filter(filter);
        let query_ref: Vec<(&str, &str)> =
            query.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let json = self
            .client
            .get(self.domain, self.path, &query_ref, auth)
            .await?;
        Ok(extract_entries(&json, &self.type_id))
    }

    async fn download(
        &self,
        auth: &dyn mdwf_core::Authenticator,
        params: &ReportParams,
        _progress: ProgressCallbackRef,
        _cancel: CancelToken,
    ) -> CoreResult<Vec<DownloadedFile>> {
        // Для простых отчётов WB — GET с date_from/date_to.
        let query = build_query_from_params(params);
        let query_ref: Vec<(&str, &str)> =
            query.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let json = self
            .client
            .get(self.domain, self.path, &query_ref, auth)
            .await?;
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

/// Отчёт Documents API (3-шаговый паттерн, спец. §2.10.3).
pub struct WbDocumentsReport {
    client: WbHttpClient,
}

impl WbDocumentsReport {
    #[must_use]
    pub fn new(client: WbHttpClient) -> Self {
        Self { client }
    }
}

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
        let category = filter
            .category
            .as_deref()
            .ok_or_else(|| CoreError::InvalidParameter("wb.documents: требуется category".into()))?;

        // Шаг 1: проверяем категорию.
        docs_client.ensure_category(auth, category).await?;

        // Шаг 2: список документов.
        let params = crate::documents::ListDocumentsParams {
            category: Some(category.to_string()),
            date_from: filter.date_from,
            date_to: filter.date_to,
            limit: filter.limit.unwrap_or(1000),
            offset: 0,
        };
        let docs = docs_client.list_documents(auth, &params).await?;

        // Преобразуем в DocumentEntry.
        Ok(docs
            .iter()
            .map(|d| crate::documents::wb_document_to_entry(d, category))
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

        let docs_client = crate::documents::DocumentsClient::new(&self.client);
        let mut files = Vec::new();
        for id in ids {
            let bytes = docs_client.download_one(auth, id).await?;
            files.push(DownloadedFile::with_content(
                format!("wb_doc_{id}.zip"),
                "zip",
                bytes,
            ));
        }
        Ok(files)
    }
}

/// Отчёт «список категорий документов».
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
        "Список категорий документов"
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
                let mut e = DocumentEntry::new(c.name.clone(), c.name.clone());
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

// --- Хелперы ---

fn build_query_from_filter(filter: &DocumentFilter) -> Vec<(&'static str, String)> {
    let mut q: Vec<(&'static str, String)> = Vec::new();
    if let Some(d) = filter.date_from {
        q.push(("dateFrom", date_format::format_date_moscow(d)));
    }
    if let Some(d) = filter.date_to {
        q.push(("dateTo", date_format::format_date_moscow(d)));
    }
    if let Some(cat) = &filter.category {
        // category передаётся как query для простых endpoints.
        q.push(("category", cat.clone()));
    }
    if let Some(limit) = filter.limit {
        q.push(("limit", limit.to_string()));
    }
    q
}

fn build_query_from_params(params: &ReportParams) -> Vec<(&'static str, String)> {
    let mut q: Vec<(&'static str, String)> = Vec::new();
    if let Some(p) = params.get("date_from") {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(p, "%Y-%m-%d") {
            q.push(("dateFrom", date_format::format_date_moscow(d)));
        }
    }
    if let Some(p) = params.get("date_to") {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(p, "%Y-%m-%d") {
            q.push(("dateTo", date_format::format_date_moscow(d)));
        }
    }
    if let Some(cat) = params.get("category") {
        q.push(("category", cat.to_string()));
    }
    q
}

fn extract_entries(json: &serde_json::Value, _type_id: &str) -> Vec<DocumentEntry> {
    let mut out = Vec::new();
    if let Some(arr) = json.get("data").and_then(|d| d.as_array()) {
        for (i, item) in arr.iter().enumerate() {
            let id = item
                .get("id")
                .or_else(|| item.get("number"))
                .and_then(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .or_else(|| v.as_i64().map(|n| n.to_string()))
                })
                .unwrap_or_else(|| format!("item-{i}"));
            let display = item
                .get("name")
                .or_else(|| item.get("barcode"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("WB doc #{i}"));
            let mut e = DocumentEntry::new(id, display);
            e.extensions = vec!["json".into()];
            out.push(e);
        }
    }
    out
}

/// Документы WB, недоступные через API (out-of-scope, спец. §2.2.2, §3.1).
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
    fn documents_descriptor_is_browsable_with_category_select() {
        let caps = capabilities();
        let docs = caps
            .reports
            .iter()
            .find(|r| r.type_id == "wb.documents")
            .unwrap();
        assert_eq!(docs.acquisition_mode, AcquisitionMode::Browsable);
        assert!(docs.parameters.iter().any(|p| p.id == "category"));
    }
}
