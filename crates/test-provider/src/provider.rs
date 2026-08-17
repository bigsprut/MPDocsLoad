//! Реализация TestProvider — фейковый провайдер для разработки GUI/CLI.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Duration, Utc};
use mdwf_core::{
    AcquisitionMode, AuthField, AuthFieldKind, AuthType, Authenticator, Capabilities,
    CancelToken, CoreResult, DocumentEntry, DocumentFilter, DownloadedFile, DownloaderKind,
    HealthStatus, MarketplaceProvider, Profile, ProgressCallbackRef, Report, ReportCategory,
    ReportDescriptor, ReportParameter, ReportParams,
};
use parking_lot::RwLock;

/// Фейковый аутентификатор (без реальной авторизации).
#[derive(Debug, Default)]
pub struct TestAuthenticator;

#[async_trait]
impl Authenticator for TestAuthenticator {
    fn apply(
        &self,
        req: reqwest::RequestBuilder,
    ) -> reqwest::RequestBuilder {
        req.header("X-Test-Provider", "mdwf-test")
    }
    fn expires_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        // Истекает далеко в будущем.
        Some(Utc::now() + Duration::days(365))
    }
    fn auth_type(&self) -> AuthType {
        AuthType::ApiKey
    }
    fn describe(&self) -> String {
        "TestProvider (mock)".into()
    }
}

/// Фейковый отчёт. Может работать в обоих режимах (Period/Browsable).
pub struct TestReport {
    type_id: String,
    display_name: String,
    category: ReportCategory,
    mode: AcquisitionMode,
}

impl TestReport {
    /// Browsable-отчёт (список → выбор → скачивание).
    #[must_use]
    pub fn browsable(type_id: &str, display_name: &str, category: ReportCategory) -> Self {
        Self {
            type_id: type_id.into(),
            display_name: display_name.into(),
            category,
            mode: AcquisitionMode::Browsable,
        }
    }

    /// Period-отчёт (тип + период → генерация).
    #[must_use]
    pub fn period(type_id: &str, display_name: &str, category: ReportCategory) -> Self {
        Self {
            type_id: type_id.into(),
            display_name: display_name.into(),
            category,
            mode: AcquisitionMode::Period,
        }
    }
}

#[async_trait]
impl Report for TestReport {
    fn type_id(&self) -> &str {
        &self.type_id
    }
    fn display_name(&self) -> &str {
        &self.display_name
    }
    fn category(&self) -> ReportCategory {
        self.category
    }
    fn acquisition_mode(&self) -> AcquisitionMode {
        self.mode
    }
    fn downloader_kind(&self) -> DownloaderKind {
        DownloaderKind::Api
    }
    fn parameters(&self) -> &[ReportParameter] {
        &[]
    }

    async fn list(
        &self,
        _auth: &dyn Authenticator,
        filter: &DocumentFilter,
        _progress: ProgressCallbackRef,
        _cancel: CancelToken,
    ) -> CoreResult<Vec<DocumentEntry>> {
        // Генерируем N фейковых документов, зависящих от лимита фильтра.
        let count = filter.limit.unwrap_or(10).min(20) as usize;
        let today = Utc::now().date_naive();
        let mut entries = Vec::with_capacity(count);
        for i in 0..count {
            let mut e = DocumentEntry::new(
                format!("doc-{i:04}"),
                format!("{} №{i:04}", self.display_name),
            );
            e.category = filter.category.clone().unwrap_or_else(|| "test".into());
            #[allow(clippy::cast_possible_wrap)]
            let i_as_i64 = i as i64;
            e.date = Some(today - Duration::days(i_as_i64));
            e.extensions = vec!["xml".into(), "pdf".into()];
            e.size_hint = Some(1024 * (i as u64 + 1));
            entries.push(e);
        }
        Ok(entries)
    }

    async fn download(
        &self,
        _auth: &dyn Authenticator,
        params: &ReportParams,
        _progress: ProgressCallbackRef,
        _cancel: CancelToken,
    ) -> CoreResult<Vec<DownloadedFile>> {
        // Для Browsable: скачиваем выбранные id (через params.get("ids")).
        // Для Period: генерируем один файл по периоду.
        let ids_csv = params.get("ids");
        let mut files = Vec::new();
        if self.mode == AcquisitionMode::Browsable {
            let ids: Vec<&str> = ids_csv.map(|s| s.split(',').collect()).unwrap_or_default();
            for id in ids {
                let content = format!(
                    "<test-doc id=\"{id}\" from=\"{}\"/>\n",
                    self.type_id
                );
                files.push(DownloadedFile::with_content(
                    format!("{id}.xml"),
                    "xml",
                    content.into_bytes(),
                ));
            }
        } else {
            let period = params.period.clone().unwrap_or_else(|| "unknown".into());
            let content = format!("period report {} for {}\nrow1\nrow2\n", self.type_id, period);
            files.push(DownloadedFile::with_content(
                format!("{}.csv", self.type_id),
                "csv",
                content.into_bytes(),
            ));
        }
        Ok(files)
    }
}

