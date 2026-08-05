//! Описания отчётов Ozon (спец. §2.2.1 — 19 отчётов через API) + out-of-scope.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::NaiveDate;
use serde_json::json;

use mdwf_core::{
    AcquisitionMode, Capabilities, CoreError, CoreResult, CancelToken, DocumentEntry,
    DocumentFilter, DownloadedFile, DownloaderKind, ProgressCallbackRef, Report,
    ReportCategory, ReportDescriptor, ReportParameter, ReportParameterKind, ReportParams, ReportRef,
};
use mdwf_core::capabilities::{AuthField, AuthFieldKind, AuthType};

use crate::client::OzonHttpClient;
use crate::date_format;

/// Возвращает Capabilities Ozon: тип авторизации + поля формы + 19 отчётов.
#[must_use]
pub fn capabilities() -> Capabilities {
    Capabilities {
        auth_type: AuthType::ApiKey,
        auth_fields: vec![
            AuthField {
                id: "client_id".into(),
                label: "Client-Id".into(),
                kind: AuthFieldKind::Number,
                required: true,
                placeholder: Some("1234567".into()),
                help_text: Some("Числовой идентификатор продавца из личного кабинета".into()),
                secret: false,
            },
            AuthField {
                id: "api_key".into(),
                label: "Api-Key".into(),
                kind: AuthFieldKind::Password,
                required: true,
                placeholder: None,
                help_text: Some(
                    "API-ключ (TTL 180 дней). Создаётся в личном кабинете: Настройки → API ключи."
                        .into(),
                ),
                secret: true,
            },
        ],
        reports: all_report_descriptors(),
    }
}

/// Описания всех 19 отчётов Ozon (спец. §2.2.1). «Справочник типов начислений»
/// удалён — это служебный метод, а не выгрузка для пользователя.
#[must_use]
pub fn all_report_descriptors() -> Vec<ReportDescriptor> {
    vec![
        // --- Финансовые отчёты (Period-режим) ---
        desc_period(
            "ozon.realization",
            "Отчёт о реализации (месячный)",
            ReportCategory::Finance,
            &[param_period_month("period", "Месяц (YYYY-MM)", true)],
        ),
        desc_period(
            "ozon.realization_by_day",
            "Отчёт о реализации (за день)",
            ReportCategory::Finance,
            &[param_date("date", "Дата (YYYY-MM-DD)", true)],
        ),
        desc_period(
            "ozon.realization_posting",
            "Отчёт о реализации (позаказный)",
            ReportCategory::Finance,
            &[param_date_range(true)],
        ),
        desc_period(
            "ozon.buyout",
            "Выкупы маркетплейсом (ЕАЭС)",
            ReportCategory::Finance,
            &[param_date_range(true)],
        ),
        desc_period(
            "ozon.balance",
            "Баланс",
            ReportCategory::Finance,
            &[],
        ),
        desc_period(
            "ozon.cash_flow",
            "Финансовый отчёт (ДДС)",
            ReportCategory::Finance,
            &[param_date_range(true)],
        ),
        desc_period(
            "ozon.act_discrepancy",
            "Акт о расхождениях FBS (PDF)",
            ReportCategory::Documents,
            &[param_date_range(true)],
        ),
        desc_period(
            "ozon.analytics",
            "Аналитика (Premium Plus)",
            ReportCategory::Analytics,
            &[param_date_range(true)],
        ),
        // --- Browsable-режим (список → выбор → скачать) ---
        desc_browsable(
            "ozon.transaction_list",
            "Список транзакций (⚠️ deprecated → 6 июля 2026)",
            ReportCategory::Finance,
            &[param_date_range(true)],
        ),
        desc_browsable(
            "ozon.transaction_totals",
            "Итоги транзакций (⚠️ deprecated)",
            ReportCategory::Finance,
            &[param_date_range(true)],
        ),
        desc_browsable(
            "ozon.accrual_postings",
            "Начисления по отправлениям",
            ReportCategory::Finance,
            &[param_date_range(true)],
        ),
        desc_browsable(
            "ozon.accrual_by_day",
            "Начисления по дням",
            ReportCategory::Finance,
            &[param_date_range(true)],
        ),
        desc_period(
            "ozon.compensation",
            "Компенсации",
            ReportCategory::Finance,
            &[param_date_range(true)],
        ),
        desc_period(
            "ozon.decompensation",
            "Декомпенсации (штрафы/антифрод)",
            ReportCategory::Penalties,
            &[param_date_range(true)],
        ),
        desc_period(
            "ozon.b2b_sales",
            "Продажи юрлицам (PDF)",
            ReportCategory::Documents,
            &[param_date_range(true)],
        ),
        desc_browsable(
            "ozon.b2b_sales_json",
            "Продажи юрлицам (JSON)",
            ReportCategory::Documents,
            &[param_date_range(true)],
        ),
        desc_period(
            "ozon.mutual_settlement",
            "Отчёт о взаиморасчётах",
            ReportCategory::Finance,
            &[param_date_range(true)],
        ),
    ]
}

