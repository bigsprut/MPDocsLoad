//! Детерминированные имена файлов (спец. §2.7.1: шаблон по умолчанию
//! `{provider}_{profile}_{report}_{period}.{ext}`).

use std::path::{Path, PathBuf};

use mdwf_core::CoreResult;

use crate::error::StorageError;

/// Контекст для построения имени файла.
#[derive(Debug, Clone)]
pub struct FileNameContext<'a> {
    pub provider_id: &'a str,
    pub profile_name: &'a str,
    pub report_type: &'a str,
    pub period: Option<&'a str>,
    pub extension: &'a str,
    /// Идентификатор документа (для Browsable-режима, где несколько файлов).
    pub document_id: Option<&'a str>,
    /// Дата документа (для Browsable).
    pub document_date: Option<&'a str>,
}

impl FileNameContext<'_> {
    /// Строит имя файла по шаблону.
    ///
    /// Поддерживаемые плейсхолдеры: `{provider}`, `{profile}`, `{report}`,
    /// `{period}`, `{ext}`, `{doc_id}`, `{doc_date}`.
    /// Незаданные значения заменяются на `"unknown"`. Сегменты со значением
    /// `"unknown"` (для незаданных `doc_id`/`period`) вырезаются из имени,
    /// а повторы разделителей `_` и пустые сегменты схлопываются — чтобы
    /// шаблон с `{doc_id}` не портил имена Period-отчётов (где doc_id нет).
    #[must_use]
    pub fn render(&self, template: &str) -> String {
        let provider = sanitize(self.provider_id);
        let profile = sanitize(self.profile_name);
        let report = sanitize(self.report_type);
        let period = sanitize(self.period.unwrap_or("unknown"));
        let doc_id = sanitize(self.document_id.unwrap_or("unknown"));
        let doc_date = sanitize(self.document_date.unwrap_or("nodate"));
        let raw = template
            .replace("{provider}", &provider)
            .replace("{profile}", &profile)
            .replace("{report}", &report)
            .replace("{period}", &period)
            .replace("{ext}", self.extension)
            .replace("{doc_id}", &doc_id)
            .replace("{doc_date}", &doc_date);
        normalize_name(&raw)
    }

    /// Полный путь к файлу.
    pub fn path(&self, base_dir: &Path, template: &str) -> CoreResult<PathBuf> {
        let name = self.render(template);
        if name.is_empty() {
            return Err(StorageError::InvalidTemplateName.into());
        }
        Ok(base_dir.join(name))
    }
}

/// Очищает строку от символов, недопустимых в именах файлов (Windows + POSIX).
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

/// Нормализует итоговое имя файла после подстановки плейсхолдеров:
/// - вырезает сегменты-маркеры `"unknown"` (от незаполненных `{doc_id}`/`{period}`)
///   вместе с соседним разделителем `_`;
/// - схлопывает повторы `_` и убирает подчёркивания в начале/конце;
/// - схлопывает повторы точек `..` → `.` (защищает расширение).
///
/// Гарантирует, что шаблон с `{doc_id}` не портит имена Period-отчётов,
/// у которых нет идентификатора документа: вместо `report__2026-06.json`
/// получается `report_2026-06.json`.
fn normalize_name(name: &str) -> String {
    // Сначала вырежем сегменты "unknown" между подчёркиваниями: заменим
    // паттерны _unknown_ , _unknown (в конце), unknown_ (в начале) на _,
    // оставляя разделитель для последующего схлопывания.
    let mut s = name.to_string();
    // Повторяем: замена может создавать новые стыки.
    for _ in 0..4 {
        let prev = s.clone();
        s = s.replace("_unknown_", "_");
        s = s.replace("_unknown", "");
        s = s.replace("unknown_", "");
        // Отдельный "unknown" без соседей (весь сегмент имени).
        if s == "unknown" {
            s.clear();
        }
        if s == prev {
            break;
        }
    }

    // Схлопнуть повторы подчёркивания и обрезать по краям.
    while s.contains("__") {
        s = s.replace("__", "_");
    }
    let s = s.trim_matches('_');

    // Схлопнуть повторы точек (защита расширения: "..json" -> ".json").
    let mut s = s.to_string();
    while s.contains("..") {
        s = s.replace("..", ".");
    }

    if s.is_empty() {
        // Ничего осмысленного не осталось — вернём safe-имя.
        "file.unknown".to_string()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_default_template() {
        let ctx = FileNameContext {
            provider_id: "ozon",
            profile_name: "Ozon-1",
            report_type: "realization",
            period: Some("2026-06"),
            extension: "csv",
            document_id: None,
            document_date: None,
        };
        assert_eq!(
            ctx.render("{provider}_{profile}_{report}_{period}.{ext}"),
            "ozon_Ozon-1_realization_2026-06.csv"
        );
    }

    #[test]
    fn sanitize_dangerous_chars() {
        let ctx = FileNameContext {
            provider_id: "oz",
            profile_name: "a/b:c",
            report_type: "r",
            period: Some("2026-06"),
            extension: "xml",
            document_id: None,
            document_date: None,
        };
        // двоеточие/слеш заменяются на _
        assert!(ctx.render("{profile}").contains("a_b_c"));
    }

    #[test]
    fn browsable_doc_id() {
        let ctx = FileNameContext {
            provider_id: "wildberries",
            profile_name: "WB",
            report_type: "documents",
            period: None,
            extension: "xml",
            document_id: Some("12345"),
            document_date: Some("2026-06-15"),
        };
        assert_eq!(
            ctx.render("{provider}_{report}_{doc_id}_{doc_date}.{ext}"),
            "wildberries_documents_12345_2026-06-15.xml"
        );
    }

    /// Дефолтный шаблон с {doc_id}, но для Period-отчёта (doc_id=None):
    /// сегмент unknown должен вырезаться, без двойных подчёркиваний.
    #[test]
    fn period_report_with_doc_id_placeholder() {
        let ctx = FileNameContext {
            provider_id: "ozon",
            profile_name: "Ozon-1",
            report_type: "realization",
            period: Some("2026-06"),
            extension: "json",
            document_id: None, // Period-отчёт — документа нет
            document_date: None,
        };
        assert_eq!(
            ctx.render("{provider}_{profile}_{report}_{doc_id}_{period}.{ext}"),
            "ozon_Ozon-1_realization_2026-06.json"
        );
    }

    /// Документ WB: doc_id = человекочитаемое имя, period=None → unknown вырезается.
    #[test]
    fn wb_document_name_no_period() {
        let ctx = FileNameContext {
            provider_id: "wildberries",
            profile_name: "Профиль",
            report_type: "wb.documents",
            period: None,
            extension: "xml",
            document_id: Some("УПД №123"),
            document_date: None,
        };
        assert_eq!(
            ctx.render("{provider}_{profile}_{report}_{doc_id}_{period}.{ext}"),
            "wildberries_Профиль_wb.documents_УПД №123.xml"
        );
    }

    /// Все опциональные поля None → unknown-сегменты вырезаются, остаётся
    /// безопасное имя без мусора.
    #[test]
    fn all_optional_none() {
        let ctx = FileNameContext {
            provider_id: "wb",
            profile_name: "P",
            report_type: "r",
            period: None,
            extension: "zip",
            document_id: None,
            document_date: None,
        };
        assert_eq!(
            ctx.render("{provider}_{profile}_{report}_{doc_id}_{period}.{ext}"),
            "wb_P_r.zip"
        );
    }
}