/// Фейковый провайдер с предзаполненными отчётами.
pub struct TestProvider {
    reports: RwLock<Vec<Arc<TestReport>>>,
    capabilities: Capabilities,
}

impl Default for TestProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TestProvider {
    #[must_use]
    pub fn new() -> Self {
        let reports: Vec<Arc<TestReport>> = vec![
            Arc::new(TestReport::browsable(
                "test.documents",
                "Документы (список)",
                ReportCategory::Documents,
            )),
            Arc::new(TestReport::period(
                "test.realization",
                "Отчёт о реализации",
                ReportCategory::Finance,
            )),
        ];
        let descriptors = reports
            .iter()
            .map(|r| ReportDescriptor {
                type_id: r.type_id.clone(),
                display_name: r.display_name.clone(),
                category: r.category,
                acquisition_mode: r.mode,
                downloader_kind: DownloaderKind::Api,
                parameters: Vec::new(),
                period_kind: mdwf_core::PeriodKind::Range,
                description: Some(format!("Тестовый отчёт «{}» (mock).", r.display_name)),
                max_range_days: None,
                cabinet_path: None,
            cabinet_url: None,
            })
            .collect();
        let capabilities = Capabilities {
            auth_type: AuthType::ApiKey,
            auth_fields: vec![
                AuthField {
                    id: "name".into(),
                    label: "Название профиля".into(),
                    kind: AuthFieldKind::Text,
                    required: true,
                    placeholder: Some("Test-1".into()),
                    help_text: None,
                    secret: false,
                },
                AuthField {
                    id: "token".into(),
                    label: "Токен (фейковый)".into(),
                    kind: AuthFieldKind::Password,
                    required: false,
                    placeholder: None,
                    help_text: Some("Не используется — это mock".into()),
                    secret: true,
                },
            ],
            reports: descriptors,
        };
        Self {
            reports: RwLock::new(reports),
            capabilities,
        }
    }
}

#[async_trait]
impl MarketplaceProvider for TestProvider {
    fn id(&self) -> &'static str {
        "test"
    }
    fn display_name(&self) -> &'static str {
        "Test Provider (mock)"
    }
    fn version(&self) -> &'static str {
        "0.1.0"
    }
    fn docs_url(&self) -> &'static str {
        "about:blank"
    }
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    async fn authenticator(&self, _profile: &Profile) -> CoreResult<Arc<dyn Authenticator>> {
        Ok(Arc::new(TestAuthenticator))
    }

    async fn report(&self, report_type: &str) -> CoreResult<Arc<dyn Report>> {
        self.reports
            .read()
            .iter()
            .find(|r| r.type_id() == report_type)
            .cloned()
            .map(|r| r as Arc<dyn Report>)
            .ok_or_else(|| {
                mdwf_core::CoreError::ReportTypeNotSupported(report_type.to_string())
            })
    }

    async fn reports(&self) -> CoreResult<Vec<Arc<dyn Report>>> {
        Ok(self
            .reports
            .read()
            .iter()
            .cloned()
            .map(|r| r as Arc<dyn Report>)
            .collect())
    }

    async fn health_check(
        &self,
        _auth: &dyn Authenticator,
    ) -> CoreResult<HealthStatus> {
        Ok(HealthStatus::ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdwf_core::NoopProgress;

    #[tokio::test]
    async fn browsable_list_and_download() {
        let provider = TestProvider::new();
        let auth: Arc<dyn Authenticator> = Arc::new(TestAuthenticator);
        let report = provider.report("test.documents").await.unwrap();
        assert_eq!(report.acquisition_mode(), AcquisitionMode::Browsable);

        let filter = DocumentFilter {
            limit: Some(5),
            ..Default::default()
        };
        let entries = report
            .list(auth.as_ref(), &filter, std::sync::Arc::new(NoopProgress) as std::sync::Arc<dyn mdwf_core::ProgressCallback>, CancelToken::new())
            .await
            .unwrap();
        assert_eq!(entries.len(), 5);

        let params = ReportParams::new().with("ids", "doc-0000,doc-0001");
        let files = report
            .download(
                auth.as_ref(),
                &params,
                Arc::new(NoopProgress) as ProgressCallbackRef,
                CancelToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].extension, "xml");
    }

    #[tokio::test]
    async fn period_download() {
        let provider = TestProvider::new();
        let auth: Arc<dyn Authenticator> = Arc::new(TestAuthenticator);
        let report = provider.report("test.realization").await.unwrap();
        assert_eq!(report.acquisition_mode(), AcquisitionMode::Period);

        let params = ReportParams {
            period: Some("2026-06".into()),
            ..Default::default()
        };
        let files = report
            .download(
                auth.as_ref(),
                &params,
                Arc::new(NoopProgress) as ProgressCallbackRef,
                CancelToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].extension, "csv");
    }
}