// --- Хелперы для построения дескрипторов ---

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

fn param_period_month(id: &str, label: &str, required: bool) -> ReportParameter {
    ReportParameter {
        id: id.into(),
        label: label.into(),
        kind: ReportParameterKind::YearMonth,
        required,
        default: Some(date_format::format_year_month(2026, 1)),
    }
}

fn param_date(id: &str, label: &str, required: bool) -> ReportParameter {
    ReportParameter {
        id: id.into(),
        label: label.into(),
        kind: ReportParameterKind::Date,
        required,
        default: Some("2026-07-01".into()),
    }
}

fn param_date_range(required: bool) -> ReportParameter {
    ReportParameter {
        id: "date_range".into(),
        label: "Период (с..по)".into(),
        kind: ReportParameterKind::DateRange,
        required,
        default: None,
    }
}

/// Фабрика отчётов: возвращает `ReportRef` по `type_id`.
///
/// Каждая реализация делегирует HTTP-вызовы в `OzonHttpClient`. Парсинг
/// специфичных полей отчётов будет уточняться по мере интеграции с реальным API.
pub fn make_report(type_id: &str, client: OzonHttpClient) -> CoreResult<ReportRef> {
    let report: ReportRef = match type_id {
        "ozon.realization" => Arc::new(OzonReport::period(
            "ozon.realization",
            "Отчёт о реализации (месячный)",
            ReportCategory::Finance,
            client,
            "/v2/finance/realization",
        )),
        "ozon.realization_by_day" => Arc::new(OzonReport::period(
            "ozon.realization_by_day",
            "Отчёт о реализации (за день)",
            ReportCategory::Finance,
            client,
            "/v1/finance/realization/by-day",
        )),
        "ozon.realization_posting" => Arc::new(OzonReport::period(
            "ozon.realization_posting",
            "Отчёт о реализации (позаказный)",
            ReportCategory::Finance,
            client,
            "/v1/finance/realization/posting",
        )),
        "ozon.buyout" => Arc::new(OzonReport::period(
            "ozon.buyout",
            "Выкупы маркетплейсом (ЕАЭС)",
            ReportCategory::Finance,
            client,
            "/v1/finance/products/buyout",
        )),
        "ozon.balance" => Arc::new(OzonReport::period(
            "ozon.balance",
            "Баланс",
            ReportCategory::Finance,
            client,
            "/v1/finance/balance",
        )),
        "ozon.cash_flow" => Arc::new(OzonReport::period(
            "ozon.cash_flow",
            "Финансовый отчёт (ДДС)",
            ReportCategory::Finance,
            client,
            "/v1/finance/cash-flow-statement/list",
        )),
        "ozon.act_discrepancy" => Arc::new(OzonReport::period(
            "ozon.act_discrepancy",
            "Акт о расхождениях FBS (PDF)",
            ReportCategory::Documents,
            client,
            "/v1/carriage/act-discrepancy/pdf",
        )),
        "ozon.analytics" => Arc::new(OzonReport::period(
            "ozon.analytics",
            "Аналитика (Premium Plus)",
            ReportCategory::Analytics,
            client,
            "/v1/analytics/data",
        )),
        // Browsable-режим.
        "ozon.transaction_list" => Arc::new(OzonReport::browsable(
            "ozon.transaction_list",
            "Список транзакций (deprecated)",
            ReportCategory::Finance,
            client,
            "/v3/finance/transaction/list",
        )),
        "ozon.transaction_totals" => Arc::new(OzonReport::browsable(
            "ozon.transaction_totals",
            "Итоги транзакций (deprecated)",
            ReportCategory::Finance,
            client,
            "/v3/finance/transaction/totals",
        )),
        "ozon.accrual_postings" => Arc::new(OzonReport::browsable(
            "ozon.accrual_postings",
            "Начисления по отправлениям",
            ReportCategory::Finance,
            client,
            "/v1/finance/accrual/postings",
        )),
        "ozon.accrual_by_day" => Arc::new(OzonReport::browsable(
            "ozon.accrual_by_day",
            "Начисления по дням",
            ReportCategory::Finance,
            client,
            "/v1/finance/accrual/by-day",
        )),
        "ozon.compensation" => Arc::new(OzonAsyncReport::new(
            "ozon.compensation",
            "Компенсации",
            ReportCategory::Finance,
            client,
            "/v1/finance/compensation",
        )),
        "ozon.decompensation" => Arc::new(OzonAsyncReport::new(
            "ozon.decompensation",
            "Декомпенсации",
            ReportCategory::Penalties,
            client,
            "/v1/finance/decompensation",
        )),
        "ozon.b2b_sales" => Arc::new(OzonAsyncReport::new(
            "ozon.b2b_sales",
            "Продажи юрлицам (PDF)",
            ReportCategory::Documents,
            client,
            "/v1/finance/document-b2b-sales",
        )),
        "ozon.b2b_sales_json" => Arc::new(OzonReport::browsable(
            "ozon.b2b_sales_json",
            "Продажи юрлицам (JSON)",
            ReportCategory::Documents,
            client,
            "/v1/finance/document-b2b-sales/json",
        )),
        "ozon.mutual_settlement" => Arc::new(OzonAsyncReport::new(
            "ozon.mutual_settlement",
            "Отчёт о взаиморасчётах",
            ReportCategory::Finance,
            client,
            "/v1/finance/mutual-settlement",
        )),
        _ => {
            return Err(CoreError::ReportTypeNotSupported(type_id.to_string()));
        }
    };
    Ok(report)
}

