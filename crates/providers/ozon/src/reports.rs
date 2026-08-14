//! Описания отчётов Ozon (спец. §2.2.1 — 10 отчётов через API) + out-of-scope.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use mdwf_core::{
    AcquisitionMode, Capabilities, CoreError, CoreResult, CancelToken, DownloadedFile,
    DownloaderKind, PeriodKind, ProgressCallbackRef, Report, ReportCategory, ReportDescriptor,
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
                label: "Номер кабинета (Client-Id)".into(),
                kind: AuthFieldKind::Number,
                required: true,
                placeholder: Some("1234567".into()),
                help_text: Some("Числовой идентификатор продавца из личного кабинета".into()),
                secret: false,
            },
            AuthField {
                id: "api_key".into(),
                label: "API-ключ".into(),
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

/// Описания всех отчётов Ozon (спец. §2.2.1). Все 21 отчёта, Period-режим.
///
/// Группы (сверено с docs.ozon.ru):
/// - Финансовые: realization, realization_posting, balance, buyout, compensation,
///   decompensation, mutual_settlement, cash_flow, accrual_postings, accrual_by_day.
/// - Реестры/документы: b2b_sales.
/// - Отчёты (create→code→/v1/report/info→файл): products, returns, postings,
///   discounted, warehouse_stock, placement_by_products, placement_by_supplies,
///   marked_products_sales.
/// - Аналитика: analytics_stocks, analytics_turnover.
///
/// НЕ включены (deprecated, отключаются 8 сент 2026 / заменены):
/// transaction_list, transaction_totals, stock_on_warehouses.
#[must_use]
/// Как строится отчёт: единственная точка правды для ФАБРИКИ. Раньше
/// дескрипторы и make_report были двумя раздельными списками по 21 arm с
/// дословно дублированными строками (type_id/display/category) — дрейф
/// ловился только тестом (случай «Декомпенсации» vs «Декомпенсации (штрафы)»).
#[derive(Clone, Copy)]
enum Dispatch {
    /// Прямой POST → JSON (OzonReport::period).
    Direct(&'static str),
    /// Async: create → code → /v1/report/info → файл (OzonAsyncReport).
    Async(&'static str),
    /// Inline sweep с пагинацией (OzonPaginatedReport).
    Paginated(&'static str, PaginationKind),
}

/// Форма параметров дескриптора (в таблице — данные, не код).
enum DefParams {
    None,
    DateRange,
    /// param_period_month("period", "Месяц (YYYY-MM)").
    MonthField,
    /// param_period_month("period", "День (YYYY-MM)").
    DayField,
    Text { id: &'static str, label: &'static str },
}

impl DefParams {
    fn build(&self) -> Vec<ReportParameter> {
        match self {
            Self::None => Vec::new(),
            Self::DateRange => vec![param_date_range(true)],
            Self::MonthField => vec![param_period_month("period", "Месяц (YYYY-MM)", true)],
            Self::DayField => vec![param_period_month("period", "День (YYYY-MM)", true)],
            Self::Text { id, label } => vec![param_text(id, label, true)],
        }
    }
}

/// ОПРЕДЕЛЕНИЕ ОТЧЁТА: дескриптор (данные для UI) + диспетчеризация (как
/// скачивать). Единая таблица: all_report_descriptors() и make_report()
/// генерируются из неё — 21 определение вместо 2x21 дублированных arm.
struct ReportDef {
    type_id: &'static str,
    display_name: &'static str,
    category: ReportCategory,
    period_kind: PeriodKind,
    description: &'static str,
    params: DefParams,
    max_range_days: Option<u32>,
    dispatch: Dispatch,
}

impl ReportDef {
    fn descriptor(&self) -> ReportDescriptor {
        ReportDescriptor {
            type_id: self.type_id.into(),
            display_name: self.display_name.into(),
            category: self.category,
            acquisition_mode: AcquisitionMode::Period,
            downloader_kind: DownloaderKind::Api,
            parameters: self.params.build(),
            period_kind: self.period_kind,
            description: Some(self.description.into()),
            max_range_days: self.max_range_days,
        }
    }
}

/// Единая таблица всех 21 отчётов Ozon (сверено с docs.ozon.ru).
static REPORT_DEFS: &[ReportDef] = &[
    // --- Финансовые (Period, прямой JSON) ---
    ReportDef {
        type_id: "ozon.realization",
        display_name: "Отчёт о реализации (месячный)",
        category: ReportCategory::Finance,
        period_kind: PeriodKind::Month,
        description: "Финансовый отчёт о реализации за месяц. Строго месячный — за интервал соберём по месяцам.",
        params: DefParams::MonthField,
        max_range_days: None,
        dispatch: Dispatch::Direct("/v2/finance/realization"),
    },
    // Серверный Excel от Ozon (create→code→info→.xlsx), тело {month, year} —
    // готовый xlsx с шапкой «Отчет о реализации №…» (как в личном кабинете).
    ReportDef {
        type_id: "ozon.realization_posting",
        display_name: "Отчёт о реализации (позаказный)",
        category: ReportCategory::Finance,
        period_kind: PeriodKind::Month,
        description: "Позаказный отчёт о реализации за месяц (async). Строго месячный.",
        params: DefParams::DateRange,
        max_range_days: None,
        dispatch: Dispatch::Async("/v1/report/realization/posting/create"),
    },
    ReportDef {
        type_id: "ozon.buyout",
        display_name: "Выкупы маркетплейсом (ЕАЭС)",
        category: ReportCategory::Finance,
        period_kind: PeriodKind::Range,
        description: "Выкупы маркетплейсом в ЕАЭС за период (диапазон ≤31 дня).",
        params: DefParams::DateRange,
        max_range_days: Some(31),
        dispatch: Dispatch::Direct("/v1/finance/products/buyout"),
    },
    ReportDef {
        type_id: "ozon.balance",
        display_name: "Баланс",
        category: ReportCategory::Finance,
        period_kind: PeriodKind::Range,
        description: "Баланс кошелька за период (диапазон ≤30 дней).",
        params: DefParams::None,
        max_range_days: Some(30),
        dispatch: Dispatch::Direct("/v1/finance/balance"),
    },
    ReportDef {
        type_id: "ozon.cash_flow",
        display_name: "Финансовый отчёт (движение средств)",
        category: ReportCategory::Finance,
        period_kind: PeriodKind::Range,
        description: "Движение средств по датам за период (диапазон, без жёсткого лимита дней).",
        params: DefParams::DateRange,
        max_range_days: None,
        dispatch: Dispatch::Paginated(
            "/v1/finance/cash-flow-statement/list",
            PaginationKind::CashFlow,
        ),
    },
    // accrual — бета-методы начислений (замена deprecated transaction-list).
    ReportDef {
        type_id: "ozon.accrual_by_day",
        display_name: "Начисления за день",
        category: ReportCategory::Finance,
        period_kind: PeriodKind::Day,
        description: "Начисления по дням выбранного месяца (цикл по всем дням месяца).",
        params: DefParams::DayField,
        max_range_days: None,
        dispatch: Dispatch::Paginated("/v1/finance/accrual/by-day", PaginationKind::LastId),
    },
    // posting_numbers[] (1–200); auto-fill FBO+FBS, затем батчинг ≤200.
    ReportDef {
        type_id: "ozon.accrual_postings",
        display_name: "Начисления по отправлениям",
        category: ReportCategory::Finance,
        period_kind: PeriodKind::None,
        description: "Начисления по отправлениям (номера подставляются автоматически за период).",
        params: DefParams::Text {
            id: "posting_numbers",
            label: "Номера отправлений (через запятую, 1–200)",
        },
        max_range_days: None,
        dispatch: Dispatch::Paginated(
            "/v1/finance/accrual/postings",
            PaginationKind::AccrualPostings,
        ),
    },
    // --- Штрафы/компенсации (async) ---
    ReportDef {
        type_id: "ozon.compensation",
        display_name: "Компенсации",
        category: ReportCategory::Finance,
        period_kind: PeriodKind::Month,
        description: "Компенсации за месяц (async). Строго месячный.",
        params: DefParams::DateRange,
        max_range_days: None,
        dispatch: Dispatch::Async("/v1/finance/compensation"),
    },
    ReportDef {
        type_id: "ozon.decompensation",
        display_name: "Декомпенсации (штрафы/антифрод)",
        category: ReportCategory::Penalties,
        period_kind: PeriodKind::Month,
        description: "Декомпенсации и штрафы за месяц (async). Строго месячный.",
        params: DefParams::DateRange,
        max_range_days: None,
        dispatch: Dispatch::Async("/v1/finance/decompensation"),
    },
    // --- Реестры/документы (async) ---
    ReportDef {
        type_id: "ozon.b2b_sales",
        display_name: "Продажи юрлицам (PDF)",
        category: ReportCategory::Documents,
        period_kind: PeriodKind::Month,
        description: "Реестр продаж юрлицам за месяц, PDF (async). Строго месячный.",
        params: DefParams::DateRange,
        max_range_days: None,
        dispatch: Dispatch::Async("/v1/finance/document-b2b-sales"),
    },
    ReportDef {
        type_id: "ozon.mutual_settlement",
        display_name: "Отчёт о взаиморасчётах",
        category: ReportCategory::Finance,
        period_kind: PeriodKind::Month,
        description: "Отчёт о взаиморасчётах за месяц (async). Строго месячный.",
        params: DefParams::DateRange,
        max_range_days: None,
        dispatch: Dispatch::Async("/v1/finance/mutual-settlement"),
    },
    // --- Seller-отчёты (async: create→code→/v1/report/info→файл) ---
    ReportDef {
        type_id: "ozon.products",
        display_name: "Отчёт по товарам",
        category: ReportCategory::Documents,
        period_kind: PeriodKind::None,
        description: "Отчёт по товарам (без привязки к периоду, async).",
        params: DefParams::None,
        max_range_days: None,
        dispatch: Dispatch::Async("/v1/report/products/create"),
    },
    // /v1/returns/list — возвраты FBO+FBS одним списком (все статусы),
    // без обязательного filter.status; пагинация last_id + has_next, limit≤500.
    ReportDef {
        type_id: "ozon.returns",
        display_name: "Отчёт о возвратах",
        category: ReportCategory::Documents,
        period_kind: PeriodKind::Range,
        description: "Отчёт о возвратах за период (диапазон дат).",
        params: DefParams::DateRange,
        max_range_days: None,
        dispatch: Dispatch::Paginated("/v1/returns/list", PaginationKind::ReturnsList),
    },
    ReportDef {
        type_id: "ozon.postings",
        display_name: "Отчёт об отправлениях",
        category: ReportCategory::Documents,
        period_kind: PeriodKind::Range,
        description: "Отчёт об отправлениях за период (диапазон дат, async).",
        params: DefParams::DateRange,
        max_range_days: None,
        dispatch: Dispatch::Async("/v1/report/postings/create"),
    },
    ReportDef {
        type_id: "ozon.discounted",
        display_name: "Отчёт об уценённых товарах",
        category: ReportCategory::Documents,
        period_kind: PeriodKind::None,
        description: "Отчёт об уценённых товарах (без привязки к периоду, async).",
        params: DefParams::None,
        max_range_days: None,
        dispatch: Dispatch::Async("/v1/report/discounted/create"),
    },
    // ID складов — auto-fill через /v2/warehouse/list, если не переданы.
    ReportDef {
        type_id: "ozon.warehouse_stock",
        display_name: "Остатки на FBS-складе",
        category: ReportCategory::Documents,
        period_kind: PeriodKind::None,
        description: "Остатки на FBS-складе (ID складов подставляются автоматически).",
        params: DefParams::Text {
            id: "warehouse_ids",
            label: "ID складов (через запятую)",
        },
        max_range_days: None,
        dispatch: Dispatch::Async("/v1/report/warehouse/stock"),
    },
    ReportDef {
        type_id: "ozon.placement_by_products",
        display_name: "Стоимость размещения по товарам",
        category: ReportCategory::Documents,
        period_kind: PeriodKind::Range,
        description: "Стоимость размещения по товарам за период (диапазон ≤31 дня, async).",
        params: DefParams::DateRange,
        max_range_days: Some(31),
        dispatch: Dispatch::Async("/v1/report/placement/by-products/create"),
    },
    ReportDef {
        type_id: "ozon.placement_by_supplies",
        display_name: "Стоимость размещения по поставкам",
        category: ReportCategory::Documents,
        period_kind: PeriodKind::Range,
        description: "Стоимость размещения по поставкам за период (диапазон ≤31 дня, async).",
        params: DefParams::DateRange,
        max_range_days: Some(31),
        dispatch: Dispatch::Async("/v1/report/placement/by-supplies/create"),
    },
    ReportDef {
        type_id: "ozon.marked_products_sales",
        display_name: "Продажи товаров с маркировкой",
        category: ReportCategory::Documents,
        period_kind: PeriodKind::Range,
        description: "Продажи маркированных товаров за период (диапазон дат, async).",
        params: DefParams::DateRange,
        max_range_days: None,
        dispatch: Dispatch::Async("/v1/report/marked-products-sales/create"),
    },
    // --- Аналитика остатков ---
    // skus[] обязательны ≤100; auto-fill /v3/product/list + батчинг + pacing.
    ReportDef {
        type_id: "ozon.analytics_stocks",
        display_name: "Аналитика по остаткам",
        category: ReportCategory::Finance,
        period_kind: PeriodKind::None,
        description: "Аналитика по остаткам (SKU подставляются автоматически, без даты — срез).",
        params: DefParams::Text {
            id: "skus",
            label: "SKU (через запятую, ≤100)",
        },
        max_range_days: None,
        dispatch: Dispatch::Paginated("/v1/analytics/stocks", PaginationKind::Skus),
    },
    ReportDef {
        type_id: "ozon.analytics_turnover",
        display_name: "Оборачиваемость товара",
        category: ReportCategory::Finance,
        period_kind: PeriodKind::None,
        description: "Оборачиваемость товара (без привязки к периоду — срез).",
        params: DefParams::None,
        max_range_days: None,
        dispatch: Dispatch::Paginated("/v1/analytics/turnover/stocks", PaginationKind::Offset),
    },
];

#[must_use]
pub fn all_report_descriptors() -> Vec<ReportDescriptor> {
    REPORT_DEFS.iter().map(ReportDef::descriptor).collect()
}

// --- Хелперы для построения дескрипторов ---

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

/// Текстовый параметр (CSV значений через запятую) — для warehouse_ids,
/// posting_numbers, skus. `id` — ключ в params.values.
fn param_text(id: &str, label: &str, required: bool) -> ReportParameter {
    ReportParameter {
        id: id.into(),
        label: label.into(),
        kind: ReportParameterKind::Text,
        required,
        default: None,
    }
}

/// Фабрика отчётов: возвращает `ReportRef` по `type_id`.
///
/// Каждая реализация делегирует HTTP-вызовы в `OzonHttpClient`. Парсинг
/// специфичных полей отчётов будет уточняться по мере интеграции с реальным API.
pub fn make_report(type_id: &str, client: OzonHttpClient) -> CoreResult<ReportRef> {
    // Фабрика генерируется из ЕДИНОЙ таблицы REPORT_DEFS: type_id → impl+эндпоинт.
    let Some(def) = REPORT_DEFS.iter().find(|d| d.type_id == type_id) else {
        return Err(CoreError::ReportTypeNotSupported(type_id.to_string()));
    };
    let report: ReportRef = match def.dispatch {
        Dispatch::Direct(ep) => Arc::new(OzonReport::period(
            def.type_id,
            def.display_name,
            def.category,
            client,
            ep,
        )),
        Dispatch::Async(ep) => Arc::new(OzonAsyncReport::new(
            def.type_id,
            def.display_name,
            def.category,
            client,
            ep,
        )),
        Dispatch::Paginated(ep, kind) => Arc::new(OzonPaginatedReport::new(
            def.type_id,
            def.display_name,
            def.category,
            client,
            ep,
            kind,
        )),
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
        self.category
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
        // Конвертация ответа в Excel (.xlsx) с русскими заголовками колонок
        // (для buyout, balance, realization). realization_posting использует
        // серверный Excel через OzonAsyncReport.
        let content = crate::xlsx::workbook_bytes(&self.type_id, &json)?;
        let period = params.period.clone().unwrap_or_else(|| "current".into());
        Ok(vec![DownloadedFile::with_content(
            format!("{}_{}.xlsx", self.type_id, period),
            "xlsx",
            content,
        )])
    }
}

/// Строит тело запроса для download из параметров.
///
/// Конвертирует period `"YYYY-MM"` в диапазон дат `(date_from, date_to)` формата
/// `YYYY-MM-DD`: первый день месяца .. последний день месяца. Применяется когда
/// UI/CLI передаёт только period (без явных date_from/date_to в values) — для
/// отчётов, требующих диапазон дат (balance, buyout, placement_*, returns,
/// postings, cash_flow, marked_products_sales).
/// Возвращает (None, None) если period пуст или невалиден.
fn period_to_date_range(period: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(p) = period else {
        return (None, None);
    };
    let Some((y, m)) = date_format::parse_year_month(p) else {
        return (None, None);
    };
    let Some(first) = chrono::NaiveDate::from_ymd_opt(y, m, 1) else {
        return (None, None);
    };
    let Some(last) = first
        .checked_add_months(chrono::Months::new(1))
        .and_then(|d| d.pred_opt())
    else {
        return (None, None);
    };
    (
        Some(first.format("%Y-%m-%d").to_string()),
        Some(last.format("%Y-%m-%d").to_string()),
    )
}

/// Возвращает список всех дней месяца в формате `YYYY-MM-DD` для period `"YYYY-MM"`.
/// Для `accrual_by_day`: дока требует date=YYYY-MM-DD (один конкретный день),
/// `last_id` пагинирует записи внутри дня. Чтобы получить весь месяц — перебираем
/// все дни. Дни ранее 2022-01-01 отрезаются (дока: самая ранняя дата начислений).
fn month_days(period: Option<&str>) -> Vec<String> {
    let Some(p) = period else {
        return Vec::new();
    };
    let Some((y, m)) = date_format::parse_year_month(p) else {
        return Vec::new();
    };
    let Some(first) = chrono::NaiveDate::from_ymd_opt(y, m, 1) else {
        return Vec::new();
    };
    let Some(last) = first
        .checked_add_months(chrono::Months::new(1))
        .and_then(|d| d.pred_opt())
    else {
        return Vec::new();
    };
    let min_date = chrono::NaiveDate::from_ymd_opt(2022, 1, 1).unwrap_or(first);
    let mut days = Vec::new();
    let mut cur = first.max(min_date);
    while cur <= last {
        days.push(cur.format("%Y-%m-%d").to_string());
        cur = match cur.succ_opt() {
            Some(n) => n,
            None => break,
        };
    }
    days
}

/// Определяет расширение файла по сигнатуре (magic bytes) скачанного контента.
/// Для async-отчётов Ozon: сервер отдаёт разные форматы — настоящий xlsx (это
/// ZIP, сигнатура `PK`), CSV (products/realization_posting), PDF (b2b_sales),
/// JSON, XML. Расширение обязано соответствовать реальному формату, иначе файл
/// с `.xlsx` окажется CSV/JSON/PDF внутри — это обман пользователя («такого не
/// должно быть»).
fn detect_format(bytes: &[u8]) -> &'static str {
    // ZIP-архив: xlsx/zip/docx — для Ozon async-отчётов это xlsx.
    if bytes.starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
        return "xlsx";
    }
    // PDF (b2b_sales): сигнатура `%PDF`.
    if bytes.starts_with(b"%PDF") {
        return "pdf";
    }
    // Пропускаем UTF-8 BOM (Ozon products отдаёт CSV с BOM).
    let b = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match b.first() {
        Some(b'{' | b'[') => "json",
        Some(b'<') => "xml",
        // Текстовый — скорее всего CSV (Ozon products: «;» с BOM,
        // realization_posting: «,»).
        _ => "csv",
    }
}

/// Формат тела зависит от type_id — разные эндпоинты Ozon ждут разные схемы
/// (сверено с docs.ozon.ru):
/// - `month`+`year` как integer: realization, realization_posting.
/// - `date` как строка YYYY-MM: compensation, decompensation, b2b_sales,
///   mutual_settlement (async-отчёты).
/// - `date_from`+`date_to` как YYYY-MM-DD: balance, buyout, placement_*.
/// - `filter{date_from,to}` ISO datetime: returns, postings (filter.processed_at_*).
/// - `date{from,to}` ISO datetime: marked_products_sales, cash_flow.
/// - `warehouseId[]` camelCase: warehouse_stock.
/// - `posting_numbers[]` (обязательны): accrual_postings.
/// - `date` YYYY-MM-DD + `last_id` (sweep): accrual_by_day.
/// - `limit`/`offset`/`skus[]`: analytics_turnover, analytics_stocks.
fn build_download_body(type_id: &str, params: &ReportParams) -> serde_json::Value {
    let mut body = json!({});
    let period = params.period.as_deref();
    // date_from/date_to: из params.values (UI с полями дат) ИЛИ из period
    // (CLI/расписание — period YYYY-MM → первый..последний день месяца). Раньше
    // при отсутствии values поля дат молча пропускались → тело без дат → 4xx.
    let (date_from, date_to) = match (
        params.get("date_from").map(str::to_string),
        params.get("date_to").map(str::to_string),
    ) {
        (Some(df), Some(dt)) => (Some(df), Some(dt)),
        _ => period_to_date_range(period),
    };

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
        // date_from + date_to как YYYY-MM-DD: balance, buyout, placement_*.
        "ozon.balance" | "ozon.buyout" | "ozon.placement_by_products"
        | "ozon.placement_by_supplies" => {
            if let Some(df) = &date_from {
                body["date_from"] = json!(df);
            }
            if let Some(dt) = &date_to {
                body["date_to"] = json!(dt);
            }
        }
        // date как строка YYYY-MM (async-отчёты, тело date=YYYY-MM).
        "ozon.compensation" | "ozon.decompensation" | "ozon.b2b_sales"
        | "ozon.mutual_settlement" => {
            if let Some(p) = period {
                body["date"] = json!(p);
            }
        }
        // --- Async-отчёты «create → code → /v1/report/info → файл» ---
        // products/create и discounted/create: тело пустое (фильтры опциональны,
        // добавятся из values ниже) — попадают в общую ветку `_`.
        // returns: /v1/returns/list — filter.logistic_return_date {time_from,time_to} (ISO).
        // Намеренно НЕ шлём filter.status и filter.return_schema → сервер вернёт ВСЕ
        // возвраты (все статусы, FBO+FBS). Это и есть «отчёт по всем возвратам».
        // limit (≤500) и last_id проставляются в sweep (PaginationKind::ReturnsList).
        "ozon.returns" => {
            let mut date = serde_json::Map::new();
            if let Some(df) = &date_from {
                if let Some(iso) = date_format::date_only_to_iso(df, false) {
                    date.insert("time_from".into(), json!(iso));
                }
            }
            if let Some(dt) = &date_to {
                if let Some(iso) = date_format::date_only_to_iso(dt, true) {
                    date.insert("time_to".into(), json!(iso));
                }
            }
            let mut filter = serde_json::Map::new();
            filter.insert("logistic_return_date".into(), json!(date));
            body["filter"] = json!(filter);
        }
        // postings/create: filter{processed_at_from,to} — ISO datetime, language.
        "ozon.postings" => {
            let mut filter = serde_json::Map::new();
            if let Some(df) = &date_from {
                if let Some(iso) = date_format::date_only_to_iso(df, false) {
                    filter.insert("processed_at_from".into(), json!(iso));
                }
            }
            if let Some(dt) = &date_to {
                if let Some(iso) = date_format::date_only_to_iso(dt, true) {
                    filter.insert("processed_at_to".into(), json!(iso));
                }
            }
            body["filter"] = json!(filter);
            body["language"] = json!("DEFAULT");
        }
        // warehouse/stock: warehouseId[] (camelCase, значения-строки), language.
        // Идентификаторы складов передаются в values["warehouse_ids"] (CSV).
        "ozon.warehouse_stock" => {
            body["language"] = json!("DEFAULT");
            if let Some(csv) = params.get("warehouse_ids") {
                let ids: Vec<&str> = csv.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
                body["warehouseId"] = json!(ids);
            }
        }
        // marked-products-sales/create: date{from,to} — date-only YYYY-MM-DD
        // (сервер требует ровно 10 символов; ISO datetime с T..Z — 24 символа, отвергается).
        "ozon.marked_products_sales" => {
            let mut date = serde_json::Map::new();
            if let Some(df) = &date_from {
                date.insert("from".into(), json!(df));
            }
            if let Some(dt) = &date_to {
                date.insert("to".into(), json!(dt));
            }
            body["date"] = json!(date);
        }
        // --- Inline-отчёты (списки с пагинацией, sweep внутри download) ---
        // cash-flow-statement/list: date{from,to} ISO, page/page_size (page в sweep).
        "ozon.cash_flow" => {
            let mut date = serde_json::Map::new();
            if let Some(df) = &date_from {
                if let Some(iso) = date_format::date_only_to_iso(df, false) {
                    date.insert("from".into(), json!(iso));
                }
            }
            if let Some(dt) = &date_to {
                if let Some(iso) = date_format::date_only_to_iso(dt, true) {
                    date.insert("to".into(), json!(iso));
                }
            }
            body["date"] = json!(date);
            body["page_size"] = json!(1000);
        }
        // analytics/turnover/stocks: limit/offset (offset в sweep).
        "ozon.analytics_turnover" => {
            body["limit"] = json!(1000);
        }
        // analytics/stocks: skus[] (≤100 за запрос; батчинг в download).
        "ozon.analytics_stocks" => {
            if let Some(csv) = params.get("skus") {
                let skus: Vec<&str> = csv.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
                body["skus"] = json!(skus);
            }
        }
        // --- Возвращаемые accrual (ранее удалены, реализованы по доке) ---
        // accrual/postings: posting_numbers[] (1–200, обязательны).
        "ozon.accrual_postings" => {
            if let Some(csv) = params.get("posting_numbers") {
                let nums: Vec<&str> = csv.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
                body["posting_numbers"] = json!(nums);
            }
        }
        // accrual/by-day: date YYYY-MM-DD, last_id (sweep в download).
        "ozon.accrual_by_day" => {
            // date — из period или date_from (один конкретный день).
            let day = period.or(date_from.as_deref()).unwrap_or("current");
            body["date"] = json!(day);
        }
        _ => {}
    }

    // ids — для Browsable-режима (если есть; Ozon сейчас не использует, но оставлено
    // для совместимости с UI, который шлёт ids для wb-стиля).
    if let Some(ids) = params.get("ids") {
        body["ids"] = json!(ids.split(',').collect::<Vec<_>>());
    }
    // Произвольные параметры из values (кроме служебных ключей, уже учтённых выше).
    for (k, v) in &params.values {
        if !matches!(
            k.as_str(),
            "ids" | "date_from" | "date_to" | "warehouse_ids" | "skus" | "posting_numbers"
        ) {
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
        self.category
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
        cancel: CancelToken,
    ) -> CoreResult<Vec<DownloadedFile>> {
        // Параметры поллинга /v1/report/info (шаг 2): async-отчёты Ozon генерируются
        // не мгновенно — waiting/processing это нормальные промежуточные статусы.
        const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
        const POLL_MAX_ATTEMPTS: usize = 120; // ~10 минут максимум.

        // Авто-fill: для warehouse_stock, если ID складов не переданы явно —
        // получаем их через /v2/warehouse/list (склады FBS/rFBS продавца).
        let params = if self.type_id == "ozon.warehouse_stock"
            && params.get("warehouse_ids").map(str::is_empty).unwrap_or(true)
        {
            match self.client.fetch_warehouse_ids(auth).await {
                Ok(ids) if !ids.is_empty() => {
                    let p = params.clone().with("warehouse_ids", ids.join(","));
                    tracing::info!(
                        type_id = %self.type_id,
                        count = ids.len(),
                        "warehouse_ids auto-filled из /v2/warehouse/list"
                    );
                    p
                }
                Ok(_) => {
                    // Список FBS/rFBS-складов пуст → отчёт «Остатки на FBS-складе»
                    // неприменим. Возвращаем понятную ошибку вместо молчаливой
                    // отправки запроса без warehouseId (→ 4xx «at least 1 item»).
                    return Err(CoreError::Internal(
                        "у продавца нет FBS/rFBS-складов ( /v2/warehouse/list вернул пустой список) \
                         — отчёт «Остатки на FBS-складе» неприменим. \
                         Если используете FBO — отчёт недоступен для вашей схемы работы."
                            .into(),
                    ));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "не удалось получить warehouse_ids; пробуем без них");
                    params.clone()
                }
            }
        } else {
            params.clone()
        };

        // Шаг 1: запрос отчёта → получаем code.
        let body = build_download_body(&self.type_id, &params);
        let resp = self.client.post(self.endpoint, &body, auth).await?;

        // Извлекаем code. Большинство create-эндпоинтов возвращают {result:{code}},
        // но часть — {code} без result-обёртки (discounted, warehouse_stock,
        // placement/by-products, placement/by-supplies). Сверено с docs.ozon.ru.
        let code = resp
            .get("result")
            .and_then(|r| r.get("code"))
            .and_then(|c| c.as_str())
            .or_else(|| resp.get("code").and_then(|c| c.as_str()))
            .ok_or_else(|| {
                CoreError::Internal(format!(
                    "Ozon {}: ответ не содержит result.code/code",
                    self.type_id
                ))
            })?;

        // Шаг 2: поллинг /v1/report/info до терминального статуса (success/failed).
        // Дока: waiting/processing — нормальные промежуаточные статусы. Раньше код
        // возвращал ошибку при первом waiting — теперь ждём с интервалом и таймаутом.
        let mut attempts = 0usize;
        let file_url = loop {
            // Отмена пользователем (кнопка «Отмена») — выходим без долгого ожидания.
            if cancel.is_cancelled() {
                return Err(CoreError::Cancelled);
            }
            attempts += 1;
            let report_info = self.client.post_report_info(code, auth).await?;
            let result = report_info
                .get("result")
                .ok_or_else(|| CoreError::Protocol("report/info: нет result".into()))?;
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
                "success" => {
                    let url = result
                        .get("file")
                        .and_then(|f| f.as_str())
                        .ok_or_else(|| {
                            CoreError::Internal(format!(
                                "Ozon {}: report/info вернул success, но нет ссылки file",
                                self.type_id
                            ))
                        })?;
                    break url.to_string();
                }
                // waiting | processing | пусто | неизвестное — продолжаем поллинг.
                _ => {
                    if attempts >= POLL_MAX_ATTEMPTS {
                        return Err(CoreError::Internal(format!(
                            "Ozon {}: отчёт не сгенерировался за {} попыток (статус {status})",
                            self.type_id, attempts
                        )));
                    }
                    tracing::debug!(
                        type_id = %self.type_id,
                        attempt = attempts,
                        status,
                        "отчёт ещё генерируется, ждём"
                    );
                    tokio::time::sleep(POLL_INTERVAL).await;
                }
            }
        };

        // Шаг 3: скачиваем файл по ссылке. Расширение определяем по РЕАЛЬНОМУ
        // формату серверного файла (magic bytes), а не захардкоженное «xlsx» —
        // Ozon отдаёт разные форматы: настоящий xlsx (compensation и др.),
        // CSV (products, realization_posting), PDF (b2b_sales). Иначе .xlsx с
        // CSV/PDF внутри = обман.
        let bytes = self.client.download_file(&file_url).await?;
        let period = params.period.clone().unwrap_or_else(|| "current".into());
        let ext = detect_format(&bytes);
        Ok(vec![DownloadedFile::with_content(
            format!("{}_{}.{}", self.type_id, period, ext),
            ext,
            bytes,
        )])
    }
}

