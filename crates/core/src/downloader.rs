//! Downloader — абстракция выгрузки (спец. §2.4: `downloader.rs`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::progress::ProgressCallbackRef;

/// Тип выгрузчика (спец. §2.3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloaderKind {
    /// Прямой HTTP-запрос → результат сразу.
    Api,
    /// Асинхронный: create → poll task → download.
    ApiAsyncPoll,
}

/// Скачанный файл (результат выгрузки).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadedFile {
    /// Имя файла на диске (без пути).
    pub file_name: String,
    /// Расширение ("xml", "pdf", "xlsx", "zip").
    pub extension: String,
    /// MIME-тип, если известен.
    pub mime_type: Option<String>,
    /// Размер в байтах.
    pub size: u64,
    /// SHA-256 хэш (для дедупликации, спец. §1.3 п.7).
    pub sha256: Option<String>,
    /// Время скачивания (UTC).
    pub downloaded_at: DateTime<Utc>,
    /// Провайдер-нативный идентификатор документа (если применимо).
    pub source_id: Option<String>,
    /// Исходный URL (если применимо).
    pub source_url: Option<String>,
    /// Дата документа (для Browsable-режима WB: creationTime → YYYY-MM-DD).
    /// Используется для записи в каталог (document_date) и плейсхолдера {doc_date}
    /// в имени файла. None для Period-отчётов.
    pub document_date: Option<String>,
    /// In-memory содержимое файла (провайдер заполняет; app-слой пишет на диск).
    /// `None` если файл уже записан провайдером или контент не возвращается.
    #[serde(skip)]
    pub content: Option<Vec<u8>>,
}

impl DownloadedFile {
    #[must_use]
    pub fn new(file_name: impl Into<String>, extension: impl Into<String>, size: u64) -> Self {
        Self {
            file_name: file_name.into(),
            extension: extension.into(),
            mime_type: None,
            size,
            sha256: None,
            downloaded_at: Utc::now(),
            source_id: None,
            source_url: None,
            document_date: None,
            content: None,
        }
    }

    /// Создаёт файл с in-memory контентом и автоматически вычисляет размер.
    #[must_use]
    pub fn with_content(
        file_name: impl Into<String>,
        extension: impl Into<String>,
        content: Vec<u8>,
    ) -> Self {
        let size = content.len() as u64;
        let mut f = Self::new(file_name, extension, size);
        f.content = Some(content);
        f
    }
}

/// Трейт выгрузщика — выполняет фактическое скачивание байтов.
///
/// `Report::download` делегирует сюда работу конкретного под-клиента.
pub trait Downloader: Send + Sync {
    /// Скачивает один документ по идентификатору (Browsable) или генерирует
    /// отчёт по параметрам (Period). Реализуется провайдером.
    fn fetch_one(
        &self,
        id_or_params: &str,
        progress: ProgressCallbackRef,
    ) -> CoreResult<DownloadedFile>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_file_defaults() {
        let f = DownloadedFile::new("a.csv", "csv", 100);
        assert_eq!(f.file_name, "a.csv");
        assert_eq!(f.extension, "csv");
        assert_eq!(f.size, 100);
        assert!(f.mime_type.is_none());
        assert!(f.sha256.is_none());
        assert!(f.source_id.is_none());
        assert!(f.source_url.is_none());
        assert!(f.content.is_none());
    }

    #[test]
    fn with_content_sets_size_and_content() {
        let f = DownloadedFile::with_content("b.xml", "xml", b"<x/>".to_vec());
        assert_eq!(f.size, 4);
        assert_eq!(f.content.as_deref(), Some(&b"<x/>"[..]));
    }

    #[test]
    fn downloader_kind_serde() {
        for k in [DownloaderKind::Api, DownloaderKind::ApiAsyncPoll] {
            let json = serde_json::to_string(&k).expect("serialize");
            let back: DownloaderKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(k, back);
        }
    }
}