/// Универсальная реализация отчёта Ozon: Period или Browsable.
///
/// Для Period: POST с телом из params → возвращает JSON как один файл.
/// Для Browsable: `list` POST-ит эндпоинт и строит DocumentEntry из массива `result`;
///   `download` POST-ит с фильтром по `ids` и возвращает выбранные.
pub struct OzonReport {
    type_id: String,
    display_name: String,
    category: ReportCategory,
    mode: AcquisitionMode,
    client: OzonHttpClient,
    endpoint: &'static str,
}

impl OzonReport {
    #[must_use]
    pub fn period(
        type_id: &str,
        display_name: &str,
        category: ReportCategory,
        client: OzonHttpClient,
        endpoint: &'static str,
    ) -> Self {
        Self {
            type_id: type_id.into(),
            display_name: display_name.into(),
            category,
            mode: AcquisitionMode::Period,
            client,
            endpoint,
        }
    }

    #[must_use]
    pub fn browsable(
        type_id: &str,
        display_name: &str,
        category: ReportCategory,
        client: OzonHttpClient,
        endpoint: &'static str,
    ) -> Self {
        Self {
            type_id: type_id.into(),
            display_name: display_name.into(),
            category,
            mode: AcquisitionMode::Browsable,
            client,
            endpoint,
        }
    }
}

