//! Отчёт — центральная абстракция выгрузки (спец. §2.3.2, гл. 13).
//!
//! Ключевое расширение против спеки: `AcquisitionMode` (Period/Browsable).
//! См. обоснование в плане: Ozon и WB имеют оба режима — list-based и period-based.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::auth::Authenticator;
use crate::downloader::{DownloadedFile, DownloaderKind};
use crate::error::CoreResult;
use crate::params::{ReportParams, ReportParameter};
use crate::progress::ProgressCallbackRef;

/// Категория отчёта (для группировки в UI).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReportCategory {
    /// Финансовые отчёты (реализация, взаиморасчёты, баланс, ДДС).
    Finance,
    /// Документы строгой отчётности (УПД, УКД, акты).
    Documents,
    /// Операционные (заказы, продажи).
    Operational,
    /// Штрафы, антифрод, декомпенсации.
    Penalties,
    /// Возвраты.
    Returns,
    /// Аналитика.
    Analytics,
}

/// Режим получения данных — ключевое расширение против спеки.
///
/// * `Period` — отчёт генерируется по периоду (тип + период → скачать).
///   Примеры: Ozon realization, WB sales-reports/detailed.
/// * `Browsable` — у маркетплейса есть список документов с фильтром, из которого
///   пользователь выбирает конкретные и скачивает их.
///   Примеры: WB Documents API (УПД/УКД/акты по категории и дате),
///   Ozon transaction-list / accrual-postings / b2b-sales / mutual-settlement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AcquisitionMode {
    /// Тип + период → генерация → скачивание.
    Period,
    /// Список → фильтр → выбор → скачивание.
    Browsable,
}

impl AcquisitionMode {
    #[must_use]
    pub fn is_browsable(self) -> bool {
        matches!(self, Self::Browsable)
    }
}

/// Элемент списка документов для интерактивного выбора (Browsable-режим).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentEntry {
    /// Провайдер-нативный идентификатор (для download).
    pub id: String,
    /// Отображаемое имя (для списка в UI).
    pub display_name: String,
    /// Категория документа (провайдер-нативная, например "upd").
    pub category: String,
    /// Дата документа (если применимо).
    pub date: Option<NaiveDate>,
    /// Доступные расширения ("xml", "pdf", "xlsx", "zip").
    pub extensions: Vec<String>,
    /// Приблизительный размер (если известен).
    pub size_hint: Option<u64>,
    /// Произвольные провайдер-нативные метаданные.
    pub metadata: serde_json::Value,
}

impl DocumentEntry {
    #[must_use]
    pub fn new(id: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            display_name: display_name.into(),
            category: String::new(),
            date: None,
            extensions: Vec::new(),
            size_hint: None,
            metadata: serde_json::Value::Null,
        }
    }
}

/// Фильтр для Browsable-списка (формируется UI из параметров).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DocumentFilter {
    /// Категория документа (например, "upd").
    pub category: Option<String>,
    /// Начало периода.
    pub date_from: Option<NaiveDate>,
    /// Конец периода.
    pub date_to: Option<NaiveDate>,
    /// Только эти расширения (None = все).
    pub extensions: Vec<String>,
    /// Лимит числа документов (None = по умолчанию провайдера).
    pub limit: Option<u32>,
    /// Произвольные дополнительные фильтры.
    pub extra: std::collections::BTreeMap<String, String>,
}

/// Токен отмены выгрузки.
pub type CancelToken = tokio_util::sync::CancellationToken;

/// Трейт отчёта (спец. §2.3.2 + расширение AcquisitionMode).
#[async_trait]
pub trait Report: Send + Sync {
    /// Идентификатор типа (например, "ozon.realization", "wb.documents").
    fn type_id(&self) -> &str;

    /// Человекочитаемое имя.
    fn display_name(&self) -> &str;

    /// Категория для группировки в UI.
    fn category(&self) -> ReportCategory;

    /// Режим получения: Period или Browsable.
    fn acquisition_mode(&self) -> AcquisitionMode;

    /// Тип выгрузщика (Api или ApiAsyncPoll).
    fn downloader_kind(&self) -> DownloaderKind;

    /// Декларация параметров для динамической формы.
    fn parameters(&self) -> &[ReportParameter];

    // --- Browsable-режим ---

    /// Возвращает список документов по фильтру. Только для `Browsable`.
    ///
    /// Реализация по умолчанию возвращает ошибку — отчёты `Period`
    /// этот метод не поддерживают.
    async fn list(
        &self,
        _auth: &dyn Authenticator,
        _filter: &DocumentFilter,
        _cancel: CancelToken,
    ) -> CoreResult<Vec<DocumentEntry>> {
        Err(crate::error::CoreError::InvalidParameter(format!(
            "report '{}' does not support Browsable list",
            self.type_id()
        )))
    }

    // --- Общий метод скачивания ---

    /// Скачивает отчёт.
    ///
    /// * Для `Period`: `params.values` содержит период и параметры генерации.
    /// * Для `Browsable`: `params.values` содержит ключ `"ids"` (разделённые
    ///   запятой идентификаторы выбранных `DocumentEntry`).
    async fn download(
        &self,
        auth: &dyn Authenticator,
        params: &ReportParams,
        progress: ProgressCallbackRef,
        cancel: CancelToken,
    ) -> CoreResult<Vec<DownloadedFile>>;
}

/// Тип-псевдоним для arc-ссылки на отчёт.
pub type ReportRef = Arc<dyn Report>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquisition_mode_helpers() {
        assert!(AcquisitionMode::Browsable.is_browsable());
        assert!(!AcquisitionMode::Period.is_browsable());
    }

    #[test]
    fn document_entry_new() {
        let e = DocumentEntry::new("123", "УПД №123");
        assert_eq!(e.id, "123");
        assert_eq!(e.display_name, "УПД №123");
    }
}
