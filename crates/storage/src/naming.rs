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
    /// Незаданные значения заменяются на `"unknown"`.
    #[must_use]
    pub fn render(&self, template: &str) -> String {
        let provider = sanitize(self.provider_id);
        let profile = sanitize(self.profile_name);
        let report = sanitize(self.report_type);
        let period = sanitize(self.period.unwrap_or("unknown"));
        let doc_id = sanitize(self.document_id.unwrap_or(""));
        let doc_date = sanitize(self.document_date.unwrap_or("nodate"));
        template
            .replace("{provider}", &provider)
            .replace("{profile}", &profile)
            .replace("{report}", &report)
            .replace("{period}", &period)
            .replace("{ext}", self.extension)
            .replace("{doc_id}", &doc_id)
            .replace("{doc_date}", &doc_date)
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
}
