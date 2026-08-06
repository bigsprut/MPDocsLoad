//! Описания отчётов Ozon (спец. §2.2.1 — 10 отчётов через API) + out-of-scope.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use mdwf_core::{
    AcquisitionMode, Capabilities, CoreError, CoreResult, CancelToken, DownloadedFile,
    DownloaderKind, ProgressCallbackRef, Report, ReportCategory, ReportDescriptor,
    ReportParameter, ReportParameterKind, ReportParams, ReportRef,
};
use mdwf_core::capabilities::{AuthField, AuthFieldKind, AuthType};

use crate::client::OzonHttpClient;
use crate::date_format;

/// Возвращает Capabilities Ozon: тип авторизации + поля формы + 10 отчётов.
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

/// Описания всех отчётов Ozon (спец. §2.2.1). Удалены: служебные, дублёры,
/// сложные/устаревшие. Теперь 8 отчётов.
///
/// Удалено в этом чате: `accrual_postings` и `accrual_by_day` — сверкено с
/// docs.ozon.ru: `/v1/finance/accrual/postings` требует конкретные `posting_numbers`
/// (1–200 номеров отправлений), а `/v1/finance/accrual/by-day` — один конкретный
/// `date` + постраничный `last_id`. Ни один не встаёт в модель Report
/// (Browsable: диапазон→список→выбор; Period: месяц→генерация), а выдумывать
/// обёртку нельзя. Раньше код шлёт `{limit,offset,date_from,date_to}` → 400.
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

