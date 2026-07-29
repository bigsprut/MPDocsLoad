//! # mdwf-test-provider
//!
//! Mock-провайдер `TestProvider` (спец. гл. 17, этап 5) для разработки
//! и интерактивного тестирования GUI/CLI без реальных API маркетплейсов.
//!
//! Возвращает фейковые `DocumentEntry` и `DownloadedFile` по предсказуемой схеме.

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_errors_doc)]

mod provider;

pub use provider::{TestAuthenticator, TestProvider, TestReport};
