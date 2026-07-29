//! Команда `schedule` — управление расписаниями планировщика.

use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;

use mdwf_core::{NoopProgress, ReportParams};
use mdwf_scheduler::{
    disable_autostart, enable_autostart, is_autostart_enabled, run_due_schedules, JobExecutor,
    JobRequest, JobResult, RunStatus,
};
use mdwf_storage::{NewSchedule};

use crate::commands::Context;
use crate::exit_code::ExitCode;
use crate::ScheduleCmd;

pub async fn run(ctx: &Context, action: ScheduleCmd) -> Result<ExitCode> {
    match action {
        ScheduleCmd::List => {
            let schedules = ctx.catalog.list_schedules()?;
            if schedules.is_empty() {
                println!("Расписаний нет.");
            } else {
                println!("{:<20} {:<20} {:<15} {:<10} {}", "Имя", "Профиль(id)", "Cron", "Вкл", "Следующий запуск");
                println!("{}", "-".repeat(85));
                for s in schedules {
                    println!(
                        "{:<20} {:<20} {:<15} {:<10} {}",
                        s.name,
                        s.profile_id,
                        s.cron_expr,
                        if s.enabled { "да" } else { "нет" },
                        s.next_run_at.as_deref().unwrap_or("-")
                    );
                }
            }
            Ok(ExitCode::Success)
        }
        ScheduleCmd::Add {
            name,
            profile,
            report,
            cron,
            period_offset,
            disabled,
        } => {
            // Валидация cron.
            mdwf_scheduler::parse(&cron)?;

            // Профиль должен существовать.
            let p = ctx
                .catalog
                .get_profile_by_name(&profile)?
                .ok_or_else(|| anyhow::anyhow!("профиль '{profile}' не найден"))?;
            let profile_id = p.id.ok_or_else(|| anyhow::anyhow!("профиль без id"))?;

            let next = mdwf_scheduler::next_run(&cron, Utc::now())?;
            let new_sched = NewSchedule {
                id: None,
                name: name.clone(),
                profile_id,
                reports: report.clone(),
                cron_expr: cron.clone(),
                period_offset,
                params: None,
                enabled: !disabled,
                next_run_at_ts: Some(next.to_rfc3339()),
            };
            let id = ctx.catalog.upsert_schedule(&new_sched)?;
            println!(
                "Расписание '{name}' создано (id={id}, cron='{cron}', отчётов={}, следующий запуск: {next})",
                report.len()
            );
            Ok(ExitCode::Success)
        }
        ScheduleCmd::Delete { name } => {
            ctx.catalog.delete_schedule(&name)?;
            println!("Расписание '{name}' удалено.");
            Ok(ExitCode::Success)
        }
        ScheduleCmd::Run => {
            println!("Запуск просроченных расписаний...");
            let executor = Arc::new(CliJobExecutor::new(ctx));
            let launched = run_due_schedules(&ctx.catalog, executor.as_ref()).await?;
            println!("Выполнено расписаний: {launched}.");
            Ok(ExitCode::Success)
        }
        ScheduleCmd::Autostart { enable, disable } => {
            if enable && disable {
                println!("Укажите только один флаг: --enable или --disable.");
                return Ok(ExitCode::UsageError);
            }
            if enable {
                enable_autostart()?;
                println!("Автозапуск включён (ключ HKCU Run).");
            } else if disable {
                disable_autostart()?;
                println!("Автозапуск выключен.");
            } else {
                let enabled = is_autostart_enabled();
                println!("Автозапуск: {}", if enabled { "включён" } else { "выключен" });
            }
            Ok(ExitCode::Success)
        }
    }
}

/// Исполнитель задач выгрузки для CLI scheduler.
struct CliJobExecutor<'a> {
    ctx: &'a Context,
}

impl<'a> CliJobExecutor<'a> {
    fn new(ctx: &'a Context) -> Self {
        Self { ctx }
    }
}

#[async_trait::async_trait]
impl JobExecutor for CliJobExecutor<'_> {
    async fn execute(&self, req: JobRequest) -> mdwf_core::CoreResult<JobResult> {
        let profiles = self.ctx.catalog.list_profiles()?;
        let profile = profiles
            .into_iter()
            .find(|p| p.id == Some(req.profile_id))
            .ok_or_else(|| mdwf_core::CoreError::ProfileNotFound(format!("id={}", req.profile_id)))?;

        let provider = self.ctx.registry.require(&profile.provider_id)?;
        let auth = provider.authenticator(&profile).await?;
        let progress = Arc::new(NoopProgress) as Arc<dyn mdwf_core::ProgressCallback>;

        let mut total_files = 0usize;
        let mut had_error = false;
        for report_type in &req.reports {
            let report = match provider.report(report_type).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(report = %report_type, error = %e, "report not available");
                    had_error = true;
                    continue;
                }
            };
            let params = ReportParams::new();
            match report
                .download(auth.as_ref(), &params, progress.clone(), mdwf_core::CancelToken::new())
                .await
            {
                Ok(files) => {
                    total_files += files.len();
                }
                Err(e) => {
                    tracing::warn!(report = %report_type, error = %e, "download failed");
                    had_error = true;
                }
            }
        }

        let status = if had_error && total_files > 0 {
            RunStatus::Partial
        } else if had_error {
            RunStatus::Failed
        } else {
            RunStatus::Ok
        };
        Ok(JobResult {
            files_count: total_files,
            status,
            error: None,
        })
    }
}
