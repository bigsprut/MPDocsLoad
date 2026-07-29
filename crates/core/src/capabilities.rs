//! Capabilities — самоописание провайдера (спец. §1.2, §2.3.2, §2.5.3).
//!
//! GUI, CLI и Scheduler строятся динамически из этих данных (спец. §1.3 п.2).

use serde::{Deserialize, Serialize};

use crate::params::ReportParameter;
use crate::report::{AcquisitionMode, ReportCategory};
use crate::downloader::DownloaderKind;

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
/// Полностью описывает отчёт для построения UI до инстанцирования `Report`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportDescriptor {
    pub type_id: String,
    pub display_name: String,
    pub category: ReportCategory,
    pub acquisition_mode: AcquisitionMode,
    pub downloader_kind: DownloaderKind,
    pub parameters: Vec<ReportParameter>,
}

/// Полное самоописание провайдера (спец. §2.3.2 — `Capabilities`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    pub auth_type: AuthType,
    pub auth_fields: Vec<AuthField>,
    pub reports: Vec<ReportDescriptor>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_type_serde() {
        for t in [AuthType::ApiKey, AuthType::BearerToken, AuthType::OAuth2] {
            let json = serde_json::to_string(&t).unwrap();
            let back: AuthType = serde_json::from_str(&json).unwrap();
            assert_eq!(t, back);
        }
    }

    #[test]
    fn auth_field_kind_select() {
        let kind = AuthFieldKind::Select(vec!["a".into(), "b".into()]);
        let json = serde_json::to_string(&kind).unwrap();
        let back: AuthFieldKind = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{kind:?}"), format!("{back:?}"));
    }

    #[test]
    fn capabilities_roundtrip() {
        let caps = Capabilities {
            auth_type: AuthType::ApiKey,
            auth_fields: vec![AuthField {
                id: "x".into(),
                label: "X".into(),
                kind: AuthFieldKind::Text,
                required: true,
                placeholder: None,
                help_text: None,
                secret: false,
            }],
            reports: Vec::new(),
        };
        let json = serde_json::to_string(&caps).unwrap();
        let back: Capabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(back.auth_type, AuthType::ApiKey);
        assert_eq!(back.auth_fields.len(), 1);
    }
}
