//! # mdwf-secrets
//!
//! Обёртка над OS keychain (Windows Credential Manager / macOS Keychain /
//! Linux Secret Service) через крейт `keyring` + in-memory mock для тестов
//! (спец. §2.4, §1.3 п.6, ADR-009).

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_errors_doc)]

pub mod keychain;
pub mod memory;
pub mod os_keychain;

pub use keychain::{KEYCHAIN_SERVICE, SecretStore};
pub use memory::InMemorySecretStore;
pub use os_keychain::OsKeychain;
