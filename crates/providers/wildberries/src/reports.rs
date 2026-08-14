//! Описания отчётов WB (сверено с официальной OpenAPI-спекой dev.wildberries.ru
//! через зеркало github.com/eslazarev/wildberries-sdk, specs/*.yaml; аудит
//! живым прогоном 2026-08-14).
//!
//! Разделы документации (все домены подтверждены):
//! - Баланс: GET finance-api /api/v1/account/balance (плоский объект)
//! - Финансы: POST finance-api /api/finance/v1/sales-reports/* , /acquiring/*
//!   (list — offset-пагинация; detailed — курсор rrdId, конец по 204)
//! - Документы: GET documents-api /api/v1/documents/*
//! - Отчёты (стат): GET statistics-api /api/v1/supplier/orders, /sales
//!   (только dateFrom по lastChangeDate + flag; без пагинации)
//! - Удержания: GET seller-analytics /api/analytics/v1/{deductions,measurement-penalties}
//!   (dateTo и limit обязательны; offset-пагинация с total)
//! - Антифрод: GET seller-analytics /api/v1/analytics/antifraud-details
//!   (только date; фильтрация по периоду — локально)
//! - Возвраты: GET returns-api /api/v1/claims (окно 14 дней,
//!   обязательный is_archive; фильтрация по периоду — локально)
//! - Приёмка (async): GET seller-analytics /api/v1/acceptance_report → taskId
//!   → poll status → download

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use mdwf_core::{
    capabilities::{AuthField, AuthFieldKind, AuthType},
    AcquisitionMode, Capabilities, CoreError, CoreResult, CancelToken, DocumentEntry,
    DocumentFilter, DownloadedFile, DownloaderKind, PeriodKind, ProgressCallbackRef, Report,
    ReportCategory, ReportDescriptor, ReportParameter, ReportParameterKind, ReportParams, ReportRef,
};

use crate::client::{WbDomain, WbHttpClient};
use crate::date_format;
use tracing::warn;

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
        desc_period(
            "wb.balance",
            "Баланс продавца",
            ReportCategory::Finance,
            PeriodKind::None,
            "Текущий баланс продавца (срез на момент запроса, без периода).",
        ),
        // --- Финансы (finance-api, POST) ---
        desc_period(
            "wb.sales_reports_list",
            "Реестр реализации (список)",
            ReportCategory::Finance,
            PeriodKind::Range,
            "Реестр отчётов реализации за период.",
        ),
        desc_period(
            "wb.sales_reports_detailed",
            "Детализация реализации (за период)",
            ReportCategory::Finance,
            PeriodKind::Range,
            "Детализация реализации за период.",
        ),
        desc_period(
            "wb.acquiring_list",
            "Эквайринг (список)",
            ReportCategory::Finance,
            PeriodKind::Range,
            "Эквайринг — список операций за период.",
        ),
        desc_period(
            "wb.acquiring_detailed",
            "Эквайринг (детализация)",
            ReportCategory::Finance,
            PeriodKind::Range,
            "Эквайринг — детализация операций за период.",
        ),
        // --- Документы (documents-api, GET) ---
        desc_browsable(
            "wb.documents",
            "Документы (УПД/УКД/акты) — по категории",
            ReportCategory::Documents,
            PeriodKind::Range,
            "Документы (УПД/УКД/акты) по категории за период.",
        ),
        // --- Статистика (statistics-api, GET) ---
        desc_browsable(
            "wb.orders",
            "Заказы",
            ReportCategory::Operational,
            PeriodKind::Range,
            "Заказы, изменёншиеся с начала периода (API фильтрует по дате изменения; \
             WB хранит данные 90 дней).",
        ),
        desc_browsable(
            "wb.sales",
            "Продажи",
            ReportCategory::Operational,
            PeriodKind::Range,
            "Продажи, изменявшиеся с начала периода (API фильтрует по дате изменения; \
             WB хранит данные 90 дней).",
        ),
        // --- Удержания (seller-analytics-api, GET) ---
        desc_browsable(
            "wb.deductions",
            "Штрафы за подмены",
            ReportCategory::Penalties,
            PeriodKind::Range,
            "Штрафы за подмены и неверные вложения за период.",
        ),
        desc_browsable(
            "wb.measurement_penalties",
            "Штрафы за габариты",
            ReportCategory::Penalties,
            PeriodKind::Range,
            "Штрафы за занижение габаритов за период.",
        ),
        desc_browsable(
            "wb.antifraud",
            "Самовыкупы (антифрод)",
            ReportCategory::Penalties,
            PeriodKind::Range,
            "Отчёт по самовыкупам (антифрод), еженедельный. Выгружаются все данные, \
             фильтрация по периоду выполняется локально.",
        ),
        // --- Возвраты (claims) — спека 09-communications.yaml ---
        desc_browsable(
            "wb.claims",
            "Возвраты (заявки покупателей)",
            ReportCategory::Returns,
            PeriodKind::Range,
            "Заявки на возврат товаров. API WB отдаёт заявки только за последние \
             14 дней; выгружаются и активные, и архивные.",
        ),
        // --- Async-отчёт приёмки (спека 12-reports.yaml) ---
        desc_period(
            "wb.acceptance_report",
            "Аналитический отчёт приёмки (async)",
            ReportCategory::Finance,
            PeriodKind::Range,
            "Аналитический отчёт приёмки за период (до 31 дня). Формируется на \
             стороне WB — выгрузка может занять несколько минут.",
        )
        .with_max_range_days(31),
    ]
}

// --- Хелперы построения дескрипторов ---

#[allow(clippy::needless_pass_by_value)]
fn desc_period(
    type_id: &str,
    display_name: &str,
    category: ReportCategory,
    period_kind: PeriodKind,
    description: &str,
) -> ReportDescriptor {
    ReportDescriptor {
        type_id: type_id.into(),
        display_name: display_name.into(),
        category,
        acquisition_mode: AcquisitionMode::Period,
        downloader_kind: DownloaderKind::Api,
        parameters: vec![param_date_range()],
        period_kind,
        description: Some(description.into()),
        max_range_days: None,
    }
}

