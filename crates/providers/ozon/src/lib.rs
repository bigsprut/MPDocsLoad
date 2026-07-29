//! # mdwf-providers-ozon
//!
//! Реализация `MarketplaceProvider` для Ozon Seller API (спец. §2.9, гл. 11).
//! 20 отчётов, авторизация Client-Id + Api-Key, TTL ключа 180 дней.
//! Появится на ЭТАПЕ 8.

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]
