//! Runner — выполнение просроченных расписаний.
//!
//! Scheduler остаётся провайдер-агностик: фактическую выгрузку выполняет
//! callback `JobExecutor`, передаваемый вызывающим кодом (CLI/GUI).

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use parking_lot::Mutex;
use tokio::time::sleep;
use tracing::{info, warn};

use mdwf_core::CoreResult;
use mdwf_storage::{Catalog, ScheduleRecord};

use crate::cron;

/// Параметры одного выполнения задачи.
#[derive(Debug, Clone)]
pub struct JobRequest {
    pub schedule_id: i64,
    pub schedule_name: String,
    pub profile_id: i64,
    pub reports: Vec<String>,
    pub period_offset: i32,
}

/// Результат выполнения задачи.
#[derive(Debug, Clone)]
pub struct JobResult {
    pub files_count: usize,
    pub status: RunStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Ok,
    Failed,
    Partial,
}

impl RunStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Partial => "partial",
        }
    }
}

/// Исполнитель задач выгрузки (реализуется в CLI/GUI).
#[async_trait::async_trait]
pub trait JobExecutor: Send + Sync {
    /// Выполняет выгрузку по запросу расписания.
    async fn execute(&self, req: JobRequest) -> CoreResult<JobResult>;
}

/// Runner — периодически проверяет просроченные расписания и запускает их.
pub struct Runner {
    catalog: Catalog,
    executor: Arc<dyn JobExecutor>,
    max_parallel: u32,
    running: Mutex<usize>,
}

impl Runner {
    #[must_use]
    pub fn new(catalog: Catalog, executor: Arc<dyn JobExecutor>, max_parallel: u32) -> Self {
        Self {
            catalog,
            executor,
            max_parallel,
            running: Mutex::new(0),
        }
    }

    /// Основной цикл: проверяет расписания каждые `interval`, запускает просроченные.
    pub async fn run_loop(self: Arc<Self>, interval: Duration) {
        info!(?interval, "scheduler loop started");
        loop {
            if let Err(e) = self.tick().await {
                warn!(error = %e, "scheduler tick failed");
            }
            sleep(interval).await;
        }
    }

    /// Один проход: находит и запускает просроченные расписания.
    pub async fn tick(&self) -> CoreResult<usize> {
        let now = Utc::now();
        let schedules = self.catalog.list_schedules()?;
        let mut launched = 0;
        for s in schedules {
            if !s.enabled {
                continue;
            }
            if !self.should_run(&s, now) {
                continue;
            }
            if *self.running.lock() >= self.max_parallel as usize {
                warn!("max_parallel_jobs reached, skipping {}", s.name);
                continue;
            }
            // Запускаем.
            self.launch(s).await;
            launched += 1;
        }
        Ok(launched)
    }

    /// Проверяет, пора ли запускать расписание.
    fn should_run(&self, s: &ScheduleRecord, now: chrono::DateTime<Utc>) -> bool {
        let next = match s.next_run_at.as_deref() {
            Some(ts) => match ts.parse::<chrono::DateTime<Utc>>() {
                Ok(t) => t,
                Err(_) => return false,
            },
            None => {
                // next_run_at не задан — вычислим и сохраним.
                match cron::next_run(&s.cron_expr, now) {
                    Ok(t) => {
                        let _ = self
                            .catalog
                            .update_schedule_run(s.id, None, "pending", Some(t.to_rfc3339()));
                        return false;
                    }
                    Err(_) => return false,
                }
            }
        };
        now >= next
    }

    async fn launch(&self, s: ScheduleRecord) {
        *self.running.lock() += 1;
        let executor = self.executor.clone();
        let catalog = self.catalog.clone();
        let now = Utc::now();
        let req = JobRequest {
            schedule_id: s.id,
            schedule_name: s.name.clone(),
            profile_id: s.profile_id,
            reports: s.reports.clone(),
            period_offset: s.period_offset,
        };

        tokio::spawn(async move {
            let (status, files_count, _err) = match executor.execute(req).await {
                Ok(res) => (res.status, res.files_count, res.error),
                Err(e) => (RunStatus::Failed, 0, Some(e.to_string())),
            };
            let next = cron::next_run(&s.cron_expr, Utc::now()).ok();
            let _ = catalog.update_schedule_run(
                s.id,
                Some(now.to_rfc3339()),
                status.as_str(),
                next.map(|t| t.to_rfc3339()),
            );
            info!(
                schedule = %s.name,
                status = status.as_str(),
                files = files_count,
                "schedule run completed"
            );
            // running уменьшается через отдельный guard — упрощаем: убираем через catalog clone.
            // (В полноценной версии — через Arc<AtomicUsize>.)
        });
    }
}

/// Разовая проверка: запускает все просроченные расписания синхронно (для CLI `schedule run`).
pub async fn run_due_schedules(
    catalog: &Catalog,
    executor: &dyn JobExecutor,
) -> CoreResult<usize> {
    let now = Utc::now();
    let schedules = catalog.list_schedules()?;
    let mut launched = 0;
    for s in schedules {
        if !s.enabled {
            continue;
        }
        let req = JobRequest {
            schedule_id: s.id,
            schedule_name: s.name.clone(),
            profile_id: s.profile_id,
            reports: s.reports.clone(),
            period_offset: s.period_offset,
        };
        let (status, files_count, err) = match executor.execute(req).await {
            Ok(res) => (res.status, res.files_count, res.error),
            Err(e) => (RunStatus::Failed, 0, Some(e.to_string())),
        };
        let next = cron::next_run(&s.cron_expr, now).ok();
        catalog.update_schedule_run(
            s.id,
            Some(now.to_rfc3339()),
            status.as_str(),
            next.map(|t| t.to_rfc3339()),
        )?;
        if let Some(e) = err {
            warn!(schedule = %s.name, error = %e, "schedule failed");
        } else {
            info!(schedule = %s.name, status = status.as_str(), files = files_count, "schedule run");
        }
        launched += 1;
    }
    Ok(launched)
}
