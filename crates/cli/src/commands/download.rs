//! Команда `download` — выгрузка отчётов.

use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;

use mdwf_core::{DownloadedFile, NoopProgress, ReportParams};
use mdwf_storage::FileNameContext;

use crate::commands::Context;
use crate::exit_code::ExitCode;
use crate::DownloadArgs;

pub async fn run(ctx: &Context, args: DownloadArgs) -> Result<ExitCode> {
    let profile = ctx.profile_with_secrets(&args.profile).await?;
    let provider = ctx.registry.require(&profile.provider_id)?;
    let auth = provider.authenticator(&profile).await?;

    let mut total = 0usize;
    let mut errors = 0usize;
    for report_type in &args.report {
        println!("Выгрузка отчёта '{report_type}'...");
        let report = match provider.report(report_type).await {
            Ok(r) => r,
            Err(e) => {
                println!("  ошибка: {e}");
                errors += 1;
                continue;
            }
        };

        let mut params = ReportParams::new();
        if let Some(p) = &args.period {
            params.period = Some(p.clone());
        }
        if let Some(ids) = &args.ids {
            params = params.with("ids", ids);
        }
        if let Some(cat) = &args.category {
            params = params.with("category", cat);
        }

        let progress = Arc::new(NoopProgress) as Arc<dyn mdwf_core::ProgressCallback>;
        match report
            .download(
                auth.as_ref(),
                &params,
                progress,
                mdwf_core::CancelToken::new(),
            )
            .await
        {
            Ok(files) => {
                let saved = persist(ctx, &files, &profile.provider_id, &args.profile, report_type, &params)?;
                println!("  скачано файлов: {}; записано: {}", files.len(), saved);
                total += saved;
            }
            Err(e) => {
                println!("  ошибка выгрузки: {e}");
                errors += 1;
            }
        }
    }

    println!("\nГотово. Всего файлов: {total}, ошибок: {errors}.");
    if errors > 0 && total > 0 {
        Ok(ExitCode::PartialSuccess)
    } else if errors > 0 {
        Ok(ExitCode::ApiError)
    } else {
        Ok(ExitCode::Success)
    }
}

/// Записывает файлы на диск через FileStore и регистрирует в каталоге.
fn persist(
    ctx: &Context,
    files: &[DownloadedFile],
    provider_id: &str,
    profile_name: &str,
    report_type: &str,
    params: &ReportParams,
) -> Result<usize> {
    let profile = ctx
        .catalog
        .get_profile_by_name(profile_name)?
        .ok_or_else(|| anyhow::anyhow!("профиль не найден"))?;
    let profile_id = profile
        .id
        .ok_or_else(|| anyhow::anyhow!("профиль без id"))?;

    let mut count = 0usize;
    for f in files {
        let content = f
            .content
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("файл без контента"))?;
        let ctx_file = FileNameContext {
            provider_id,
            profile_name,
            report_type,
            period: params.period.as_deref(),
            extension: &f.extension,
            document_id: f.source_id.as_deref(),
            document_date: None,
        };
        let stored = ctx.file_store.save(content, &ctx_file)?;
        println!("    → {} ({} байт)", stored.file_name, stored.size);

        let new_dl = mdwf_storage::NewDownload {
            profile_id,
            report_type: report_type.to_string(),
            period: params.period.clone(),
            params: Some(serde_json::to_string(params).unwrap_or_default()),
            file_path: format!("{} ({} байт)", stored.file_name, stored.size),
            file_size: {
                #[allow(clippy::cast_possible_wrap)]
                {
                    stored.size as i64
                }
            },
            file_hash: stored.sha256.clone(),
            file_format: stored.extension.clone(),
            rows_count: None,
            downloader_kind: "Api".to_string(),
            source_url: stored.source_url.clone(),
            document_id: None, // CLI не использует значок «уже загружен» (только GUI).
        };
        ctx.catalog.record_download(&new_dl)?;
        count += 1;
    }
    let _ = Utc::now(); // suppress unused
    Ok(count)
}
