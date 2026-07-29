//! Documents API WB (спец. §2.10.3) — список/скачивание УПД, УКД, актов.
//!
//! Трёхшаговый паттерн:
//! 1. `GET /api/v1/documents/categories` — список категорий.
//! 2. `GET /api/v1/documents/list` — список документов по категории.
//! 3. `GET /api/v1/documents/download` или `POST /api/v1/documents/download/all`
//!    — скачивание (одиночное или батчами до 50).

use base64::Engine;
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::json;
use tracing::debug;

use mdwf_core::{Authenticator, CoreError, CoreResult, DocumentEntry};

use crate::client::{WbDomain, WbHttpClient};
use crate::date_format::format_date_moscow;

/// Категория документа WB (из `/documents/categories`).
#[derive(Debug, Clone, Deserialize)]
pub struct DocumentCategory {
    pub name: String,
    #[serde(default)]
    pub parent: Option<String>,
}

/// Параметры запроса списка документов.
#[derive(Debug, Clone)]
pub struct ListDocumentsParams {
    pub category: Option<String>,
    pub date_from: Option<NaiveDate>,
    pub date_to: Option<NaiveDate>,
    pub limit: u32,
    pub offset: u32,
}

impl Default for ListDocumentsParams {
    fn default() -> Self {
        Self {
            category: None,
            date_from: None,
            date_to: None,
            limit: 1000,
            offset: 0,
        }
    }
}

/// Элемент списка документов WB.
#[derive(Debug, Clone, Deserialize)]
pub struct WbDocument {
    pub id: serde_json::Value,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub extension: Option<String>,
    #[serde(default)]
    pub size: Option<i64>,
    #[serde(default)]
    pub create_date: Option<String>,
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

    /// Шаг 1: список поддерживаемых категорий.
    pub async fn list_categories(
        &self,
        auth: &dyn Authenticator,
    ) -> CoreResult<Vec<DocumentCategory>> {
        debug!("WB documents: list categories");
        let json = self
            .http
            .get(WbDomain::Documents, "/api/v1/documents/categories", &[], auth)
            .await?;
        let cats: Vec<DocumentCategory> = json
            .get("data")
            .and_then(|d| serde_json::from_value(d.clone()).ok())
            .unwrap_or_default();
        Ok(cats)
    }

    /// Проверяет, что категория поддерживается WB (спец. §2.10.3 шаг 1).
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

    /// Шаг 2: список документов по параметрам.
    pub async fn list_documents(
        &self,
        auth: &dyn Authenticator,
        params: &ListDocumentsParams,
    ) -> CoreResult<Vec<WbDocument>> {
        debug!(category = ?params.category, "WB documents: list");
        let mut query: Vec<(&str, String)> = vec![
            ("limit", params.limit.to_string()),
            ("offset", params.offset.to_string()),
        ];
        if let Some(c) = &params.category {
            query.push(("category", c.clone()));
        }
        if let Some(d) = params.date_from {
            query.push(("begin_time", format_date_moscow(d)));
        }
        if let Some(d) = params.date_to {
            query.push(("end_time", format_date_moscow(d)));
        }
        let query_ref: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let json = self
            .http
            .get(WbDomain::Documents, "/api/v1/documents/list", &query_ref, auth)
            .await?;
        let docs: Vec<WbDocument> = json
            .get("data")
            .and_then(|d| serde_json::from_value(d.clone()).ok())
            .unwrap_or_default();
        Ok(docs)
    }

    /// Шаг 3 (одиночный): скачивание одного документа.
    /// Возвращает декодированные байты (base64 ZIP/XLSX/XML).
    pub async fn download_one(
        &self,
        auth: &dyn Authenticator,
        download_id: &str,
    ) -> CoreResult<Vec<u8>> {
        debug!(%download_id, "WB documents: download one");
        let json = self
            .http
            .get(
                WbDomain::Documents,
                "/api/v1/documents/download",
                &[("downloadId", download_id)],
                auth,
            )
            .await?;
        let b64 = json
            .get("data")
            .and_then(|d| d.get("file"))
            .and_then(|f| f.as_str())
            .ok_or_else(|| CoreError::Internal("WB download: no 'data.file' field".into()))?;
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| CoreError::Internal(format!("base64 decode: {e}")))
    }

    /// Шаг 3 (батч): батч-скачивание до 50 документов через `/download/all`.
    pub async fn download_batch(
        &self,
        auth: &dyn Authenticator,
        ids: &[(String, String)], // (service_name, extension)
    ) -> CoreResult<Vec<Vec<u8>>> {
        debug!(count = ids.len(), "WB documents: download batch");
        if ids.len() > 50 {
            return Err(CoreError::InvalidParameter(format!(
                "batch limit is 50, got {}",
                ids.len()
            )));
        }
        let body: Vec<serde_json::Value> = ids
            .iter()
            .map(|(name, ext)| json!({" serviceName": name, "extension": ext}))
            .collect();
        let json = self
            .http
            .post(WbDomain::Documents, "/api/v1/documents/download/all", &body, auth)
            .await?;
        let arr = json
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| CoreError::Internal("WB download/all: no 'data' array".into()))?;
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            let b64 = item
                .get("file")
                .and_then(|f| f.as_str())
                .ok_or_else(|| CoreError::Internal("WB download/all: missing 'file'".into()))?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| CoreError::Internal(format!("base64 decode: {e}")))?;
            out.push(bytes);
        }
        Ok(out)
    }
}

/// Преобразует `WbDocument` в `DocumentEntry` для UI.
#[must_use]
pub fn wb_document_to_entry(doc: &WbDocument, category: &str) -> DocumentEntry {
    let id_str = match &doc.id {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    };
    let display = doc
        .filename
        .clone()
        .unwrap_or_else(|| format!("Документ {id_str}"));
    let mut e = DocumentEntry::new(id_str, display);
    e.category = category.to_string();
    e.extensions = doc
        .extension
        .as_deref()
        .map(|x| vec![x.to_string()])
        .unwrap_or_default();
    e.size_hint = doc.size.map(|s| s.max(0) as u64);
    if let Some(date_str) = &doc.create_date {
        e.date = chrono::DateTime::parse_from_rfc3339(date_str)
            .ok()
            .map(|dt| dt.date_naive());
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn wb_doc_to_entry_basic() {
        let doc = WbDocument {
            id: json!(12345),
            url: None,
            filename: Some("УПД №123".into()),
            extension: Some("xml".into()),
            size: Some(2048),
            create_date: Some("2026-07-01T00:00:00+03:00".into()),
        };
        let e = wb_document_to_entry(&doc, "upd");
        assert_eq!(e.id, "12345");
        assert_eq!(e.display_name, "УПД №123");
        assert_eq!(e.extensions, vec!["xml"]);
        assert_eq!(e.size_hint, Some(2048));
        assert_eq!(e.date, Some(NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()));
    }
}
