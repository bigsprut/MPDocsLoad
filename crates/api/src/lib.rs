//! # mdwf-api
//!
//! Опциональный REST API (спец. §2.3.1, future) за feature-флагом `server`.
//!
//! Предоставляет HTTP-endpoints для удалённого управления MDWF:
//! - `GET /api/v1/providers` — список провайдеров.
//! - `GET /api/v1/reports/:provider_id` — отчёты провайдера.
//! - `GET /api/v1/profiles` — список профилей.
//! - `POST /api/v1/download` — запустить выгрузку (синхронно).
//!
//! Сервер mode (v2.0 по спец.); в v1.4 — базовый набор для интеграций.

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]

#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "server")]
pub use server::{AppState, serve};
