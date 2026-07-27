//! # mdwf-secrets
//!
//! Обёртка над OS keychain (Windows Credential Manager) + in-memory mock для тестов
//! (спец. §2.4, §1.3 п.6, ADR-009).
//!
//! Трейт `SecretStore` и реализации появятся на ЭТАПЕ 3.

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
