//! Capabilities — самоописание провайдера (спец. §1.2, §2.3.2, §2.5.3).
//!
//! GUI, CLI и Scheduler строятся динамически из этих данных (спец. §1.3 п.2).

use serde::{Deserialize, Serialize};

/// Типы авторизации, поддерживаемые провайдерами (спец. §2.3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthType {
    /// Ozon: заголовки `Client-Id` + `Api-Key`.
    ApiKey,
    /// Wildberries: заголовок `Authorization` (БЕЗ префикса `Bearer`).
    BearerToken,
    /// `OAuth2` для будущих маркетплейсов.
    OAuth2,
}

/// Тип поля формы авторизации (для динамической GUI-формы, спец. §2.5.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthFieldKind {
    Text,
    Password,
    Number,
    Select(Vec<String>),
}

/// Описание одного поля формы авторизации.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthField {
    pub id: String,
    pub label: String,
    pub kind: AuthFieldKind,
    pub required: bool,
    pub placeholder: Option<String>,
    pub help_text: Option<String>,
    pub secret: bool,
}

/// Описание отчёта, который провайдер умеет выгружать.
///
/// Полное наполнение (`AcquisitionMode`, `downloader_kind`, parameters) будет
/// добавлено на ЭТАПЕ 2 вместе с трейтом `Report`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportDescriptor {
    pub type_id: String,
    pub display_name: String,
    pub category: String,
}

/// Полное самоописание провайдера (спец. §2.3.2 — `Capabilities`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    pub auth_type: AuthType,
    pub auth_fields: Vec<AuthField>,
    pub reports: Vec<ReportDescriptor>,
}
