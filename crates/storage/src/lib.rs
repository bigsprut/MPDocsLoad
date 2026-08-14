//! # mdwf-storage
//!
//! Файловое хранилище выгрузок + SQLite-каталог (спец. §2.4, §2.7.2, гл. 06).

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::needless_pass_by_value)]

pub mod catalog;
pub mod dedup;
pub mod error;
pub mod file_store;
pub mod naming;

pub use catalog::{
    ArchiveEntry, Catalog, DownloadRecord, DownloadedDocInfo, JournalRow, NewDownload, NewSchedule,
    SavedFilter, ScheduleRecord, JOURNAL_KEEP, SCHEMA_VERSION,
};
pub use dedup::sha256_hex;
pub use error::{StorageError, StorageResult};
pub use file_store::{FileStore, FileStoreConfig, FolderStructure};
pub use naming::FileNameContext;
