//! Файловое хранилище (спец. §2.4: `file_store.rs`, §2.7.1: конфигурация).

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;

use mdwf_core::DownloadedFile;

use crate::dedup::sha256_hex;
use crate::error::{StorageError, StorageResult};
use crate::naming::FileNameContext;

/// Структура каталогов (спец. §2.7.1: `folder_structure`).
#[derive(Debug, Clone)]
pub enum FolderStructure {
    /// Все файлы в одной папке.
    Flat,
    /// `{output_dir}/{provider}/{period}/{file}`.
    ByProviderPeriod,
    /// `{output_dir}/{provider}/{profile}/{period}/{file}`.
    ByProviderProfilePeriod,
}

impl Default for FolderStructure {
    fn default() -> Self {
        Self::ByProviderPeriod
    }
}

/// Параметры сохранения файлов.
#[derive(Debug, Clone)]
pub struct FileStoreConfig {
    pub output_dir: PathBuf,
    pub file_name_template: String,
    pub folder_structure: FolderStructure,
    /// Вычислять SHA-256 при сохранении (для дедупликации).
    pub compute_hash: bool,
}

impl Default for FileStoreConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("downloads"),
            file_name_template: "{provider}_{profile}_{report}_{doc_id}_{period}.{ext}".to_string(),
            folder_structure: FolderStructure::default(),
            compute_hash: true,
        }
    }
}

/// Файловое хранилище. Сохраняет байты на диск и возвращает `DownloadedFile`
/// с вычисленным хэшем (если включено).
#[derive(Clone)]
pub struct FileStore {
    config: FileStoreConfig,
}

impl FileStore {
    #[must_use]
    pub fn new(config: FileStoreConfig) -> Self {
        Self { config }
    }

    /// Конфигурация (read-only).
    #[must_use]
    pub fn config(&self) -> &FileStoreConfig {
        &self.config
    }

    /// Сохраняет байты на диск.
    pub fn save(
        &self,
        data: &[u8],
        ctx: &FileNameContext<'_>,
    ) -> StorageResult<DownloadedFile> {
        let (file, _dir) = self.save_with_dir(data, ctx)?;
        Ok(file)
    }

    /// Сохраняет байты на диск и возвращает (DownloadedFile, директория).
    pub fn save_with_dir(
        &self,
        data: &[u8],
        ctx: &FileNameContext<'_>,
    ) -> StorageResult<(DownloadedFile, PathBuf)> {
        let dir = self.target_dir(ctx);
        fs::create_dir_all(&dir).map_err(StorageError::Io)?;

        let file_name = ctx.render(&self.config.file_name_template);
        let path = dir.join(&file_name);

        // Атомарная запись: tmp + rename — крах посреди записи не оставит
        // обрезанный файл (который уже мог быть записан в каталог как «скачан»).
        let tmp = path.with_extension("part");
        fs::write(&tmp, data).map_err(StorageError::Io)?;
        fs::rename(&tmp, &path).map_err(StorageError::Io)?;

        let hash = if self.config.compute_hash {
            Some(sha256_hex(data))
        } else {
            None
        };

        Ok((
            DownloadedFile {
                file_name,
                extension: ctx.extension.to_string(),
                mime_type: guess_mime(ctx.extension),
                size: data.len() as u64,
                sha256: hash,
                downloaded_at: Utc::now(),
                source_id: ctx.document_id.map(str::to_string),
                source_url: None,
                document_date: ctx.document_date.map(str::to_string),
                content: None,
            },
            dir,
        ))
    }

    fn target_dir(&self, ctx: &FileNameContext<'_>) -> PathBuf {
        let base = self.config.output_dir.clone();
        match self.config.folder_structure {
            FolderStructure::Flat => base,
            FolderStructure::ByProviderPeriod => base
                .join(ctx.provider_id)
                .join(year_folder(ctx.period)),
            FolderStructure::ByProviderProfilePeriod => base
                .join(ctx.provider_id)
                .join(ctx.profile_name)
                .join(year_folder(ctx.period)),
        }
    }

    /// Гарантирует существование базовой папки.
    pub fn ensure_output_dir(&self) -> StorageResult<()> {
        fs::create_dir_all(&self.config.output_dir).map_err(StorageError::Io)
    }

    /// Путь по умолчанию для каталога SQLite в директории данных MDWF.
    #[must_use]
    pub fn default_db_path(data_dir: &Path) -> PathBuf {
        data_dir.join("mdwf.db")
    }
}

/// Подпапка-период для структуры каталогов: ГОД («2026»), а не месяц.
/// Извлекается из периода/даты («2026-07» / «2026-07-15» → «2026»);
/// не-датная строка используется как есть; отсутствие периода → «nop».
fn year_folder(period: Option<&str>) -> &str {
    match period {
        Some(p) if p.len() >= 4 && p.as_bytes()[..4].iter().all(u8::is_ascii_digit) => &p[..4],
        Some(p) => p,
        None => "nop",
    }
}

fn guess_mime(ext: &str) -> Option<String> {
    match ext.to_ascii_lowercase().as_str() {
        "csv" => Some("text/csv".into()),
        "json" => Some("application/json".into()),
        "xml" => Some("application/xml".into()),
        "pdf" => Some("application/pdf".into()),
        "xlsx" => Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into()),
        "xls" => Some("application/vnd.ms-excel".into()),
        "zip" => Some("application/zip".into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn tmp_dir() -> PathBuf {
        let d = env::temp_dir().join(format!("mdwf-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn save_writes_file_and_hash() {
        let dir = tmp_dir();
        let cfg = FileStoreConfig {
            output_dir: dir.clone(),
            ..FileStoreConfig::default()
        };
        let store = FileStore::new(cfg);
        let ctx = FileNameContext {
            provider_id: "ozon",
            profile_name: "Ozon-1",
            report_type: "realization",
            period: Some("2026-06"),
            extension: "csv",
            document_id: None,
            document_date: None,
        };
        let data = b"col1,col2\n1,2\n";
        let file = store.save(data, &ctx).unwrap();
        assert_eq!(file.extension, "csv");
        assert!(file.sha256.is_some());
        // Структура ByProviderPeriod по умолчанию; подпапка — ГОД периода
        // («2026-06» → «2026»), файл содержит полный период в имени.
        let expected_path = dir.join("ozon").join("2026").join("ozon_Ozon-1_realization_2026-06.csv");
        assert!(expected_path.exists(), "expected file at {expected_path:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn year_folder_extracts_year() {
        assert_eq!(year_folder(Some("2026-06")), "2026");
        assert_eq!(year_folder(Some("2026-07-15")), "2026");
        assert_eq!(year_folder(Some("2026")), "2026");
        // Не-датная строка — как есть (явный подпаталог из конфига).
        assert_eq!(year_folder(Some("custom")), "custom");
        assert_eq!(year_folder(None), "nop");
    }
}
