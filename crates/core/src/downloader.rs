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

/// Трейт выгрузчика — выполняет фактическое скачивание байтов.
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
