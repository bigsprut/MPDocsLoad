//! Команда `schedule` — управление расписаниями планировщика.

use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;

use mdwf_core::ReportParams;
use mdwf_scheduler::{
    disable_autostart, enable_autostart, is_autostart_enabled, run_due_schedules, JobExecutor,
    JobRequest, JobResult, RunStatus,
};
use mdwf_storage::NewSchedule;

use crate::commands::{journal_subject, journal_write, Context};
use crate::exit_code::ExitCode;
use crate::ScheduleCmd;

pub async fn run(ctx: &Context, action: ScheduleCmd) -> Result<ExitCode> {
    match action {
        ScheduleCmd::List => {
            let schedules = ctx.catalog.list_schedules()?;
            if schedules.is_empty() {
                println!("Расписаний нет.");
            } else {
                println!("{:<20} {:<20} {:<15} {:<10} {}", "Имя", "Профиль(id)", "Расписание", "Вкл", "Следующий запуск");
                println!("{}", "-".repeat(85));
                for s in schedules {
                    println!(
                        "{:<20} {:<20} {:<15} {:<10} {}",
                        s.name,
                        s.profile_id,
                        s.cron_expr,
                        if s.enabled { "да" } else { "нет" },
                        s.next_run_at
                            .as_deref().map_or_else(|| "-".into(), mdwf_scheduler::fmt_local)
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
        ScheduleCmd::Run { by_task } => {
            println!("Запуск просроченных расписаний...");
            let launched = run_due_once(ctx, by_task).await?;
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

/// Один цикл запуска просроченных расписаний (`schedule run`). Общая точка
/// входа для CLI-команды и скрытого режима GUI (`mdwf-gui --schedule-run`,
/// задача Windows Task Scheduler): подставляет период из `period_offset`,
/// сохраняет файлы (FileStore + каталог), пишет журнал с источником
/// «задача Windows» / «запуск из CLI».
pub async fn run_due_once(ctx: &Context, by_task: bool) -> Result<usize> {
    let executor = Arc::new(CliJobExecutor::new(ctx, by_task));
    run_due_schedules(&ctx.catalog, executor.as_ref())
        .await
        .map_err(|e| anyhow::anyhow!(e))
}

/// Исполнитель задач выгрузки для CLI scheduler (`mdwf schedule run` —
/// фоновая задача Windows Task Scheduler или ручной запуск из терминала).
///
/// Как и GUI-исполнитель: подставляет период из `period_offset`, записывает
/// файлы на диск (FileStore) + регистрирует в каталоге и пишет в журнал
/// событий с источником (задача Windows / запуск из CLI).
struct CliJobExecutor<'a> {
    ctx: &'a Context,
    /// true — запуск фоновой задачей Windows (флаг --by-task).
    by_task: bool,
}

impl<'a> CliJobExecutor<'a> {
    fn new(ctx: &'a Context, by_task: bool) -> Self {
        Self { ctx, by_task }
    }
}

#[async_trait::async_trait]
impl JobExecutor for CliJobExecutor<'_> {
    async fn execute(&self, req: JobRequest) -> mdwf_core::CoreResult<JobResult> {
        let (profile_name, provider_id) = {
            let profiles = self.ctx.catalog.list_profiles()?;
            let p = profiles
                .into_iter()
                .find(|p| p.id == Some(req.profile_id))
                .ok_or_else(|| {
                    mdwf_core::CoreError::ProfileNotFound(format!("id={}", req.profile_id))
                })?;
            (p.name, p.provider_id)
        };

        let provider = self.ctx.registry.require(&provider_id)?;
        // Подмешиваем секреты из keyring перед authenticator.
        let caps = provider.capabilities();
        let secret_fields = mdwf_secrets::secret_field_ids(caps);
        let profile = mdwf_secrets::load_profile_secrets(
            self.ctx
                .catalog
                .get_profile_by_name(&profile_name)?
                .ok_or_else(|| {
                    mdwf_core::CoreError::ProfileNotFound(profile_name.clone())
                })?,
            &secret_fields,
            self.ctx.secrets.as_ref(),
        )
        .await?;
        let auth = provider.authenticator(&profile).await?;
        let progress =
            Arc::new(mdwf_core::NoopProgress) as Arc<dyn mdwf_core::ProgressCallback>;

        // Период из смещения расписания (как в GUI-исполнителе): без этого
        // месячные отчёты уходили без периода и падали/возвращали пустоту.
        let period = mdwf_scheduler::period_for_offset(req.period_offset);
        let origin = if self.by_task {
            mdwf_core::LogOrigin::ScheduleWinTask(req.schedule_name.clone())
        } else {
            mdwf_core::LogOrigin::ScheduleCliRun(req.schedule_name.clone())
        };

        let display_names: std::collections::HashMap<String, String> = caps
            .reports
            .iter()
            .map(|r| (r.type_id.clone(), r.display_name.clone()))
            .collect();

        let mut total_files = 0usize;
        let mut had_error = false;
        for report_type in &req.reports {
            let subject = journal_subject(
                display_names.get(report_type).map(String::as_str),
                report_type,
                Some(period.as_str()),
            );
            journal_write(
                self.ctx,
                "info",
                &origin,
                &format!("Скачивание {subject} — {profile_name}"),
            );
            let report = match provider.report(report_type).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(report = %report_type, error = %e, "report not available");
                    journal_write(
                        self.ctx,
                        "error",
                        &origin,
                        &format!("{subject}: {e}"),
                    );
                    had_error = true;
                    continue;
                }
            };
            let params = ReportParams {
                period: Some(period.clone()),
                ..Default::default()
            };
            match report
                .download(
                    auth.as_ref(),
                    &params,
                    progress.clone(),
                    mdwf_core::CancelToken::new(),
                )
                .await
            {
                Ok(files) => {
                    // Записываем на диск + каталог (раньше файлы выбрасывались —
                    // фоновые запуски по задаче Windows теряли выгрузки).
                    let saved_paths = match crate::commands::download::persist(
                        self.ctx,
                        &files,
                        &profile.provider_id,
                        &profile_name,
                        report_type,
                        &params,
                    ) {
                        Ok(p) => p,
                        Err(e) => {
                            journal_write(
                                self.ctx,
                                "error",
                                &origin,
                                &format!("{subject}: запись на диск не удалась: {e}"),
                            );
                            had_error = true;
                            continue;
                        }
                    };
                    let note = files.iter().find_map(|f| f.note.clone()).unwrap_or_default();
                    let note_suffix = if note.is_empty() {
                        String::new()
                    } else {
                        format!(" — внимание: {note}")
                    };
                    total_files += saved_paths.len();
                    crate::commands::journal_write_report(
                        self.ctx,
                        "success",
                        &origin,
                        &format!(
                            "{subject}: скачано {} файл(ов){}{}",
                            files.len(),
                            mdwf_core::journal::paths_suffix(&saved_paths),
                            note_suffix
                        ),
                        saved_paths.first().map_or("", |s| s.as_str()),
                        report_type,
                    );
                }
                Err(e) => {
                    tracing::warn!(report = %report_type, error = %e, "download failed");
                    journal_write(self.ctx, "error", &origin, &format!("{subject}: {e}"));
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
        let period_desc = mdwf_core::describe_report_period(Some(period.as_str()))
            .map_or_else(String::new, |p| format!(" ({p})"));
        journal_write(
            self.ctx,
            if had_error { "error" } else { "success" },
            &origin,
            &format!(
                "Расписание «{}»{period_desc}: {} файл(ов){}",
                req.schedule_name,
                total_files,
                if had_error { ", ошибки есть" } else { "" }
            ),
        );
        Ok(JobResult {
            files_count: total_files,
            status,
            error: None,
        })
    }
}
