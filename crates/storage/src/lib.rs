//! # mdwf-storage
//!
//! Файловое хранилище выгрузок + SQLite-каталог (спец. §2.4, §2.7.2, гл. 06).
//!
//! Модули (`file_store`, `naming`, `catalog`, `dedup`, миграции) появятся на ЭТАПЕ 3.

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
