//! # mdwf-providers-wildberries
//!
//! Реализация `MarketplaceProvider` для Wildberries OpenAPI (спец. §2.10, гл. 12).
//!
//! 24 отчёта, 5 subclients по доменам, Documents API (УПД/УКД/акты).
//! **Authorization БЕЗ префикса `Bearer`** (спец. §2.10.1).

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::map_unwrap_or)]
#![allow(clippy::unnecessary_literal_bound)]
#![allow(clippy::needless_question_mark)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::inefficient_to_string)]
#![allow(clippy::assigning_clones)]
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(clippy::unused_async)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::doc_lazy_continuation)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::unused_self)]

mod auth;
mod client;
mod date_format;
mod documents;
mod provider;
mod reports;
mod xlsx;

pub use auth::{WbAuthenticator, WbTokenType, TOKEN_TTL_DAYS};
pub use client::{RateLimiter, RetryPolicy, WbDomain, WbHttpClient};
pub use date_format::{format_date_moscow, format_moscow_rfc3339};
pub use documents::{DocumentCategory, DocumentsClient, ListDocumentsParams, WbDocument};
pub use provider::WildberriesProvider;
pub use reports::out_of_scope;