#[async_trait]
impl Report for OzonReport {
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
        _progress: ProgressCallbackRef,
        _cancel: CancelToken,
    ) -> CoreResult<Vec<DocumentEntry>> {
        let body = build_query_body(filter);
        let json = self.client.post(self.endpoint, &body, auth).await?;
        let entries = extract_entries(&json, &self.type_id);
        Ok(entries)
    }

    async fn download(
        &self,
        auth: &dyn mdwf_core::Authenticator,
        params: &ReportParams,
        _progress: ProgressCallbackRef,
        _cancel: CancelToken,
    ) -> CoreResult<Vec<DownloadedFile>> {
        let body = build_download_body(params);
        let json = self.client.post(self.endpoint, &body, auth).await?;
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

/// Строит тело запроса для Browsable-list из фильтра.
fn build_query_body(filter: &DocumentFilter) -> serde_json::Value {
    let mut body = json!({
        "limit": filter.limit.unwrap_or(1000),
        "offset": 0,
    });
    if let Some(from) = filter.date_from {
        body["date_from"] = json!(date_format::format_date_only(from));
    }
    if let Some(to) = filter.date_to {
        body["date_to"] = json!(date_format::format_date_only(to));
    }
    for (k, v) in &filter.extra {
        body[k.as_str()] = json!(v);
    }
    body
}

/// Строит тело запроса для download из параметров.
fn build_download_body(params: &ReportParams) -> serde_json::Value {
    let mut body = json!({});
    if let Some(p) = &params.period {
        // Месячный формат YYYY-MM.
        if p.len() == 7 {
            body["month"] = json!(p);
        } else if NaiveDate::parse_from_str(p, "%Y-%m-%d").is_ok() {
            body["date"] = json!(p);
        }
    }
    if let Some(ids) = params.get("ids") {
        body["ids"] = json!(ids.split(',').collect::<Vec<_>>());
    }
    for (k, v) in &params.values {
        if k != "ids" {
            body[k.as_str()] = json!(v);
        }
    }
    body
}

/// Извлекает DocumentEntry из типового ответа Ozon `{"result": {...}}`.
fn extract_entries(json: &serde_json::Value, type_id: &str) -> Vec<DocumentEntry> {
    // Ozon оборачивает данные в `result` (или `result.operations`, `result.rows`, ...).
    let result = json.get("result").unwrap_or(json);
    let array = find_array(result);
    let mut out = Vec::new();
    if let Some(arr) = array {
        for (i, item) in arr.as_array().unwrap_or(&vec![]).iter().enumerate() {
            let id = item
                .get("posting_number")
                .or_else(|| item.get("id"))
                .or_else(|| item.get("operation_id"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("item-{i}"));
            let display = item
                .get("posting_number")
                .or_else(|| item.get("name"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("{type_id} #{i}"));
            let mut e = DocumentEntry::new(id, display);
            e.category = type_id.to_string();
            e.extensions = vec!["json".into()];
            out.push(e);
        }
    }
    out
}

fn find_array(v: &serde_json::Value) -> Option<&serde_json::Value> {
    if v.is_array() {
        return Some(v);
    }
    if let Some(obj) = v.as_object() {
        for (_, val) in obj {
            if val.is_array() {
                return Some(val);
            }
        }
    }
    None
}
#[must_use]
pub fn out_of_scope() -> Vec<(&'static str, &'static str)> {
    vec![
        ("УПД с доп. услугами", "Нет API-эндпоинта. Скачайте вручную в личном кабинете."),
        ("Отчёты партнёров", "Нет API-эндпоинта. Обратитесь к партнёру."),
        ("Обеспечительные платежи", "Нет API-эндпоинта. Скачайте вручную."),
        ("Счета на оплату", "Нет API-эндпоинта. Скачайте вручную в личном кабинете."),
        ("Акты сверки", "Нет API-эндпоинта. Запросите у поддержки Ozon."),
    ]
}

// =========================================================================
// OzonAsyncReport — 2-шаговый паттерн для отчётов, возвращающих code.
//
// По доке Ozon:
// Шаг 1: POST /v1/finance/... → {result:{code:"..."}} — идентификатор отчёта.
// Шаг 2: POST /v1/report/info {code} → {result:{file:"<URL>", status:"success"}}.
// Шаг 3: GET <URL> → скачиваем XLSX-файл.
//
// Применяется для: b2b_sales (PDF), mutual_settlement, compensation, decompensation.
// =========================================================================

pub struct OzonAsyncReport {
    type_id: String,
    display_name: String,
    category: ReportCategory,
    client: OzonHttpClient,
    endpoint: &'static str,
}

impl OzonAsyncReport {
    #[must_use]
    pub fn new(
        type_id: &str,
        display_name: &str,
        category: ReportCategory,
        client: OzonHttpClient,
        endpoint: &'static str,
    ) -> Self {
        Self {
            type_id: type_id.into(),
            display_name: display_name.into(),
            category,
            client,
            endpoint,
        }
    }
}

#[async_trait]
impl Report for OzonAsyncReport {
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
        AcquisitionMode::Period
    }
    fn downloader_kind(&self) -> DownloaderKind {
        DownloaderKind::Api
    }
    fn parameters(&self) -> &[ReportParameter] {
        &[]
    }

    async fn download(
        &self,
        auth: &dyn mdwf_core::Authenticator,
        params: &ReportParams,
        _progress: ProgressCallbackRef,
        _cancel: CancelToken,
    ) -> CoreResult<Vec<DownloadedFile>> {
        // Шаг 1: запрос отчёта → получаем code.
        let body = build_download_body(params);
        let resp = self.client.post(self.endpoint, &body, auth).await?;

        // Извлекаем code: {result:{code:"..."}}.
        let code = resp
            .get("result")
            .and_then(|r| r.get("code"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                CoreError::Internal(format!(
                    "Ozon {}: ответ не содержит result.code",
                    self.type_id
                ))
            })?;

        // Шаг 2: запрашиваем статус/ссылку через /v1/report/info.
        let report_info = self.client.post_report_info(code, auth).await?;
        let result = report_info
            .get("result")
            .ok_or_else(|| CoreError::Internal("report/info: нет result".into()))?;

        let status = result
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("");

        match status {
            "failed" => {
                let err = result
                    .get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("неизвестная ошибка");
                return Err(CoreError::Internal(format!(
                    "Ozon {}: генерация отчёта не удалась: {err}",
                    self.type_id
                )));
            }
            "waiting" | "processing" => {
                return Err(CoreError::Internal(format!(
                    "Ozon {}: отчёт ещё генерируется (статус {status}). Повторите позже.",
                    self.type_id
                )));
            }
            // "success" и неизвестные статусы — продолжаем к скачиванию.
            _ => {}
        }

        // Извлекаем ссылку на файл.
        let file_url = result
            .get("file")
            .and_then(|f| f.as_str())
            .ok_or_else(|| {
                CoreError::Internal(format!(
                    "Ozon {}: report/info вернул success, но нет ссылки file",
                    self.type_id
                ))
            })?;

        // Шаг 3: скачиваем файл по ссылке.
        let bytes = self.client.download_file(file_url).await?;
        let period = params.period.clone().unwrap_or_else(|| "current".into());
        Ok(vec![DownloadedFile::with_content(
            format!("{}_{}.xlsx", self.type_id, period),
            "xlsx",
            bytes,
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_count_is_17() {
        let caps = capabilities();
        assert_eq!(caps.reports.len(), 17, "got {} reports", caps.reports.len());
        // accrual_types удалён — это служебный справочник, не выгрузка.
        assert!(!caps.reports.iter().any(|r| r.type_id == "ozon.accrual_types"));
    }

    #[test]
    fn capabilities_have_auth_fields() {
        let caps = capabilities();
        assert_eq!(caps.auth_fields.len(), 2);
        assert!(caps.auth_fields.iter().any(|f| f.id == "client_id"));
        assert!(caps.auth_fields.iter().any(|f| f.id == "api_key" && f.secret));
    }

    #[test]
    fn out_of_scope_has_5_documents() {
        let oos = out_of_scope();
        assert_eq!(oos.len(), 5);
        assert!(oos.iter().any(|(n, _)| *n == "Акты сверки"));
    }

    #[test]
    fn both_modes_present() {
        let caps = capabilities();
        assert!(caps.reports.iter().any(|r| r.acquisition_mode == AcquisitionMode::Period));
        assert!(caps.reports.iter().any(|r| r.acquisition_mode == AcquisitionMode::Browsable));
    }

    #[test]
    fn async_reports_are_period() {
        let caps = capabilities();
        for rid in ["ozon.compensation", "ozon.b2b_sales", "ozon.mutual_settlement"] {
            let r = caps.reports.iter().find(|r| r.type_id == rid);
            assert!(r.is_some(), "{rid} missing");
            assert_eq!(r.unwrap().acquisition_mode, AcquisitionMode::Period, "{rid}");
        }
    }
}
