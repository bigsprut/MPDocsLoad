//! Параметры отчётов (спец. §2.4 структура: `params.rs`).
//!
//! `ReportParams` — runtime-значения параметров выгрузки,
//! `ReportParameter` — декларация параметра для построения динамической формы.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Тип параметра отчёта для динамической формы.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReportParameterKind {
    /// Дата (например, `2026-07-03`).
    Date,
    /// Месяц (например, `2026-06`).
    YearMonth,
    /// Диапазон дат: `date_from` + `date_to`.
    DateRange,
    /// Выбор из списка (например, категория документа WB).
    Select(Vec<String>),
    /// Число (например, `client_id`).
    Number,
    /// Произвольная строка.
    Text,
    /// Флаг (например, использование устаревшего метода).
    Bool,
}

/// Декларация одного параметра отчёта.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportParameter {
    pub id: String,
    pub label: String,
    pub kind: ReportParameterKind,
    pub required: bool,
    pub default: Option<String>,
}

/// Runtime-значения параметров выгрузки (ключ → значение в виде строки).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReportParams {
    /// Имя профиля, для которого идёт выгрузка (заполняется слоем приложения).
    pub profile_name: Option<String>,
    /// Идентификатор провайдера (для нейминга/каталога).
    pub provider_id: Option<String>,
    /// Тип отчёта.
    pub report_type: Option<String>,
    /// Период в канонической форме (`YYYY-MM` для месячных, `YYYY-MM-DD` для дневных).
    pub period: Option<String>,
    /// Произвольные параметры (`category`, `date_from`, `date_to`, extension, ...).
    pub values: BTreeMap<String, String>,
}

impl ReportParams {
    /// Создаёт пустой набор параметров.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Добавляет параметр (builder-style).
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.values.insert(key.into(), value.into());
        self
    }

    /// Возвращает значение параметра, если оно задано.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_roundtrip() {
        let p = ReportParams::new()
            .with("category", "upd")
            .with("date_from", "2026-06-01");
        assert_eq!(p.get("category"), Some("upd"));
        assert_eq!(p.get("missing"), None);
    }
}
