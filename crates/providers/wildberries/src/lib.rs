//! # mdwf-providers-wildberries
//!
//! Реализация `MarketplaceProvider` для Wildberries OpenAPI (спец. §2.10, гл. 12).
//! 24 отчёта, 5 subclients по доменам, Documents API (УПД/УКД/акты).
//! Authorization БЕЗ префикса `Bearer`. Появится на ЭТАПЕ 9.

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