// =========================================================================
// OzonPaginatedReport — inline-отчёты со списками и пагинацией.
//
// Некоторые эндпоинты Ozon возвращают данные сразу (не через code/file), но
// постранично. Этот тип делает sweep всех страниц и собирает результат в один
// JSON-файл. Сверено с docs.ozon.ru:
// - cash-flow-statement/list: {result:{cash_flows, page_count}} — page-based.
// - analytics/turnover/stocks: {items} — offset/limit.
// - accrual/by-day: {accruals, last_id} — last_id (курсор).
//
// Защита от бесконечного цикла: MAX_PAGES (200 по образцу WB-документов).
// =========================================================================

/// Стратегия пагинации эндпоинта (сверено с докой каждого метода).
#[derive(Clone, Copy)]
enum PaginationKind {
    /// /v1/finance/cash-flow-statement/list: `{result:{cash_flows:[...], page_count:N}}`.
    /// Запрос: `{date, page, page_size}`. Цикл по page=1..=page_count.
    CashFlow,
    /// /v1/analytics/turnover/stocks: `{items:[...]}`. Запрос: `{limit, offset}`.
    /// Цикл по offset += limit, пока items непустой.
    Offset,
    /// /v1/finance/accrual/by-day: `{accruals:[...], last_id:"..."}`. Запрос:
    /// `{date, last_id}`. Цикл: last_id из ответа подставляется в следующий запрос,
    /// пока accruals непустой И last_id меняется.
    LastId,
    /// /v1/analytics/stocks: `{items:[...]}`. Запрос `{skus:[...]}` ≤100 за раз.
    /// Если SKU не переданы — auto-fill через /v3/product/list (все товары
    /// продавца), затем батчинг ≤100 SKU на запрос. Результат агрегируется из
    /// всех батчей.
    Skus,
    /// /v1/returns/list: `{returns:[...], has_next:bool}`. Запрос:
    /// `{filter:{logistic_return_date:{time_from,time_to}}, limit≤500, last_id}`.
    /// last_id (int64) — курсор (id последнего возврата страницы). Без filter.status
    /// и return_schema → все статусы и схемы (FBO+FBS). Цикл пока has_next=true.
    ReturnsList,
    /// /v1/finance/accrual/postings: `{posting_accruals:[...]}`. Запрос:
    /// `{posting_numbers:[...]}` (1–200, обязательны). Если не переданы — auto-fill
    /// через /v2/posting/fbo/list (номера отправлений за период), затем батчинг ≤200.
    /// Ответ: `posting_accruals[]` (денормализация в xlsx). Агрегация из всех батчей.
    AccrualPostings,
}

