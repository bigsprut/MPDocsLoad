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
///
/// Поля сверены с официальной OpenAPI-спецификацией (GetListDataDocumentsInner):
/// serviceName, name, category, extensions, creationTime, viewed.
#[derive(Debug, Clone, Deserialize)]
pub struct WbDocument {
    /// Уникальный ID документа (дока: передаётся как serviceName в /download).
    #[serde(rename = "serviceName", default)]
    pub service_name: Option<String>,
    /// Человекочитаемое название документа — показываем в UI.
    #[serde(default)]
    pub name: Option<String>,
    /// Название категории документа (значение поля `title` из /categories).
    #[serde(default)]
    pub category: Option<String>,
    /// Доступные форматы файла (напр. `zip`, `xml`).
    #[serde(default)]
    pub extensions: Option<Vec<String>>,
    /// Дата и время создания документа (ISO 8601, напр. "2026-07-01T10:00:00Z").
    #[serde(rename = "creationTime", default)]
    pub creation_time: Option<String>,
    /// Выгружен ли документ в личном кабинете.
    #[serde(default)]
    pub viewed: Option<bool>,
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
    ///
    /// Возвращает байты документа И метаданные (реальный формат, имя файла),
    /// которые WB сообщает в ответе — раньше они выбрасывались.
    pub async fn download_one(
        &self,
        auth: &dyn Authenticator,
        service_name: &str,
        extension: &str,
    ) -> CoreResult<WbDownloadedDoc> {
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
        let data = json.get("data").ok_or_else(|| {
            CoreError::Internal("WB download: нет поля data".into())
        })?;
        // base64-контент документа (обязательное поле).
        let b64 = data
            .get("document")
            .and_then(|f| f.as_str())
            .ok_or_else(|| CoreError::Internal("WB download: нет поля data.document".into()))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| CoreError::Internal(format!("base64 decode: {e}")))?;
        // Реальный формат из ответа; если WB не вернул — оставляем запрошенный.
        let ext = data
            .get("extension")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| extension.to_string());
        let file_name = data
            .get("fileName")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .filter(|s| !s.is_empty());
        Ok(WbDownloadedDoc {
            bytes,
            extension: ext,
            file_name,
        })
    }
}

/// Результат скачивания документа: байты + метаданные из ответа WB.
/// Поля `extension` и `file_name` берутся напрямую из ответа
/// `/documents/download` (поля `extension`, `fileName`).
#[derive(Debug, Clone)]
pub struct WbDownloadedDoc {
    pub bytes: Vec<u8>,
    /// Реальный формат из ответа WB; fallback на запрошенный.
    pub extension: String,
    /// Человекочитаемое имя файла из ответа WB (если есть).
    pub file_name: Option<String>,
}

/// Преобразует WbDocument в DocumentEntry для UI.
///
/// `category` — запасное значение категории (передаётся вызовом из
/// `WbDocumentsReport::list`), используется если в самом документе её нет.
#[must_use]
pub fn wb_document_to_entry(doc: &WbDocument, category: &str) -> DocumentEntry {
    // serviceName — обязательный технический идентификатор для /download.
    // На случай если WB вернёт документ без него, формируем запасной id.
    let id = doc.service_name.clone().unwrap_or_else(|| {
        doc.name
            .clone()
            .unwrap_or_else(|| "wb-document".to_string())
    });
    // Отображаемое имя: осмысленное `name`, иначе категория, иначе id.
    let display = doc
        .name
        .clone()
        .filter(|n| !n.is_empty())
        .or_else(|| doc.category.clone())
        .unwrap_or_else(|| id.clone());
    let mut e = DocumentEntry::new(id, display);
    e.category = doc
        .category
        .clone()
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| category.to_string());
    // Форматы берём из ответа; если WB их не вернул — оставляем пусто,
    // а не хардкод (раньше тут было зашито ["zip","xml"]).
    e.extensions = doc.extensions.clone().unwrap_or_default();
    // Дата создания документа (поле creationTime, ISO 8601).
    if let Some(date_str) = &doc.creation_time {
        e.date = parse_creation_time(date_str);
    }
    e
}

/// Парсит creationTime (ISO 8601 с временем или только дата) в NaiveDate.
fn parse_creation_time(s: &str) -> Option<NaiveDate> {
    // Полный datetime: 2026-07-01T10:00:00Z или 2026-07-01T10:00:00+03:00.
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.date_naive());
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(dt.date());
    }
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
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
            service_name: Some("redeem-notification-44841941".into()),
            name: Some("Уведомление о выкупе №44841941".into()),
            category: Some("redeem-notification".into()),
            extensions: Some(vec!["zip".into(), "xml".into()]),
            creation_time: Some("2026-07-01T10:00:00Z".into()),
            viewed: Some(true),
        };
        let e = wb_document_to_entry(&doc, "redeem-notification");
        assert_eq!(e.id, "redeem-notification-44841941");
        // Показываем человекочитаемое name, а не serviceName.
        assert_eq!(e.display_name, "Уведомление о выкупе №44841941");
        assert_eq!(e.category, "redeem-notification");
        assert_eq!(e.extensions, vec!["zip", "xml"]);
        assert_eq!(e.date, Some(NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()));
    }

    #[test]
    fn wb_doc_to_entry_fallback_to_category_then_id() {
        // Нет name → берём category; нет serviceName → запасной id из name.
        let doc = WbDocument {
            service_name: None,
            name: None,
            category: Some("upd".into()),
            extensions: None,
            creation_time: None,
            viewed: None,
        };
        let e = wb_document_to_entry(&doc, "");
        // id отсутствует в serviceName и name → запасной.
        assert_eq!(e.id, "wb-document");
        assert_eq!(e.display_name, "upd"); // category как отображаемое имя
        assert!(e.extensions.is_empty());
    }

    #[test]
    fn wb_doc_creation_time_date_only() {
        let doc = WbDocument {
            service_name: Some("s1".into()),
            name: None,
            category: None,
            extensions: None,
            creation_time: Some("2026-06-15".into()),
            viewed: None,
        };
        let e = wb_document_to_entry(&doc, "");
        assert_eq!(e.date, Some(NaiveDate::from_ymd_opt(2026, 6, 15).unwrap()));
    }

    #[test]
    fn default_params_limit_50() {
        let p = ListDocumentsParams::default();
        assert_eq!(p.limit, 50);
        assert_eq!(p.locale, "ru");
    }
}