#[allow(clippy::needless_pass_by_value)]
fn desc_browsable(
    type_id: &str,
    display_name: &str,
    category: ReportCategory,
    period_kind: PeriodKind,
    description: &str,
) -> ReportDescriptor {
    ReportDescriptor {
        type_id: type_id.into(),
        display_name: display_name.into(),
        category,
        acquisition_mode: AcquisitionMode::Browsable,
        downloader_kind: DownloaderKind::Api,
        parameters: vec![param_date_range()],
        period_kind,
        description: Some(description.into()),
        max_range_days: None,
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

/// Семейство запроса: определяет HTTP-метод, построение параметров, пагинацию
/// и разбор ответа. Специфика каждого семейства сверена со спекой WB
/// (eslazarev/wildberries-sdk, specs/*.yaml).
#[derive(Clone, Copy)]
pub(crate) enum WbQueryKind {
    /// GET без параметров; ответ — плоский объект (баланс).
    Balance,
    /// GET ?dateFrom&flag=0; ответ — прямой массив. Одно обращение: спека не
    /// предусматривает ни dateTo, ни limit (фильтр — по lastChangeDate).
    Statistics,
    /// POST {dateFrom,dateTo,limit,offset}; ответ — прямой массив; страницы по offset.
    FinanceList,
    /// POST {dateFrom,dateTo,limit,rrdId}; ответ — прямой массив; курсор rrdId,
    /// конец данных — 204 No Content (клиент возвращает пустой массив).
    FinanceDetailed,
    /// GET ?dateFrom&dateTo&limit&offset (dateTo и limit обязательны);
    /// ответ `{data:{reports:[...],total}}`; страницы по offset до total.
    Penalties,
    /// GET без параметров; ответ `{details:[...]}`; фильтрация по периоду локально.
    Antifraud,
    /// GET ?is_archive&limit&offset; ответ `{claims:[...],total}`; сервер отдаёт
    /// только последние 14 дней; выгружаются активные и архивные заявки,
    /// фильтрация по периоду локально.
    Claims,
}

/// Страница offset-пагинации финансов (list-методы): максимум по спеке — 1000.
const FINANCE_LIST_PAGE: u32 = 1000;
/// Лимит detailed-методов финансов: по спеке допускается до 100 000 строк.
const FINANCE_DETAILED_PAGE: u32 = 100_000;
/// Страница удержаний (deductions / measurement-penalties): максимум 1000.
const PENALTIES_PAGE: u32 = 1000;
/// Страница заявок возврата (claims): максимум 200.
const CLAIMS_PAGE: u32 = 200;
/// Страховочный потолок числа страниц (защита от вечного цикла при аномальных
/// ответах). 1000 × 1000 = 1 млн строк — заведомо больше реальных объёмов.
const MAX_PAGES: u32 = 1000;

/// Фабрика отчётов: возвращает `ReportRef` по `type_id`.
/// Все пути/методы/форматы сверены с официальной спекой WB.
pub fn make_report(type_id: &str, client: WbHttpClient) -> CoreResult<ReportRef> {
    let report: ReportRef = match type_id {
        // === Баланс === GET finance-api /api/v1/account/balance (плоский объект)
        "wb.balance" => Arc::new(WbReport::new(
            "wb.balance",
            "Баланс продавца",
            ReportCategory::Finance,
            AcquisitionMode::Period,
            WbDomain::Finance,
            "/api/v1/account/balance",
            WbQueryKind::Balance,
            client,
        )),

        // === Финансы === POST finance-api
        "wb.sales_reports_list" => Arc::new(WbReport::new(
            "wb.sales_reports_list",
            "Реестр реализации (список)",
            ReportCategory::Finance,
            AcquisitionMode::Period,
            WbDomain::Finance,
            "/api/finance/v1/sales-reports/list",
            WbQueryKind::FinanceList,
            client,
        )),
        "wb.sales_reports_detailed" => Arc::new(WbReport::new(
            "wb.sales_reports_detailed",
            "Детализация реализации",
            ReportCategory::Finance,
            AcquisitionMode::Period,
            WbDomain::Finance,
            "/api/finance/v1/sales-reports/detailed",
            WbQueryKind::FinanceDetailed,
            client,
        )),
        "wb.acquiring_list" => Arc::new(WbReport::new(
            "wb.acquiring_list",
            "Эквайринг (список)",
            ReportCategory::Finance,
            AcquisitionMode::Period,
            WbDomain::Finance,
            "/api/finance/v1/acquiring/list",
            WbQueryKind::FinanceList,
            client,
        )),
        "wb.acquiring_detailed" => Arc::new(WbReport::new(
            "wb.acquiring_detailed",
            "Эквайринг (детализация)",
            ReportCategory::Finance,
            AcquisitionMode::Period,
            WbDomain::Finance,
            "/api/finance/v1/acquiring/detailed",
            WbQueryKind::FinanceDetailed,
            client,
        )),

        // === Документы === (отдельные реализации, documents-api GET)
        "wb.documents" => Arc::new(WbDocumentsReport::new(client)),
        "wb.documents_categories" => Arc::new(WbCategoriesReport::new(client)),

        // === Статистика === GET statistics-api, ответ — прямой массив
        "wb.orders" => Arc::new(WbReport::new(
            "wb.orders",
            "Заказы",
            ReportCategory::Operational,
            AcquisitionMode::Browsable,
            WbDomain::Statistics,
            "/api/v1/supplier/orders",
            WbQueryKind::Statistics,
            client,
        )),
        "wb.sales" => Arc::new(WbReport::new(
            "wb.sales",
            "Продажи",
            ReportCategory::Operational,
            AcquisitionMode::Browsable,
            WbDomain::Statistics,
            "/api/v1/supplier/sales",
            WbQueryKind::Statistics,
            client,
        )),

        // === Удержания === GET seller-analytics-api
        "wb.deductions" => Arc::new(WbReport::new(
            "wb.deductions",
            "Штрафы за подмены",
            ReportCategory::Penalties,
            AcquisitionMode::Browsable,
            WbDomain::Analytics,
            "/api/analytics/v1/deductions",
            WbQueryKind::Penalties,
            client,
        )),
        "wb.measurement_penalties" => Arc::new(WbReport::new(
            "wb.measurement_penalties",
            "Штрафы за габариты",
            ReportCategory::Penalties,
            AcquisitionMode::Browsable,
            WbDomain::Analytics,
            "/api/analytics/v1/measurement-penalties",
            WbQueryKind::Penalties,
            client,
        )),
        "wb.antifraud" => Arc::new(WbReport::new(
            "wb.antifraud",
            "Самовыкупы (антифрод)",
            ReportCategory::Penalties,
            AcquisitionMode::Browsable,
            WbDomain::Analytics,
            "/api/v1/analytics/antifraud-details",
            WbQueryKind::Antifraud,
            client,
        )),

        // === Возвраты === спека 09-communications: GET /api/v1/claims
        "wb.claims" => Arc::new(WbReport::new(
            "wb.claims",
            "Возвраты (заявки покупателей)",
            ReportCategory::Returns,
            AcquisitionMode::Browsable,
            WbDomain::Returns,
            "/api/v1/claims",
            WbQueryKind::Claims,
            client,
        )),

        // === Аналитический отчёт приёмки === спека 12-reports: async create→poll→download
        "wb.acceptance_report" => Arc::new(WbAcceptanceReport::new(client)),

        _ => return Err(CoreError::ReportTypeNotSupported(type_id.to_string())),
    };
    Ok(report)
}

// =========================================================================
// WbReport — универсальный отчёт (GET или POST, с пагинацией по семейству)
// =========================================================================

/// Универсальный отчёт WB. Семейство запроса (`WbQueryKind`) определяет метод,
/// параметры и способ пагинации; `fetch_all` обходит ВСЕ страницы и отдаёт
/// полные данные (раньше финансовые отчёты молча обрезались первой страницей).
pub struct WbReport {
    type_id: String,
    display_name: String,
    category: ReportCategory,
    mode: AcquisitionMode,
    domain: WbDomain,
    path: &'static str,
    kind: WbQueryKind,
    client: WbHttpClient,
}

/// Результат полного обхода страниц: массив строк либо единичный объект (баланс).
enum Fetched {
    Rows(Vec<Value>),
    Object(Value),
}

impl WbReport {
    #[must_use]
    pub fn new(
        type_id: &str,
        display_name: &str,
        category: ReportCategory,
        mode: AcquisitionMode,
        domain: WbDomain,
        path: &'static str,
        kind: WbQueryKind,
        client: WbHttpClient,
    ) -> Self {
        Self {
            type_id: type_id.into(),
            display_name: display_name.into(),
            category,
            mode,
            domain,
            path,
            kind,
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
        self.category
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
        progress: ProgressCallbackRef,
        cancel: CancelToken,
    ) -> CoreResult<Vec<DocumentEntry>> {
        match self.fetch_all(auth, filter, progress, cancel).await? {
            Fetched::Rows(rows) => Ok(rows
                .iter()
                .enumerate()
                .map(|(i, r)| entry_from_row(r, i, &self.type_id))
                .collect()),
            // Баланс — одиночный объект, списка документов нет.
            Fetched::Object(_) => Ok(Vec::new()),
        }
    }

    async fn download(
        &self,
        auth: &dyn mdwf_core::Authenticator,
        params: &ReportParams,
        progress: ProgressCallbackRef,
        cancel: CancelToken,
    ) -> CoreResult<Vec<DownloadedFile>> {
        // Тот же обход страниц; результат — Excel с русскими колонками
        // (fallback на JSON, если конвертация не удалась — данные не теряем).
        let filter = filter_from_params(params);
        let fetched = self.fetch_all(auth, &filter, progress, cancel).await?;
        let period = params.period.clone().unwrap_or_else(|| "current".into());
        let base_name = format!("{}_{}", self.type_id, period);
        match fetched {
            Fetched::Rows(rows) if !rows.is_empty() => match crate::xlsx::rows_to_xlsx(&self.type_id, &rows) {
                Ok(bytes) => Ok(vec![DownloadedFile::with_content(
                    base_name,
                    "xlsx",
                    bytes,
                )]),
                Err(e) => {
                    warn!(error = %e, report = %self.type_id, "xlsx conversion failed — saving JSON");
                    Ok(vec![json_file(&rows, &base_name)])
                }
            },
            // Пустой результат — честный JSON [] (2 байта, однозначно «нет данных»).
            Fetched::Rows(rows) => Ok(vec![json_file(&rows, &base_name)]),
            // Баланс: плоский объект → маленькая таблица показателей.
            Fetched::Object(v) => match crate::xlsx::balance_to_xlsx(&v) {
                Ok(bytes) => Ok(vec![DownloadedFile::with_content(
                    base_name,
                    "xlsx",
                    bytes,
                )]),
                Err(e) => {
                    warn!(error = %e, report = %self.type_id, "xlsx conversion failed — saving JSON");
                    let content = serde_json::to_vec_pretty(&v)
                        .map_err(|e| CoreError::Internal(format!("serialize: {e}")))?;
                    Ok(vec![DownloadedFile::with_content(base_name, "json", content)])
                }
            },
        }
    }
}

/// Сохраняет строки как pretty-JSON (fallback при неудачной xlsx-конвертации
/// и пустой результат).
fn json_file(rows: &[Value], base_name: &str) -> DownloadedFile {
    let content = serde_json::to_vec_pretty(&Value::Array(rows.to_vec()))
        .unwrap_or_else(|_| b"[]".to_vec());
    DownloadedFile::with_content(base_name.to_string(), "json", content)
}

impl WbReport {
    /// Обходит все страницы согласно семейству запроса. Прогресс — постранично,
    /// отмена проверяется между страницами.
    async fn fetch_all(
        &self,
        auth: &dyn mdwf_core::Authenticator,
        filter: &DocumentFilter,
        progress: ProgressCallbackRef,
        cancel: CancelToken,
    ) -> CoreResult<Fetched> {
        match self.kind {
            WbQueryKind::Balance => {
                let json = self.client.get(self.domain, self.path, &[], auth).await?;
                Ok(Fetched::Object(json))
            }

            WbQueryKind::Statistics => {
                // Спека: только dateFrom (фильтр по lastChangeDate) и flag.
                // dateTo/limit эндпоинтом не поддерживаются — не шлём.
                let mut q: Vec<(&'static str, String)> = vec![("flag", "0".into())];
                if let Some(d) = filter.date_from {
                    q.push(("dateFrom", date_format::format_date_moscow(d)));
                }
                let refs: Vec<(&str, &str)> = q.iter().map(|(k, v)| (*k, v.as_str())).collect();
                let json = self.client.get(self.domain, self.path, &refs, auth).await?;
                Ok(Fetched::Rows(json.as_array().cloned().unwrap_or_default()))
            }

            WbQueryKind::FinanceList => {
                let mut rows: Vec<Value> = Vec::new();
                let mut offset: u32 = 0;
                for page_no in 1..=MAX_PAGES {
                    if cancel.is_cancelled() {
                        return Err(CoreError::Cancelled);
                    }
                    let mut body = json!({ "limit": FINANCE_LIST_PAGE, "offset": offset });
                    add_date_bounds(&mut body, filter);
                    let page = self.client.post(self.domain, self.path, &body, auth).await?;
                    let page_rows = page.as_array().cloned().unwrap_or_default();
                    let got = page_rows.len();
                    rows.extend(page_rows);
                    report_page(&progress, page_no, rows.len(), &self.type_id);
                    // Неполная страница — последняя (total в ответе нет).
                    if got < FINANCE_LIST_PAGE as usize || reached_ceiling(&rows, filter.limit) {
                        break;
                    }
                    offset = offset.saturating_add(FINANCE_LIST_PAGE);
                }
                truncate_ceiling(&mut rows, filter.limit);
                Ok(Fetched::Rows(rows))
            }

            WbQueryKind::FinanceDetailed => {
                let mut rows: Vec<Value> = Vec::new();
                let mut cursor: i64 = 0;
                for page_no in 1..=MAX_PAGES {
                    if cancel.is_cancelled() {
                        return Err(CoreError::Cancelled);
                    }
                    let mut body = json!({ "limit": FINANCE_DETAILED_PAGE, "rrdId": cursor });
                    add_date_bounds(&mut body, filter);
                    let page = self.client.post(self.domain, self.path, &body, auth).await?;
                    let page_rows = page.as_array().cloned().unwrap_or_default();
                    // 204/пустой массив — данных больше нет (курсор дошёл до конца).
                    if page_rows.is_empty() {
                        break;
                    }
                    // Новый курсор — rrdId последней строки; без него (или если
                    // не двигается) продолжать нельзя — выходим, не зацикливаясь.
                    let next = page_rows
                        .last()
                        .and_then(|r| r.get("rrdId"))
                        .and_then(Value::as_i64);
                    rows.extend(page_rows);
                    report_page(&progress, page_no, rows.len(), &self.type_id);
                    let Some(cursor_next) = next else {
                        break;
                    };
                    if cursor_next == cursor || reached_ceiling(&rows, filter.limit) {
                        break;
                    }
                    cursor = cursor_next;
                }
                truncate_ceiling(&mut rows, filter.limit);
                Ok(Fetched::Rows(rows))
            }

            WbQueryKind::Penalties => {
                // Спека: dateTo и limit ОБЯЗАТЕЛЬНЫ. Если фильтр без даты конца —
                // берём сегодня (иначе WB ответит 400 missing parameter).
                let to = filter
                    .date_to
                    .unwrap_or_else(|| chrono::Local::now().date_naive());
                let mut rows: Vec<Value> = Vec::new();
                let mut offset: u32 = 0;
                for page_no in 1..=MAX_PAGES {
                    if cancel.is_cancelled() {
                        return Err(CoreError::Cancelled);
                    }
                    let mut q: Vec<(&'static str, String)> = vec![
                        ("dateTo", date_format::format_date_moscow_eod(to)),
                        ("limit", PENALTIES_PAGE.to_string()),
                        ("offset", offset.to_string()),
                    ];
                    if let Some(d) = filter.date_from {
                        q.push(("dateFrom", date_format::format_date_moscow(d)));
                    }
                    let refs: Vec<(&str, &str)> =
                        q.iter().map(|(k, v)| (*k, v.as_str())).collect();
                    let json = self.client.get(self.domain, self.path, &refs, auth).await?;
                    let data = json.get("data").cloned().unwrap_or(Value::Null);
                    let page_rows = data
                        .get("reports")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let total = data.get("total").and_then(Value::as_u64).unwrap_or(0) as usize;
                    let got = page_rows.len();
                    rows.extend(page_rows);
                    report_page(&progress, page_no, rows.len(), &self.type_id);
                    if got < PENALTIES_PAGE as usize
                        || (total > 0 && rows.len() >= total)
                        || reached_ceiling(&rows, filter.limit)
                    {
                        break;
                    }
                    offset = offset.saturating_add(got as u32);
                }
                truncate_ceiling(&mut rows, filter.limit);
                Ok(Fetched::Rows(rows))
            }

            WbQueryKind::Antifraud => {
                // Спека: единственный параметр date (опциональный) — выгружаем всё
                // и фильтруем локально по пересечению недельных интервалов строк.
                let json = self.client.get(self.domain, self.path, &[], auth).await?;
                let mut rows = json
                    .get("details")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if let (Some(from), Some(to)) = (filter.date_from, filter.date_to) {
                    rows.retain(|r| match row_date_range(r) {
                        Some((rf, rt)) => rf <= to && rt >= from,
                        // Без дат в строке — оставляем (лучше показать, чем потерять).
                        None => true,
                    });
                }
                Ok(Fetched::Rows(rows))
            }

            WbQueryKind::Claims => {
                // Спека: параметры дат НЕТ — сервер отдаёт заявки за последние
                // 14 дней; обязательный is_archive разделяет активные и архивные.
                // Выгружаем оба среза и фильтруем локально по дате заявки (dt).
                let mut rows: Vec<Value> = Vec::new();
                for is_archive in [false, true] {
                    let label = if is_archive { "архивные" } else { "активные" };
                    progress.report(mdwf_core::ProgressUpdate::message(format!(
                        "Загрузка заявок возврата ({label})…"
                    )));
                    let mut offset: u32 = 0;
                    let mut sweep = 0usize;
                    for page_no in 1..=MAX_PAGES {
                        if cancel.is_cancelled() {
                            return Err(CoreError::Cancelled);
                        }
                        let q_owned: Vec<(&str, String)> = vec![
                            ("is_archive", is_archive.to_string()),
                            ("limit", CLAIMS_PAGE.to_string()),
                            ("offset", offset.to_string()),
                        ];
                        let refs: Vec<(&str, &str)> = q_owned
                            .iter()
                            .map(|(k, v)| (*k, v.as_str()))
                            .collect();
                        let json =
                            self.client.get(self.domain, self.path, &refs, auth).await?;
                        let page_rows = json
                            .get("claims")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        let total =
                            json.get("total").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let got = page_rows.len();
                        rows.extend(page_rows);
                        sweep += got;
                        report_page(&progress, page_no, rows.len(), &self.type_id);
                        if got == 0
                            || got < CLAIMS_PAGE as usize
                            || (total > 0 && sweep >= total)
                            || reached_ceiling(&rows, filter.limit)
                        {
                            break;
                        }
                        offset = offset.saturating_add(got as u32);
                    }
                }
                // Локальный фильтр по дате заявки (dt) в выбранном периоде.
                if let (Some(from), Some(to)) = (filter.date_from, filter.date_to) {
                    rows.retain(|r| {
                        r.get("dt")
                            .and_then(Value::as_str)
                            .and_then(parse_flexible_date)
                            .map_or(true, |d| from <= d && d <= to)
                    });
                }
                truncate_ceiling(&mut rows, filter.limit);
                Ok(Fetched::Rows(rows))
            }
        }
    }
}

/// Добавляет dateFrom/dateTo (RFC3339, Москва) в POST-тело финансов.
fn add_date_bounds(body: &mut Value, filter: &DocumentFilter) {
    if let Some(d) = filter.date_from {
        body["dateFrom"] = json!(date_format::format_date_moscow(d));
    }
    if let Some(d) = filter.date_to {
        body["dateTo"] = json!(date_format::format_date_moscow(d));
    }
}

/// Сообщает прогресс страницы: сколько строк накоплено.
fn report_page(progress: &ProgressCallbackRef, page_no: u32, total_rows: usize, type_id: &str) {
    progress.report(mdwf_core::ProgressUpdate {
        fraction: None,
        message: format!("{type_id}: страница {page_no}, строк: {total_rows}"),
        current: Some(total_rows as u64),
        total: None,
    });
}

/// Достигнут ли потолок общего числа строк (filter.limit).
fn reached_ceiling(rows: &[Value], limit: Option<u32>) -> bool {
    limit.is_some_and(|max| rows.len() >= max as usize)
}

/// Обрезает результат по потолку filter.limit.
fn truncate_ceiling(rows: &mut Vec<Value>, limit: Option<u32>) {
    if let Some(max) = limit {
        rows.truncate(max as usize);
    }
}

/// Диапазон дат строки (поля dateFrom..dateTo — недельные интервалы антифрода).
fn row_date_range(row: &Value) -> Option<(chrono::NaiveDate, chrono::NaiveDate)> {
    let from = row.get("dateFrom").and_then(Value::as_str).and_then(parse_flexible_date)?;
    let to = row.get("dateTo").and_then(Value::as_str).and_then(parse_flexible_date)?;
    Some((from, to))
}

/// Строит DocumentEntry из строки ответа: универсальные кандидаты полей id/
/// названия/даты (разные семейства WB называют их по-разному).
fn entry_from_row(item: &Value, i: usize, type_id: &str) -> DocumentEntry {
    // Универсальный ID: пробуем id (claims — UUID), srid, orderId, nmId, reportId, rrdId.
    let id = item
        .get("id")
        .or_else(|| item.get("srid"))
        .or_else(|| item.get("orderId"))
        .or_else(|| item.get("nmId"))
        .or_else(|| item.get("reportId"))
        .or_else(|| item.get("rrdId"))
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .unwrap_or_else(|| v.to_string())
        })
        .unwrap_or_else(|| format!("item-{i}"));
    let display = item
        .get("supplierArticle")
        .or_else(|| item.get("imt_name"))
        .or_else(|| item.get("subject"))
        .or_else(|| item.get("subjectName"))
        .or_else(|| item.get("category"))
        .or_else(|| item.get("brand"))
        .or_else(|| item.get("docTypeName"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{type_id} #{i}"));
    let mut e = DocumentEntry::new(id, display);
    e.category = type_id.to_string();
    e.extensions = vec!["json".into()];
    // Дата: пробуем date, saleDt, orderDt, rrDate, createDate, dt, lastChangeDate, order_dt.
    for date_field in [
        "date",
        "saleDt",
        "orderDt",
        "rrDate",
        "createDate",
        "dt",
        "lastChangeDate",
        "order_dt",
    ] {
        if let Some(d) = item.get(date_field).and_then(|v| v.as_str()) {
            e.date = parse_flexible_date(d);
            if e.date.is_some() {
                break;
            }
        }
    }
    e
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
// Хелперы построения параметров
// =========================================================================

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
        // YYYY-MM -> первый..последний день месяца (сдвиг «+30 дней» ошибался
        // для месяцев короче июля: февраль заканчивался 2-3 марта).
        if p.len() == 7 {
            if let Ok(first) = chrono::NaiveDate::parse_from_str(&format!("{p}-01"), "%Y-%m-%d") {
                f.date_from = Some(first);
                f.date_to = first
                    .checked_add_months(chrono::Months::new(1))
                    .and_then(|next| next.pred_opt());
            }
        }
    }
    f
}

// =========================================================================
// WbAcceptanceReport — async-отчёт приёмки (спека 12-reports.yaml)
// =========================================================================

/// Максимальный период выгрузки (спека: не больше 31 дня).
const ACCEPTANCE_MAX_RANGE_DAYS: i64 = 31;
/// Пауза между опросами статуса (спека допускает 1 запрос/5с; per-domain
/// лимитер аналитики дополнительно разносит запросы).
const ACCEPTANCE_POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Общий таймаут генерации (как у Ozon async-отчётов).
const ACCEPTANCE_TIMEOUT: Duration = Duration::from_secs(600);

/// Аналитический отчёт приёмки: async-паттерн WB.
/// 1) `GET /api/v1/acceptance_report?dateFrom&dateTo` → `{data:{taskId}}`;
/// 2) `GET /api/v1/acceptance_report/tasks/{id}/status` → `{data:{status}}`;
/// 3) `GET /api/v1/acceptance_report/tasks/{id}/download` → прямой массив
///    строк (204 — данных за период нет).
pub struct WbAcceptanceReport {
    client: WbHttpClient,
}

impl WbAcceptanceReport {
    #[must_use]
    pub fn new(client: WbHttpClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Report for WbAcceptanceReport {
    fn type_id(&self) -> &str {
        "wb.acceptance_report"
    }
    fn display_name(&self) -> &str {
        "Аналитический отчёт приёмки (async)"
    }
    fn category(&self) -> ReportCategory {
        ReportCategory::Finance
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

    async fn list(
        &self,
        _auth: &dyn mdwf_core::Authenticator,
        _filter: &DocumentFilter,
        _progress: ProgressCallbackRef,
        _cancel: CancelToken,
    ) -> CoreResult<Vec<DocumentEntry>> {
        // Периодический отчёт: выгружается целиком за период, списка документов нет.
        Err(CoreError::InvalidParameter(
            "wb.acceptance_report — периодический отчёт, список документов не предусмотрен".into(),
        ))
    }

    async fn download(
        &self,
        auth: &dyn mdwf_core::Authenticator,
        params: &ReportParams,
        progress: ProgressCallbackRef,
        cancel: CancelToken,
    ) -> CoreResult<Vec<DownloadedFile>> {
        let filter = filter_from_params(params);
        let from = filter.date_from.ok_or_else(|| {
            CoreError::InvalidParameter("не указана дата начала периода".into())
        })?;
        let to = filter.date_to.unwrap_or(from);
        let days = (to - from).num_days() + 1;
        if days > ACCEPTANCE_MAX_RANGE_DAYS {
            return Err(CoreError::InvalidParameter(format!(
                "период {days} дн. превышает максимум API WB ({ACCEPTANCE_MAX_RANGE_DAYS} дн.); \
                 разбейте выгрузку на части (например, по месяцам)"
            )));
        }

        // Спека: даты — ГГГГ-ММ-ДД (НЕ RFC3339), метод GET.
        let q: Vec<(&str, String)> = vec![
            ("dateFrom", from.format("%Y-%m-%d").to_string()),
            ("dateTo", to.format("%Y-%m-%d").to_string()),
        ];
        let refs: Vec<(&str, &str)> = q.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let create = self
            .client
            .get(WbDomain::Analytics, "/api/v1/acceptance_report", &refs, auth)
            .await?;
        let task_id = create
            .pointer("/data/taskId")
            .and_then(Value::as_str)
            .ok_or_else(|| CoreError::Protocol("WB: в ответе нет data.taskId".into()))?
            .to_string();

        // Поллинг статуса до done/ошибки/таймаута.
        let deadline = tokio::time::Instant::now() + ACCEPTANCE_TIMEOUT;
        loop {
            if cancel.is_cancelled() {
                return Err(CoreError::Cancelled);
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(CoreError::Protocol(format!(
                    "WB: отчёт приёмки не сформировался за {} с (taskId {task_id})",
                    ACCEPTANCE_TIMEOUT.as_secs()
                )));
            }
            let st = self
                .client
                .get(
                    WbDomain::Analytics,
                    &format!("/api/v1/acceptance_report/tasks/{task_id}/status"),
                    &[],
                    auth,
                )
                .await?;
            let status = st
                .pointer("/data/status")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_lowercase();
            match status.as_str() {
                "done" => break,
                "error" | "failed" | "canceled" | "cancelled" => {
                    return Err(CoreError::Api {
                        status: 200,
                        message: format!(
                            "WB: генерация отчёта приёмки завершилась со статусом «{status}»"
                        ),
                        retryable: false,
                    });
                }
                // in progress и прочие промежуточные статусы — ждём.
                _ => {}
            }
            progress.report(mdwf_core::ProgressUpdate::message(format!(
                "Отчёт приёмки формируется (статус: {status})…"
            )));
            tokio::time::sleep(ACCEPTANCE_POLL_INTERVAL).await;
        }

        // Скачивание результата: прямой массив строк (204 → пустой массив).
        // Непустой результат конвертируем в Excel (fallback — JSON).
        let rows = self
            .client
            .get(
                WbDomain::Analytics,
                &format!("/api/v1/acceptance_report/tasks/{task_id}/download"),
                &[],
                auth,
            )
            .await?;
        let period = params.period.clone().unwrap_or_else(|| "current".into());
        let base_name = format!("wb.acceptance_report_{period}");
        let arr = rows.as_array();
        if arr.is_some_and(|a| !a.is_empty()) {
            match crate::xlsx::rows_to_xlsx("wb.acceptance_report", arr.unwrap_or(&Vec::new())) {
                Ok(bytes) => {
                    return Ok(vec![DownloadedFile::with_content(
                        base_name,
                        "xlsx",
                        bytes,
                    )])
                }
                Err(e) => {
                    warn!(error = %e, "xlsx conversion failed — saving JSON");
                }
            }
        }
        let content = serde_json::to_vec_pretty(&rows)
            .map_err(|e| CoreError::Internal(format!("serialize: {e}")))?;
        Ok(vec![DownloadedFile::with_content(
            base_name,
            "json",
            content,
        )])
    }
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
        progress: ProgressCallbackRef,
        cancel: CancelToken,
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

        // Сигналим начало: пользователь видит, что процесс пошёл.
        progress.report(mdwf_core::ProgressUpdate::message(
            "Загрузка списка документов…",
        ));

        let mut all: Vec<crate::documents::WbDocument> = Vec::new();
        let mut offset: u32 = 0;
        let mut page_no: u32 = 0;
        for _ in 0..WB_DOCS_MAX_PAGES {
            if cancel.is_cancelled() {
                return Err(mdwf_core::CoreError::Cancelled);
            }
            // Если уже набрали ceiling — стоп.
            if let Some(max) = ceiling {
                if all.len() as u32 >= max {
                    break;
                }
            }
            page_no = page_no.saturating_add(1);
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
            // Живой прогресс: сколько уже накоплено и какая страница.
            // total известен только при заданном ceiling (limit пользователя);
            // иначе fraction=None — индикатор «качается» без процента.
            let total = ceiling.map(|m| u64::from(m.max(all.len() as u32)));
            #[allow(clippy::cast_precision_loss)]
            let fraction = total.map(|t| {
                if t == 0 {
                    1.0
                } else {
                    (all.len() as f64 / t as f64).clamp(0.0, 1.0)
                }
            });
            // Сообщение: «Получено 150 документов, всего: 500, страница 4».
            // Долю ceiling вычисляем отдельно, чтобы не вкладывать format!.
            let total_suffix = match ceiling {
                Some(max) => max.max(all.len() as u32).to_string(),
                None => String::new(),
            };
            let msg = match ceiling {
                Some(_) => format!(
                    "Получено {got} {word}, всего: {total_suffix}, страница {page_no}",
                    got = all.len(),
                    word = num_words(all.len(), "документ", "документа", "документов"),
                ),
                None => format!(
                    "Получено {got} {word}, страница {page_no}",
                    got = all.len(),
                    word = num_words(all.len(), "документ", "документа", "документов"),
                ),
            };
            progress.report(mdwf_core::ProgressUpdate {
                fraction,
                message: msg,
                current: Some(all.len() as u64),
                total,
            });
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
            // Имя файла на диске: приоритет — fileName из ответа /download
            // (WB сам сообщает осмысленное имя, напр. «Акт №072600203230 от
            // 26.07.2026.pdf» — это то, что лежит внутри zip). Иначе — name
            // из меты UI (поле name ответа /list), иначе serviceName.
            // Расширение из fileName отрезаем: оно дублируется через {ext}
            // шаблона, иначе получился бы «...pdf.zip».
            let source_id = downloaded
                .file_name
                .as_deref()
                .map(strip_extension)
                .filter(|s| !s.is_empty())
                .or_else(|| meta.get(id).and_then(|m| m.name.clone()))
                .unwrap_or_else(|| id.to_string());
            let mut f = DownloadedFile::with_content(
                // file_name здесь не важен — FileStore всё равно пересоберёт имя
                // из шаблона; оставим serviceName для отладки.
                format!("wb_doc_{id}"),
                downloaded.extension,
                downloaded.bytes,
            );
            f.source_id = Some(source_id);
            // Дата документа (creationTime → YYYY-MM-DD) из меты UI — для записи
            // document_date в каталог (фильтр периода Архива) и плейсхолдера {doc_date}.
            f.document_date = meta.get(id).and_then(|m| m.date.clone());
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
    #[serde(default)]
    date: Option<String>,
}

/// Согласует слово с числом: одна/две/пять форм. Используется для человекочитаемых
/// сообщений прогресса: num_words(1, "документ", "документа", "документов") → "документ",
/// num_words(2, ...) → "документа", num_words(5, ...) → "документов".
fn num_words(n: usize, one: &str, few: &str, many: &str) -> String {
    let n = n as u64;
    let last = n % 100;
    let last_digit = n % 10;
    let word = if last_digit == 1 && last != 11 {
        one
    } else if (2..=4).contains(&last_digit) && !(12..=14).contains(&last) {
        few
    } else {
        many
    };
    word.to_string()
}

/// Отрезает расширение (последний сегмент после `.`) из имени файла, если оно
/// похоже на настоящее расширение, а не на часть даты/числа. Правила для сегмента
/// после последней точки: непустой, ≤ 5 символов, только ASCII-буквы/цифры, и
/// хотя бы одна буква (чтобы не отрезать «.2026» из даты или «.0» из числа).
/// «Акт №123 от 26.07.2026.pdf» → «Акт №123 от 26.07.2026»,
/// «УПД №1001 от 01.07.2026» (без расширения) → без изменений.
/// Используется, чтобы не дублировать расширение: оно добавляется шаблоном
/// имени через {ext}, иначе получился бы «…pdf.zip».
fn strip_extension(name: &str) -> String {
    let Some(pos) = name.rfind('.') else { return name.to_string(); };
    if pos == 0 {
        return name.to_string();
    }
    let ext = &name[pos + 1..];
    let looks_like_ext = !ext.is_empty()
        && ext.len() <= 5
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
        && ext.chars().any(|c| c.is_ascii_alphabetic());
    if looks_like_ext {
        name[..pos].to_string()
    } else {
        name.to_string()
    }
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
        _progress: ProgressCallbackRef,
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
        let rows = [
            json!({"srid": "S1", "supplierArticle": "A1", "date": "2026-07-01T10:00:00"}),
            json!({"srid": "S2", "supplierArticle": "A2", "date": "2026-07-02"})
        ];
        let entries: Vec<DocumentEntry> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| entry_from_row(r, i, "wb.orders"))
            .collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "S1");
        assert_eq!(entries[1].display_name, "A2");
        assert_eq!(entries[0].date, chrono::NaiveDate::from_ymd_opt(2026, 7, 1));
    }

    #[test]
    fn entry_from_row_penalties() {
        // Удержания: id из nmId (число), display из subjectName, дата из dtBonus
        // отсутствует — берём rrDate.
        let row = json!({"nmId": 123, "subjectName": "Футболка", "rrDate": "2026-07-05"});
        let e = entry_from_row(&row, 0, "wb.deductions");
        assert_eq!(e.id, "123");
        assert_eq!(e.display_name, "Футболка");
        assert_eq!(e.date, chrono::NaiveDate::from_ymd_opt(2026, 7, 5));
    }

    #[test]
    fn entry_from_row_claims_prefers_claim_id() {
        // Заявки возврата: id — UUID заявки (поле id), а не srid продажи.
        let row = json!({
            "id": "0e4dd7cd-8b76-11ef-9f8a-000000000001",
            "srid": "S-777",
            "imt_name": "Кроссовки",
            "dt": "2026-08-10T12:00:00"
        });
        let e = entry_from_row(&row, 0, "wb.claims");
        assert_eq!(e.id, "0e4dd7cd-8b76-11ef-9f8a-000000000001");
        assert_eq!(e.display_name, "Кроссовки");
        assert_eq!(e.date, chrono::NaiveDate::from_ymd_opt(2026, 8, 10));
    }

    #[test]
    fn filter_from_params_month_end() {
        // Июль: 31 день; февраль 2026 (не високосный): 28 дней.
        let mut p = ReportParams::new();
        p.period = Some("2026-07".into());
        let f = filter_from_params(&p);
        assert_eq!(f.date_from, chrono::NaiveDate::from_ymd_opt(2026, 7, 1));
        assert_eq!(f.date_to, chrono::NaiveDate::from_ymd_opt(2026, 7, 31));

        p.period = Some("2026-02".into());
        let f = filter_from_params(&p);
        assert_eq!(f.date_to, chrono::NaiveDate::from_ymd_opt(2026, 2, 28));
    }

    #[test]
    fn antifraud_row_range_overlap() {
        // Недельные интервалы строк антифрода: пересечение с периодом.
        let week = json!({"dateFrom": "2026-07-01", "dateTo": "2026-07-07"});
        let (rf, rt) = row_date_range(&week).unwrap();
        assert_eq!(rf, chrono::NaiveDate::from_ymd_opt(2026, 7, 1).unwrap());
        assert_eq!(rt, chrono::NaiveDate::from_ymd_opt(2026, 7, 7).unwrap());
        // Период 05.07–11.07 пересекается с неделей 01–07.
        let from = chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap();
        let to = chrono::NaiveDate::from_ymd_opt(2026, 7, 11).unwrap();
        assert!(rf <= to && rt >= from);
        // Период 08–14.07 — не пересекается.
        let from2 = chrono::NaiveDate::from_ymd_opt(2026, 7, 8).unwrap();
        let to2 = chrono::NaiveDate::from_ymd_opt(2026, 7, 14).unwrap();
        assert!(!(rf <= to2 && rt >= from2));
    }

    #[test]
    fn claims_rows_filtered_by_dt() {
        // Локальный фильтр claims: строка без parsable dt сохраняется.
        let rows = [
            json!({"id": "c1", "dt": "2026-07-02T00:00:00"}),
            json!({"id": "c2", "dt": "2026-08-01"}),
            json!({"id": "c3"}),
        ];
        let from = chrono::NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        let to = chrono::NaiveDate::from_ymd_opt(2026, 7, 31).unwrap();
        let ids: Vec<&str> = rows
            .iter()
            .filter(|r| {
                r.get("dt")
                    .and_then(Value::as_str)
                    .and_then(parse_flexible_date)
                    .is_none_or(|d| from <= d && d <= to)
            })
            .map(|r| r.get("id").and_then(Value::as_str).unwrap_or("?"))
            .collect();
        assert_eq!(ids, vec!["c1", "c3"]);
    }

    #[test]
    fn strip_extension_basic() {
        assert_eq!(strip_extension("Акт №123 от 26.07.2026.pdf"), "Акт №123 от 26.07.2026");
        assert_eq!(strip_extension("doc.xml"), "doc");
    }

    #[test]
    fn strip_extension_no_extension() {
        // Нет точки — имя без изменений.
        assert_eq!(strip_extension("УПД №1001 от 01.07.2026"), "УПД №1001 от 01.07.2026");
        // Точка в начале (скрытый файл) — не считаем расширением.
        assert_eq!(strip_extension(".gitignore"), ".gitignore");
        // Пустая строка.
        assert_eq!(strip_extension(""), "");
    }

    #[test]
    fn strip_extension_multiple_dots() {
        // Отрезается только последний сегмент после последней точки.
        assert_eq!(strip_extension("archive.tar.gz"), "archive.tar");
    }
}