/// Максимальное число страниц/итераций sweep (защита от бесконечного цикла).
const MAX_PAGES: u32 = 200;

pub struct OzonPaginatedReport {
    type_id: String,
    display_name: String,
    category: ReportCategory,
    client: OzonHttpClient,
    endpoint: &'static str,
    kind: PaginationKind,
}

impl OzonPaginatedReport {
    /// Конструктор приватный (тип internal, как OzonReport/OzonAsyncReport) —
    /// используется только из `make_report` в этом же крейте.
    #[must_use]
    fn new(
        type_id: &str,
        display_name: &str,
        category: ReportCategory,
        client: OzonHttpClient,
        endpoint: &'static str,
        kind: PaginationKind,
    ) -> Self {
        Self {
            type_id: type_id.into(),
            display_name: display_name.into(),
            category,
            client,
            endpoint,
            kind,
        }
    }
}

#[async_trait]
impl Report for OzonPaginatedReport {
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
        cancel: CancelToken,
    ) -> CoreResult<Vec<DownloadedFile>> {
        let mut collected = serde_json::json!([]);
        match self.kind {
            PaginationKind::CashFlow => {
                // Цикл по page=1..=page_count, собираем result.cash_flows.
                let mut page = 1u32;
                loop {
                    if cancel.is_cancelled() {
                        return Err(CoreError::Cancelled);
                    }
                    let mut body = build_download_body(&self.type_id, params);
                    body["page"] = json!(page);
                    let resp = self.client.post(self.endpoint, &body, auth).await?;
                    let result = resp.get("result").cloned().unwrap_or(json!({}));
                    if let Some(rows) = result.get("cash_flows").and_then(|v| v.as_array()) {
                        if let Some(arr) = collected.as_array_mut() {
                            arr.extend(rows.iter().cloned());
                        }
                    }
                    let page_count = result
                        .get("page_count")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0) as u32;
                    if page_count == 0 || page >= page_count || page >= MAX_PAGES {
                        break;
                    }
                    page += 1;
                }
            }
            PaginationKind::Offset => {
                // Цикл по offset += limit, пока items непустой.
                let limit = 1000u64;
                let mut offset = 0u64;
                loop {
                    if cancel.is_cancelled() {
                        return Err(CoreError::Cancelled);
                    }
                    let mut body = build_download_body(&self.type_id, params);
                    body["limit"] = json!(limit);
                    body["offset"] = json!(offset);
                    let resp = self.client.post(self.endpoint, &body, auth).await?;
                    let items = resp.get("items").and_then(|v| v.as_array());
                    let n = items.map_or(0, Vec::len) as u64;
                    if let Some(rows) = items {
                        if let Some(arr) = collected.as_array_mut() {
                            arr.extend(rows.iter().cloned());
                        }
                    }
                    if n == 0 || n < limit {
                        break;
                    }
                    offset += limit;
                    if offset / limit >= u64::from(MAX_PAGES) {
                        break;
                    }
                }
            }
            PaginationKind::LastId => {
                // accrual/by-day: дока требует date=YYYY-MM-DD (один день), last_id
                // пагинирует записи внутри дня. Перебираем ВСЕ дни выбранного месяца,
                // по каждому — цикл по last_id.
                let days = month_days(params.period.as_deref());
                for day in days {
                    let mut last_id = String::new();
                    let mut iter = 0u32;
                    loop {
                        if cancel.is_cancelled() {
                            return Err(CoreError::Cancelled);
                        }
                        let mut body = build_download_body(&self.type_id, params);
                        body["date"] = json!(day);
                        body["last_id"] = json!(last_id);
                        let resp = self.client.post(self.endpoint, &body, auth).await?;
                        let accruals = resp.get("accruals").and_then(|v| v.as_array());
                        let n = accruals.map_or(0, Vec::len);
                        if let Some(rows) = accruals {
                            if let Some(arr) = collected.as_array_mut() {
                                arr.extend(rows.iter().cloned());
                            }
                        }
                        let next_last_id = resp
                            .get("last_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        // Выход: пустой ответ, last_id не изменился, лимит итераций.
                        if n == 0 || next_last_id.is_empty() || next_last_id == last_id {
                            break;
                        }
                        last_id = next_last_id;
                        iter += 1;
                        if iter >= MAX_PAGES {
                            break;
                        }
                    }
                }
            }
            PaginationKind::Skus => {
                // /v1/analytics/stocks: skus[] обязательны и ≤100 за запрос.
                // Если SKU не переданы явно (GUI / CLI без --skus) — auto-fill
                // через /v3/product/list (все товары продавца). Затем батчинг ≤100.
                const STOCKS_BATCH_DELAY: std::time::Duration = std::time::Duration::from_secs(1);
                let skus: Vec<String> = match params.get("skus") {
                    Some(csv) if !csv.is_empty() => csv
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .collect(),
                    _ => match self.client.fetch_skus(auth).await {
                        Ok(v) if !v.is_empty() => {
                            tracing::info!(
                                type_id = %self.type_id,
                                count = v.len(),
                                "skus auto-filled из /v3/product/list"
                            );
                            v
                        }
                        Ok(_) => {
                            // Товаров нет — отчёт «Аналитика по остаткам» нечего
                            // строить. Понятная ошибка вместо 4xx «at least 1 item».
                            return Err(CoreError::Internal(
                                "у продавца нет товаров ( /v3/product/list вернул пустой список) \
                                 — отчёт «Аналитика по остаткам» нечего строить."
                                    .into(),
                            ));
                        }
                        Err(e) => {
                            return Err(CoreError::Internal(format!(
                                "не удалось получить список SKU через /v3/product/list: {e}"
                            )));
                        }
                    },
                };
                // Батчинг ≤100 SKU на запрос (лимит /v1/analytics/stocks). Между
                // батчами — пауза STOCKS_BATCH_DELAY: метод имеет жёсткий per-second
                // rate limit, и без pacing каталог в ~2840 SKU (29 батчей)
                // триггерит 429 «You have reached request rate limit per second».
                for (i, chunk) in skus.chunks(100).enumerate() {
                    if cancel.is_cancelled() {
                        return Err(CoreError::Cancelled);
                    }
                    if i > 0 {
                        tokio::time::sleep(STOCKS_BATCH_DELAY).await;
                    }
                    let mut body = build_download_body(&self.type_id, params);
                    body["skus"] = json!(chunk.to_vec());
                    // Batch-level retry: per-request retry в клиенте (3 попытки)
                    // иногда не хватает — throttle снимается дольше. Без этой
                    // страховки один 429 убивает всю выгрузку из N батчей.
                    let resp = {
                        let mut batch_attempts = 0u32;
                        loop {
                            if cancel.is_cancelled() {
                                return Err(CoreError::Cancelled);
                            }
                            match self.client.post(self.endpoint, &body, auth).await {
                                Ok(r) => break r,
                                Err(e) if e.is_rate_limited() && batch_attempts < 5 => {
                                    batch_attempts += 1;
                                    tracing::warn!(
                                        error = %e,
                                        batch = i,
                                        attempt = batch_attempts,
                                        "rate-limit на батче analytics_stocks, пауза 10с"
                                    );
                                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                                }
                                Err(e) => return Err(e),
                            }
                        }
                    };
                    if let Some(rows) = resp.get("items").and_then(|v| v.as_array()) {
                        if let Some(arr) = collected.as_array_mut() {
                            arr.extend(rows.iter().cloned());
                        }
                    }
                }
            }
            PaginationKind::ReturnsList => {
                // /v1/returns/list: filter.logistic_return_date + last_id (int64) + has_next.
                // Без filter.status и return_schema → все статусы и схемы (FBO+FBS).
                let mut last_id: i64 = 0;
                let mut iter = 0u32;
                loop {
                    if cancel.is_cancelled() {
                        return Err(CoreError::Cancelled);
                    }
                    let prev_last_id = last_id;
                    let mut body = build_download_body(&self.type_id, params);
                    body["limit"] = json!(500);
                    body["last_id"] = json!(last_id);
                    let resp = self.client.post(self.endpoint, &body, auth).await?;
                    let has_next = resp
                        .get("has_next")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let rows = resp.get("returns").and_then(|v| v.as_array());
                    let n = rows.map_or(0, Vec::len);
                    if let Some(rows) = rows {
                        if let Some(arr) = collected.as_array_mut() {
                            arr.extend(rows.iter().cloned());
                        }
                        // Курсор next last_id = id последнего возврата страницы
                        // (id — строка-int64, напр. "1000015552").
                        if let Some(id_val) = rows.last().and_then(|r| r.get("id")) {
                            if let Some(id) = id_val.as_i64() {
                                last_id = id;
                            } else if let Some(s) = id_val.as_str() {
                                if let Ok(id) = s.parse::<i64>() {
                                    last_id = id;
                                }
                            }
                        }
                    }
                    // Выход: пустая страница, has_next=false, либо лимит итераций.
                    if n == 0 || !has_next || iter >= MAX_PAGES {
                        break;
                    }
                    // Защита от зависшего курсора: если id в ответе не спарсился и
                    // last_id не изменился — следующая страница вернёт ТЕ ЖЕ данные
                    // (раньше: 200 итераций дублей). Останов с понятной ошибкой.
                    if last_id == prev_last_id {
                        return Err(CoreError::Internal(format!(
                            "Ozon {}: returns/list: курсор last_id не продвинулся \
                             (id в ответе отсутствует/непарсим) — останов во избежание дублей",
                            self.type_id
                        )));
                    }
                    iter += 1;
                }
            }
            PaginationKind::AccrualPostings => {
                // posting_numbers[] обязательны (1–200 за запрос). Если не переданы
                // (GUI / CLI без --posting-numbers) — auto-fill через /v2/posting/fbo/list
                // (номера отправлений за период). Затем батчинг ≤200.
                let (df, dt) = match (
                    params.get("date_from").map(str::to_string),
                    params.get("date_to").map(str::to_string),
                ) {
                    (Some(a), Some(b)) => (Some(a), Some(b)),
                    _ => period_to_date_range(params.period.as_deref()),
                };
                let postings: Vec<String> = match params.get("posting_numbers") {
                    Some(csv) if !csv.is_empty() => csv
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .collect(),
                    _ => {
                        // Auto-fill: сначала FBO, затем FBS (отчёт покрывает обе
                        // схемы — раньше FBS-аккаунты оставались без отправлений).
                        let fbo_nums = self
                            .client
                            .fetch_posting_numbers(auth, df.as_deref(), dt.as_deref())
                            .await;
                        let mut v = match fbo_nums {
                            Ok(v) => v,
                            Err(e) => {
                                return Err(CoreError::Internal(format!(
                                    "не удалось получить posting_numbers через /v2/posting/fbo/list: {e}"
                                )));
                            }
                        };
                        match self
                            .client
                            .fetch_fbs_posting_numbers(auth, df.as_deref(), dt.as_deref())
                            .await
                        {
                            Ok(extra) => {
                                if !extra.is_empty() {
                                    tracing::info!(
                                        type_id = %self.type_id,
                                        fbo = v.len(),
                                        fbs = extra.len(),
                                        "posting_numbers auto-filled: FBO + FBS (/v4/posting/fbs/list)"
                                    );
                                    v.extend(extra);
                                }
                            }
                            Err(e) => {
                                return Err(CoreError::Internal(format!(
                                    "не удалось получить posting_numbers через /v4/posting/fbs/list: {e}"
                                )));
                            }
                        }
                        if v.is_empty() {
                            return Err(CoreError::Internal(
                                "нет отправлений за период (FBO /v2 + FBS /v4 вернули пустые \
                                 списки) — отчёту «Начисления по отправлениям» нечего строить."
                                    .into(),
                            ));
                        }
                        v
                    }
                };
                // Батчинг ≤200 posting_numbers на запрос (лимит /v1/finance/accrual/postings).
                // Между батчами — мягкий pacing (защита от per-second rate limit).
                for (i, chunk) in postings.chunks(200).enumerate() {
                    if cancel.is_cancelled() {
                        return Err(CoreError::Cancelled);
                    }
                    if i > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    }
                    let mut body = build_download_body(&self.type_id, params);
                    body["posting_numbers"] = json!(chunk.to_vec());
                    let resp = self.client.post(self.endpoint, &body, auth).await?;
                    if let Some(rows) = resp.get("posting_accruals").and_then(|v| v.as_array()) {
                        if let Some(arr) = collected.as_array_mut() {
                            arr.extend(rows.iter().cloned());
                        }
                    }
                }
            }
        }

        let period = params.period.clone().unwrap_or_else(|| "current".into());
        // Конвертация собранных данных в Excel (.xlsx) с русскими заголовками.
        // Ключ массива зависит от type_id: xlsx-конвертер ищет его в объекте.
        let array_key = match self.type_id.as_str() {
            "ozon.accrual_postings" => "posting_accruals",
            "ozon.accrual_by_day" => "accruals",
            "ozon.returns" => "returns",
            _ => "items",
        };
        let wrapped = serde_json::json!({
            "type_id": self.type_id,
            "period": period,
            array_key: collected,
        });
        let content = crate::xlsx::workbook_bytes(&self.type_id, &wrapped)?;
        Ok(vec![DownloadedFile::with_content(
            format!("{}_{}.xlsx", self.type_id, period),
            "xlsx",
            content,
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
    fn build_body_returns_filter_iso() {
        // /v1/returns/list: filter.logistic_return_date {time_from,time_to} — ISO.
        // Без filter.status и return_schema → все статусы и схемы (FBO+FBS).
        let params = ReportParams::default()
            .with("date_from", "2026-07-01")
            .with("date_to", "2026-07-31");
        let body = build_download_body("ozon.returns", &params);
        assert_eq!(
            body["filter"]["logistic_return_date"]["time_from"],
            "2026-07-01T00:00:00.000Z"
        );
        assert_eq!(
            body["filter"]["logistic_return_date"]["time_to"],
            "2026-07-31T23:59:59.999Z"
        );
        // Намеренно нет filter.status и filter.return_schema (все возвраты).
        assert!(body["filter"].get("status").is_none());
        assert!(body["filter"].get("return_schema").is_none());
    }

    #[test]
    fn build_body_postings_filter_processed_at_iso() {
        // /v1/report/postings/create: filter{processed_at_from,to} — ISO datetime.
        let params = ReportParams::default()
            .with("date_from", "2026-07-01")
            .with("date_to", "2026-07-31");
        let body = build_download_body("ozon.postings", &params);
        assert_eq!(body["filter"]["processed_at_from"], "2026-07-01T00:00:00.000Z");
        assert_eq!(body["filter"]["processed_at_to"], "2026-07-31T23:59:59.999Z");
    }

    #[test]
    fn build_body_warehouse_stock_camelcase() {
        // /v1/report/warehouse/stock: warehouseId[] (camelCase, значения-строки).
        let params = ReportParams::default().with("warehouse_ids", "102, 103");
        let body = build_download_body("ozon.warehouse_stock", &params);
        assert_eq!(body["warehouseId"], json!(["102", "103"]));
        assert_eq!(body["language"], "DEFAULT");
    }

    #[test]
    fn build_body_placement_date_only() {
        // placement/by-products|by-supplies: date_from/to YYYY-MM-DD (без ISO).
        for tid in ["ozon.placement_by_products", "ozon.placement_by_supplies"] {
            let params = ReportParams::default()
                .with("date_from", "2026-07-01")
                .with("date_to", "2026-07-31");
            let body = build_download_body(tid, &params);
            assert_eq!(body["date_from"], "2026-07-01", "{tid}");
            assert_eq!(body["date_to"], "2026-07-31", "{tid}");
        }
    }

    #[test]
    fn build_body_marked_products_nested_date() {
        // /v1/report/marked-products-sales/create: date{from,to} — date-only YYYY-MM-DD.
        // Сервер требует ровно 10 символов; ISO datetime с T..Z (24 символа) отвергается.
        let params = ReportParams::default()
            .with("date_from", "2026-07-01")
            .with("date_to", "2026-07-31");
        let body = build_download_body("ozon.marked_products_sales", &params);
        assert_eq!(body["date"]["from"], "2026-07-01");
        assert_eq!(body["date"]["to"], "2026-07-31");
    }

    #[test]
    fn build_body_cash_flow_page_and_date() {
        // /v1/finance/cash-flow-statement/list: date{from,to} + page_size.
        let params = ReportParams::default()
            .with("date_from", "2026-07-01")
            .with("date_to", "2026-07-31");
        let body = build_download_body("ozon.cash_flow", &params);
        assert_eq!(body["date"]["from"], "2026-07-01T00:00:00.000Z");
        assert_eq!(body["page_size"], 1000);
        // page проставляется в sweep, не здесь.
        assert!(body.get("page").is_none());
    }

    #[test]
    fn build_body_analytics_stocks_skus() {
        // /v1/analytics/stocks: skus[].
        let params = ReportParams::default().with("skus", "100, 200,300");
        let body = build_download_body("ozon.analytics_stocks", &params);
        assert_eq!(body["skus"], json!(["100", "200", "300"]));
    }

    #[test]
    fn detect_format_by_magic_bytes() {
        // Настоящий xlsx — ZIP с сигнатурой PK.
        assert_eq!(detect_format(&[0x50, 0x4B, 0x03, 0x04, 0x00]), "xlsx");
        // PDF (b2b_sales): сигнатура %PDF.
        assert_eq!(detect_format(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3"), "pdf");
        // CSV (Ozon realization_posting): текст, запятая.
        assert_eq!(detect_format(b"row_number,commission_ratio, seller"), "csv");
        // CSV с BOM (Ozon products): EF BB BF + «"».
        assert_eq!(
            detect_format(&[0xEF, 0xBB, 0xBF, b'"', b'A', b'"']),
            "csv"
        );
        // JSON.
        assert_eq!(detect_format(b"{\"items\":[]}"), "json");
        assert_eq!(detect_format(b"[1,2,3]"), "json");
        // JSON с BOM.
        assert_eq!(detect_format(&[0xEF, 0xBB, 0xBF, b'{']), "json");
        // XML.
        assert_eq!(detect_format(b"<?xml version=\"1.0\"?>"), "xml");
    }

    #[test]
    fn build_body_accrual_postings_numbers() {
        // /v1/finance/accrual/postings: posting_numbers[] (обязательны).
        let params = ReportParams::default().with("posting_numbers", "234-1-1, 234-1-2");
        let body = build_download_body("ozon.accrual_postings", &params);
        assert_eq!(body["posting_numbers"], json!(["234-1-1", "234-1-2"]));
    }

    #[test]
    fn build_body_accrual_by_day_date() {
        // /v1/finance/accrual/by-day: date YYYY-MM-DD.
        let params = ReportParams { period: Some("2026-07-15".into()), ..Default::default() };
        let body = build_download_body("ozon.accrual_by_day", &params);
        assert_eq!(body["date"], "2026-07-15");
        assert!(body.get("last_id").is_none()); // last_id проставляется в sweep.
    }

    #[test]
    fn build_body_discounted_empty() {
        // /v1/report/discounted/create: пустое тело.
        let body = build_download_body("ozon.discounted", &ReportParams::default());
        assert!(body.as_object().map_or(true, serde_json::Map::is_empty));
    }

    #[test]
    fn reports_count_is_21() {
        let caps = capabilities();
        assert_eq!(caps.reports.len(), 21, "got {} reports", caps.reports.len());
        // accrual_types удалён — это служебный справочник, не выгрузка.
        assert!(!caps.reports.iter().any(|r| r.type_id == "ozon.accrual_types"));
        // b2b_sales_json удалён — дублёр b2b_sales (PDF).
        assert!(!caps.reports.iter().any(|r| r.type_id == "ozon.b2b_sales_json"));
        // act_discrepancy удалён — требует сложных схем/UI.
        assert!(!caps.reports.iter().any(|r| r.type_id == "ozon.act_discrepancy"));
        // transaction_list/totals НЕ включены (deprecated → отключены 8 сентября 2026).
        assert!(!caps.reports.iter().any(|r| r.type_id == "ozon.transaction_list"));
        assert!(!caps.reports.iter().any(|r| r.type_id == "ozon.transaction_totals"));
        // stock_on_warehouses НЕ включён (deprecated → заменён на analytics/stocks).
        assert!(!caps.reports.iter().any(|r| r.type_id == "ozon.stock_on_warehouses"));
        // realization_by_day удалён (требует подписку Premium Plus/Pro).
        assert!(!caps.reports.iter().any(|r| r.type_id == "ozon.realization_by_day"));
        // accrual_postings/by_day ВОЗВРАЩЕНЫ (реализованы по доке: posting_numbers
        // и date+last_id — через OzonPaginatedReport, не browsable).
        assert!(caps.reports.iter().any(|r| r.type_id == "ozon.accrual_postings"));
        assert!(caps.reports.iter().any(|r| r.type_id == "ozon.accrual_by_day"));
        // cash_flow включён (inline, sweep страниц).
        assert!(caps.reports.iter().any(|r| r.type_id == "ozon.cash_flow"));
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

    /// Страж дрейфа «таблиц в 4 местах»: КАЖДЫЙ дескриптор обязан диспетчеризоваться
    /// в make_report (иначе reports() молча выкинет отчёт из UI), а КАЖДЫЙ
    /// type_id — иметь дескриптор (иначе отчёт-фантом в make_report без UI).
    #[test]
    fn descriptors_and_dispatch_are_in_sync() {
        let caps = capabilities();
        for d in &caps.reports {
            assert!(
                make_report(&d.type_id, OzonHttpClient::new(None, crate::client::RetryPolicy::default()).unwrap()).is_ok(),
                "{}: есть дескриптор, но make_report не диспетчеризует (выпадет из UI)",
                d.type_id
            );
            assert!(
                !d.display_name.trim().is_empty(),
                "{}: пустой display_name",
                d.type_id
            );
        }
        // Обратное направление: фантомных arm'ов нет — их count == числу дескрипторов
        // (косвенно: все type_id make_report покрыты дескрипторами, иначе arm мёртв).
        let ids: std::collections::HashSet<&str> =
            caps.reports.iter().map(|d| d.type_id.as_str()).collect();
        for known in [
            "ozon.realization", "ozon.realization_posting", "ozon.buyout", "ozon.balance",
            "ozon.cash_flow", "ozon.accrual_by_day", "ozon.accrual_postings", "ozon.compensation",
            "ozon.decompensation", "ozon.mutual_settlement", "ozon.b2b_sales", "ozon.products",
            "ozon.returns", "ozon.postings", "ozon.discounted", "ozon.warehouse_stock",
            "ozon.placement_by_products", "ozon.placement_by_supplies", "ozon.marked_products_sales",
            "ozon.analytics_turnover", "ozon.analytics_stocks",
        ] {
            assert!(ids.contains(known), "{known}: arm без дескриптора (мёртвый код/дрейф)");
        }
        // Кап-отчёты обзавелись max_range_days.
        for capped in ["ozon.balance", "ozon.buyout", "ozon.placement_by_products", "ozon.placement_by_supplies"] {
            let d = caps.reports.iter().find(|d| d.type_id == capped).unwrap();
            assert!(d.max_range_days.is_some(), "{capped}: max_range_days не задан");
        }
    }
}
