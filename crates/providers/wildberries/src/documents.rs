//! Documents API WB — список/скачивание УПД, УКД, актов.
//!
//! Сверено с официальной документацией dev.wildberries.ru (раздел «Документы»).
//!
//! Эндпоинты (домен documents-api.wildberries.ru):
//! - GET  /api/v1/documents/categories  -> {"data":{"categories":[...]}}
//! - GET  /api/v1/documents/list         -> {"data":{"documents":[...]}}
//!   параметры: locale, beginTime, endTime, sort, order, category, serviceName, limit(<=50), offset
//! - GET  /api/v1/documents/download     -> {"data":{"fileName","extension","document"(base64)}}
//!   параметры: serviceName, extension
//! - POST /api/v1/documents/download/all -> {"data":{"fileName","extension","document"(base64)}}
//!   тело: {"params":[{serviceName, extension}, ...]} до 50 элементов

use base64::Engine;
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::json;
use tracing::debug;

use mdwf_core::{Authenticator, CoreError, CoreResult, DocumentEntry};

use crate::client::{WbDomain, WbHttpClient};
use crate::date_format;

/// Категория документа WB (из /documents/categories).
#[derive(Debug, Clone, Deserialize)]
pub struct DocumentCategory {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
}

/// Параметры запроса списка документов.
#[derive(Debug, Clone)]
pub struct ListDocumentsParams {
    pub category: Option<String>,
    pub date_from: Option<NaiveDate>,
    pub date_to: Option<NaiveDate>,
    /// locale: "ru" | "en" | "zh" (дока: default "en").
    pub locale: String,
    /// limit: максимум 50 (дока).
    pub limit: u32,
    pub offset: u32,
}

impl Default for ListDocumentsParams {
    fn default() -> Self {
        Self {
            category: None,
            date_from: None,
            date_to: None,
            locale: "ru".into(),
            limit: 50,
            offset: 0,
        }
    }
}

/// Элемент списка документов WB (поле documents[]. из ответа).
#[derive(Debug, Clone, Deserialize)]
pub struct WbDocument {
    /// Уникальный ID документа (дока: передаётся как serviceName в /download).
    #[serde(rename = "serviceName")]
    pub service_name: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub amount: Option<f64>,
    #[serde(default)]
    pub date: Option<String>,
}

/// Subclient для Documents API.
pub struct DocumentsClient<'a> {
    http: &'a WbHttpClient,
}

impl<'a> DocumentsClient<'a> {
    #[must_use]
    pub fn new(http: &'a WbHttpClient) -> Self {
        Self { http }
    }

    /// Список поддерживаемых категорий. Ответ: {"data":{"categories":[...]}}.
    pub async fn list_categories(
        &self,
        auth: &dyn Authenticator,
    ) -> CoreResult<Vec<DocumentCategory>> {
        debug!("WB documents: list categories");
        let json = self
            .http
            .get(
                WbDomain::Documents,
                "/api/v1/documents/categories",
                &[("locale", "ru")],
                auth,
            )
            .await?;
        // Формат: {"data": {"categories": [...]}}.
        let cats: Vec<DocumentCategory> = json
            .get("data")
            .and_then(|d| d.get("categories"))
            .and_then(|c| serde_json::from_value(c.clone()).ok())
            .unwrap_or_default();
        Ok(cats)
    }

    /// Проверяет, что категория поддерживается WB.
    pub async fn ensure_category(
        &self,
        auth: &dyn Authenticator,
        category: &str,
    ) -> CoreResult<()> {
        let cats = self.list_categories(auth).await?;
        if !cats.iter().any(|c| c.name == category) {
            return Err(CoreError::InvalidParameter(format!(
                "WB documents API не возвращает категорию '{category}'"
            )));
        }
        Ok(())
    }

