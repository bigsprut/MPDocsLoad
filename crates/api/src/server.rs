//! REST API сервер на axum (спец. future, feature `server`).

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tracing::info;

use mdwf_core::{CancelToken, NoopProgress, ProviderRegistry, ReportParams};
use mdwf_storage::Catalog;

/// Состояние приложения, разделяемое между обработчиками.
#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<ProviderRegistry>,
    pub catalog: Arc<Catalog>,
}

/// Запускает REST API сервер на указанном адресе.
pub async fn serve(state: AppState, addr: SocketAddr) -> Result<()> {
    let app = Router::new()
        .route("/api/v1/providers", get(list_providers))
        .route("/api/v1/reports/:provider_id", get(list_reports))
        .route("/api/v1/profiles", get(list_profiles))
        .route("/api/v1/download", post(download))
        .with_state(state);

    info!(%addr, "MDWF REST API listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn list_providers(State(state): State<AppState>) -> impl IntoResponse {
    let providers: Vec<ProviderInfo> = state
        .registry
        .list()
        .into_iter()
        .map(|p| ProviderInfo {
            id: p.id().to_string(),
            display_name: p.display_name().to_string(),
            version: p.version().to_string(),
            reports_count: p.capabilities().reports.len(),
        })
        .collect();
    Json(providers)
}

#[derive(Serialize)]
struct ProviderInfo {
    id: String,
    display_name: String,
    version: String,
    reports_count: usize,
}

async fn list_reports(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> Result<Json<Vec<ReportInfo>>, (StatusCode, String)> {
    let provider = state
        .registry
        .require(&provider_id)
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    let reports: Vec<ReportInfo> = provider
        .capabilities()
        .reports
        .iter()
        .map(|r| ReportInfo {
            type_id: r.type_id.clone(),
            display_name: r.display_name.clone(),
            browsable: r.acquisition_mode.is_browsable(),
        })
        .collect();
    Ok(Json(reports))
}

#[derive(Serialize)]
struct ReportInfo {
    type_id: String,
    display_name: String,
    browsable: bool,
}

async fn list_profiles(State(state): State<AppState>) -> Result<Json<Vec<ProfileInfo>>, (StatusCode, String)> {
    let profiles = state
        .catalog
        .list_profiles()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let out: Vec<ProfileInfo> = profiles
        .into_iter()
        .map(|p| ProfileInfo {
            id: p.id.unwrap_or(0),
            name: p.name,
            provider_id: p.provider_id,
        })
        .collect();
    Ok(Json(out))
}

#[derive(Serialize)]
struct ProfileInfo {
    id: i64,
    name: String,
    provider_id: String,
}

#[derive(Deserialize)]
struct DownloadRequest {
    profile_name: String,
    report_type: String,
    #[serde(default)]
    period: Option<String>,
    #[serde(default)]
    ids: Option<String>,
}

#[derive(Serialize)]
struct DownloadResponse {
    files: Vec<DownloadedFileInfo>,
}

#[derive(Serialize)]
struct DownloadedFileInfo {
    file_name: String,
    extension: String,
    size: u64,
}

async fn download(
    State(state): State<AppState>,
    Json(req): Json<DownloadRequest>,
) -> Result<Json<DownloadResponse>, (StatusCode, String)> {
    let profile = state
        .catalog
        .get_profile_by_name(&req.profile_name)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("profile '{}' not found", req.profile_name)))?;

    let provider = state
        .registry
        .require(&profile.provider_id)
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    let auth = provider
        .authenticator(&profile)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let report = provider
        .report(&req.report_type)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;

    let mut params = ReportParams::new();
    if let Some(p) = req.period {
        params.period = Some(p);
    }
    if let Some(ids) = req.ids {
        params = params.with("ids", ids);
    }

    let progress = Arc::new(NoopProgress) as Arc<dyn mdwf_core::ProgressCallback>;
    let files = report
        .download(auth.as_ref(), &params, progress, CancelToken::new())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let files: Vec<DownloadedFileInfo> = files
        .into_iter()
        .map(|f| DownloadedFileInfo {
            file_name: f.file_name,
            extension: f.extension,
            size: f.size,
        })
        .collect();
    Ok(Json(DownloadResponse { files }))
}