fn param_period_month(id: &str, label: &str, required: bool) -> ReportParameter {
    ReportParameter {
        id: id.into(),
        label: label.into(),
        kind: ReportParameterKind::YearMonth,
        required,
        default: Some(date_format::format_year_month(2026, 1)),
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

/// Реализация отчёта Ozon в Period-режиме.
///
/// POST с телом из params → возвращает JSON как один файл. Browsable-режим у Ozon
/// отсутствует: эндпоинты начислений (accrual_postings/by_day) требуют конкретные
// `posting_numbers` / один `date` и не встают в модель «диапазон → список → выбор».
pub struct OzonReport {
    type_id: String,
    display_name: String,
    category: ReportCategory,
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
        let body = build_download_body(&self.type_id, params);
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

/// Строит тело запроса для download из параметров.
///
/// Формат тела зависит от type_id — разные эндпоинты Ozon ждут разные схемы
/// периода (сверено с официальным swagger.json API v2.1):
/// - `month`+`year` как integer: realization, realization_posting.
/// - `date` как строка YYYY-MM: compensation, decompensation, b2b_sales,
///   mutual_settlement (async-отчёты).
/// - `date_from`+`date_to` как YYYY-MM-DD: balance, buyout.
/// - browsable-отчёты (accrual_*): без периода, тело из params.values.
fn build_download_body(type_id: &str, params: &ReportParams) -> serde_json::Value {
    let mut body = json!({});
    let period = params.period.as_deref();
    // date_from/date_to из params.values (заполняются UI как YYYY-MM-DD).
    let date_from = params.get("date_from").map(str::to_string);
    let date_to = params.get("date_to").map(str::to_string);

    match type_id {
        // month + year как integer (строка YYYY-MM → числа).
        "ozon.realization" | "ozon.realization_posting" => {
            if let Some(p) = period {
                if let Some((y, m)) = date_format::parse_year_month(p) {
                    body["year"] = json!(y);
                    body["month"] = json!(m);
                }
            }
        }
        // date_from + date_to как YYYY-MM-DD.
        "ozon.balance" | "ozon.buyout" => {
            if let Some(df) = &date_from {
                body["date_from"] = json!(df);
            }
            if let Some(dt) = &date_to {
                body["date_to"] = json!(dt);
            }
        }
        // date как строка YYYY-MM (async-отчёты).
        "ozon.compensation" | "ozon.decompensation" | "ozon.b2b_sales"
        | "ozon.mutual_settlement" => {
            if let Some(p) = period {
                body["date"] = json!(p);
            }
        }
        _ => {
            // Browsable-отчёты (transaction_list, transaction_totals, accrual_*):
            // период не используется, тело собирается из params.values ниже.
        }
    }

    // ids — для Browsable-режима (если есть).
    if let Some(ids) = params.get("ids") {
        body["ids"] = json!(ids.split(',').collect::<Vec<_>>());
    }
    // Произвольные параметры из values (кроме служебных ключей).
    for (k, v) in &params.values {
        if !matches!(k.as_str(), "ids" | "date_from" | "date_to") {
            body[k.as_str()] = json!(v);
        }
    }
    body
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
        let body = build_download_body(&self.type_id, params);
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
    fn build_body_realization_month_year_int() {
        // regression: раньше шла строка "2026-03" → Ozon 400 invalid int32.
        let params = ReportParams {
            period: Some("2026-03".into()),
            ..Default::default()
        };
        let body = build_download_body("ozon.realization", &params);
        assert_eq!(body["year"], 2026);
        assert_eq!(body["month"], 3);
        // Не строка, а число.
        assert!(body["month"].is_number());
    }

    #[test]
    fn build_body_realization_posting_same_as_realization() {
        let params = ReportParams {
            period: Some("2025-12".into()),
            ..Default::default()
        };
        let body = build_download_body("ozon.realization_posting", &params);
        assert_eq!(body["year"], 2025);
        assert_eq!(body["month"], 12);
    }

    #[test]
    fn build_body_balance_date_from_to() {
        let params = ReportParams::default()
            .with("date_from", "2026-07-01")
            .with("date_to", "2026-07-31");
        let body = build_download_body("ozon.balance", &params);
        assert_eq!(body["date_from"], "2026-07-01");
        assert_eq!(body["date_to"], "2026-07-31");
    }

    #[test]
    fn build_body_buyout_date_from_to() {
        let params = ReportParams::default()
            .with("date_from", "2026-07-01")
            .with("date_to", "2026-07-31");
        let body = build_download_body("ozon.buyout", &params);
        assert_eq!(body["date_from"], "2026-07-01");
        assert_eq!(body["date_to"], "2026-07-31");
    }

    #[test]
    fn build_body_async_reports_date_yyyy_mm() {
        // compensation/decompensation/b2b_sales/mutual_settlement ждут date=YYYY-MM.
        for tid in [
            "ozon.compensation",
            "ozon.decompensation",
            "ozon.b2b_sales",
            "ozon.mutual_settlement",
        ] {
            let params = ReportParams {
                period: Some("2026-03".into()),
                ..Default::default()
            };
            let body = build_download_body(tid, &params);
            assert_eq!(body["date"], "2026-03", "{tid}");
        }
    }

    #[test]
    fn reports_count_is_8() {
        let caps = capabilities();
        assert_eq!(caps.reports.len(), 8, "got {} reports", caps.reports.len());
        // accrual_types удалён — это служебный справочник, не выгрузка.
        assert!(!caps.reports.iter().any(|r| r.type_id == "ozon.accrual_types"));
        // b2b_sales_json удалён — дублёр b2b_sales (PDF).
        assert!(!caps.reports.iter().any(|r| r.type_id == "ozon.b2b_sales_json"));
        // cash_flow/analytics/act_discrepancy удалены — требуют сложных схем/UI.
        assert!(!caps.reports.iter().any(|r| r.type_id == "ozon.cash_flow"));
        assert!(!caps.reports.iter().any(|r| r.type_id == "ozon.analytics"));
        assert!(!caps.reports.iter().any(|r| r.type_id == "ozon.act_discrepancy"));
        // transaction_list/totals удалены (deprecated → отключены 8 сентября 2026).
        assert!(!caps.reports.iter().any(|r| r.type_id == "ozon.transaction_list"));
        assert!(!caps.reports.iter().any(|r| r.type_id == "ozon.transaction_totals"));
        // realization_by_day удалён (требует подписку Premium Plus/Pro).
        assert!(!caps.reports.iter().any(|r| r.type_id == "ozon.realization_by_day"));
        // accrual_postings/by_day удалены (не списочные по API: требуют
        // posting_numbers / один date + last_id — не встают в модель Report).
        assert!(!caps.reports.iter().any(|r| r.type_id == "ozon.accrual_postings"));
        assert!(!caps.reports.iter().any(|r| r.type_id == "ozon.accrual_by_day"));
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
    fn all_reports_are_period() {
        // У Ozon нет Browsable-отчётов: эндпоинты начислений (accrual_postings/
        // by_day) требуют конкретные posting_numbers / один date + last_id и не
        // встают в модель «диапазон → список → выбор». Все отчёты — Period.
        let caps = capabilities();
        assert!(caps.reports.iter().all(|r| r.acquisition_mode == AcquisitionMode::Period));
        assert!(!caps
            .reports
            .iter()
            .any(|r| r.acquisition_mode == AcquisitionMode::Browsable));
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