    /// Список документов по параметрам. Ответ: {"data":{"documents":[...]}}.
    pub async fn list_documents(
        &self,
        auth: &dyn Authenticator,
        params: &ListDocumentsParams,
    ) -> CoreResult<Vec<WbDocument>> {
        debug!(category = ?params.category, "WB documents: list");
        let mut query: Vec<(&str, String)> = vec![
            ("locale", params.locale.clone()),
            ("limit", params.limit.min(50).to_string()),
            ("offset", params.offset.to_string()),
            ("sort", "date".into()),
            ("order", "desc".into()),
        ];
        if let Some(c) = &params.category {
            query.push(("category", c.clone()));
        }
        // Дока: beginTime/endTime (CamelCase), дата YYYY-MM-DD.
        if let Some(d) = params.date_from {
            query.push(("beginTime", format_date(d)));
        }
        if let Some(d) = params.date_to {
            query.push(("endTime", format_date(d)));
        }
        let query_ref: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let json = self
            .http
            .get(WbDomain::Documents, "/api/v1/documents/list", &query_ref, auth)
            .await?;
        // Формат: {"data": {"documents": [...]}}.
        let docs: Vec<WbDocument> = json
            .get("data")
            .and_then(|d| d.get("documents"))
            .and_then(|docs| serde_json::from_value(docs.clone()).ok())
            .unwrap_or_default();
        Ok(docs)
    }

    /// Скачивание одного документа. Ответ: {"data":{"fileName","extension","document"(base64)}}.
    /// Параметры (дока): serviceName (required) + extension (required).
    pub async fn download_one(
        &self,
        auth: &dyn Authenticator,
        service_name: &str,
        extension: &str,
    ) -> CoreResult<Vec<u8>> {
        debug!(service_name, extension, "WB documents: download one");
        let json = self
            .http
            .get(
                WbDomain::Documents,
                "/api/v1/documents/download",
                &[("serviceName", service_name), ("extension", extension)],
                auth,
            )
            .await?;
        // Формат: {"data": {"document": "<base64>"}}.
        let b64 = json
            .get("data")
            .and_then(|d| d.get("document"))
            .and_then(|f| f.as_str())
            .ok_or_else(|| CoreError::Internal("WB download: нет поля data.document".into()))?;
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| CoreError::Internal(format!("base64 decode: {e}")))
    }

    /// Батч-скачивание до 50 документов через /download/all.
    /// Тело: {"params":[{"serviceName","extension"}, ...]}. Ответ как у /download.
    pub async fn download_batch(
        &self,
        auth: &dyn Authenticator,
        items: &[(String, String)], // (serviceName, extension)
    ) -> CoreResult<Vec<u8>> {
        debug!(count = items.len(), "WB documents: download batch");
        if items.is_empty() {
            return Err(CoreError::InvalidParameter("пустой батч".into()));
        }
        if items.len() > 50 {
            return Err(CoreError::InvalidParameter(format!(
                "лимит батча 50, получено {}",
                items.len()
            )));
        }
        let params: Vec<serde_json::Value> = items
            .iter()
            .map(|(name, ext)| json!({"serviceName": name, "extension": ext}))
            .collect();
        let body = json!({"params": params});
        let json = self
            .http
            .post(WbDomain::Documents, "/api/v1/documents/download/all", &body, auth)
            .await?;
        let b64 = json
            .get("data")
            .and_then(|d| d.get("document"))
            .and_then(|f| f.as_str())
            .ok_or_else(|| CoreError::Internal("WB download/all: нет data.document".into()))?;
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| CoreError::Internal(format!("base64 decode: {e}")))
    }
}

/// Преобразует WbDocument в DocumentEntry для UI.
#[must_use]
pub fn wb_document_to_entry(doc: &WbDocument, category: &str) -> DocumentEntry {
    let mut e = DocumentEntry::new(doc.service_name.clone(), doc.service_name.clone());
    e.category = category.to_string();
    e.extensions = vec!["zip".into(), "xml".into()];
    e.size_hint = doc.amount.map(|a| a as u64);
    if let Some(date_str) = &doc.date {
        e.date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok();
    }
    e
}

/// Форматирует дату как YYYY-MM-DD (дока: beginTime/endTime в формате даты).
fn format_date(d: NaiveDate) -> String {
    date_format::format_date_moscow(d)
        .split('T')
        .next()
        .unwrap_or("2026-01-01")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wb_doc_to_entry_basic() {
        let doc = WbDocument {
            service_name: "redeem-notification-44841941".into(),
            category: Some("redeem-notification".into()),
            amount: Some(100.0),
            date: Some("2026-07-01".into()),
        };
        let e = wb_document_to_entry(&doc, "redeem-notification");
        assert_eq!(e.id, "redeem-notification-44841941");
        assert_eq!(e.category, "redeem-notification");
        assert_eq!(e.date, Some(NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()));
    }

    #[test]
    fn default_params_limit_50() {
        let p = ListDocumentsParams::default();
        assert_eq!(p.limit, 50);
        assert_eq!(p.locale, "ru");
    }
}
