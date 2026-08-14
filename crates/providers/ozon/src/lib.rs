//! # mdwf-providers-ozon
//!
//! Реализация `MarketplaceProvider` для Ozon Seller API (спец. §2.9, гл. 11).
//!
//! 20 отчётов через официальное API, авторизация `Client-Id` + `Api-Key`,
//! TTL ключа 180 дней (news/649). Соблюдение rate limits (50 RPS, news/584).

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::map_unwrap_or)]
#![allow(clippy::unused_async)]

mod auth;
mod client;
mod date_format;
mod provider;
mod reports;
mod xlsx;

pub use auth::{OzonAuthenticator, API_KEY_TTL_DAYS, DEFAULT_BASE_URL};
pub use client::{
    CircuitBreaker, OzonHttpClient, RateLimiter, RetryPolicy, MIN_REQUEST_INTERVAL, RATE_LIMIT_RPS,
};
pub use date_format::{
    format_date_only, format_iso8601_ms_z, format_year_month, parse_date_only, parse_year_month,
};
pub use provider::OzonProvider;
pub use reports::out_of_scope;
