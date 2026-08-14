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
    running: Arc<Mutex<usize>>,
}

/// RAII-защита счётчика running: декремент при выходе из области (в т.ч. при
/// панике внутри задачи). Без этого max_parallel_jobs навсегда залипал после
/// N запусков (инкремент был, декремента не было).
struct RunningGuard(Arc<Mutex<usize>>);
impl Drop for RunningGuard {
    fn drop(&mut self) {
        *self.0.lock() = self.0.lock().saturating_sub(1);
    }
}

impl Runner {
    #[must_use]
    pub fn new(catalog: Catalog, executor: Arc<dyn JobExecutor>, max_parallel: u32) -> Self {
        Self {
            catalog,
            executor,
            max_parallel,
            running: Arc::new(Mutex::new(0)),
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
            // Запускаем (claim внутри может отдать расписание другому процессу).
            if self.launch(s).await {
                launched += 1;
            }
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

    /// Запускает расписание. Возвращает false, если другой процесс (CLI
    /// `schedule run` из Windows Task Scheduler) уже забрал его через claim.
    async fn launch(&self, s: ScheduleRecord) -> bool {
        let now = Utc::now();
        let Ok(next) = cron::next_run(&s.cron_expr, now) else {
            return false;
        };
        // CLAIM: атомарно забираем (защита от двойного выполнения с CLI schedule run).
        match self
            .catalog
            .claim_schedule(s.id, &next.to_rfc3339(), &now.to_rfc3339())
        {
            Ok(true) => {}
            _ => return false,
        }
        *self.running.lock() += 1;
        let executor = self.executor.clone();
        let catalog = self.catalog.clone();
        // Guard декрементирует running по завершении задачи (нормальном или панике).
        let guard = RunningGuard(self.running.clone());
        let req = JobRequest {
            schedule_id: s.id,
            schedule_name: s.name.clone(),
            profile_id: s.profile_id,
            reports: s.reports.clone(),
            period_offset: s.period_offset,
        };

        tokio::spawn(async move {
            let _guard = guard; // удерживаем до конца задачи
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
        });
        true
    }
}

/// Разовая проверка: запускает просроченные расписания синхронно (для CLI
/// `schedule run`, в т.ч. из Windows Task Scheduler). Due-проверка + claim
/// (защита от двойного выполнения с in-process Runner GUI).
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
        // Due-проверка: next_run_at <= now. None — вычисляем, пропускаем в этот раз.
        let Some(ts) = s.next_run_at.as_deref() else {
            if let Ok(t) = cron::next_run(&s.cron_expr, now) {
                let _ = catalog.update_schedule_run(s.id, None, "pending", Some(t.to_rfc3339()));
            }
            continue;
        };
        let due = ts
            .parse::<chrono::DateTime<Utc>>()
            .map(|t| now >= t)
            .unwrap_or(false);
        if !due {
            continue;
        }
        // CLAIM: атомарно забираем (другой процесс мог забрать между list и здесь).
        let Ok(next) = cron::next_run(&s.cron_expr, now) else {
            continue;
        };
        if !catalog.claim_schedule(s.id, &next.to_rfc3339(), &now.to_rfc3339())? {
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
        // next уже выставлен claim'ом; перезаписываем last + status тем же next.
        catalog.update_schedule_run(
            s.id,
            Some(now.to_rfc3339()),
            status.as_str(),
            Some(next.to_rfc3339()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use mdwf_storage::NewSchedule;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn cat() -> Catalog {
        Catalog::open_in_memory().expect("in-memory catalog")
    }

    fn mk_schedule(cat: &Catalog, name: &str, enabled: bool, next: Option<&str>) -> i64 {
        // Уникальное имя профиля на расписание (UNIQUE на profiles.name).
        let pid = cat
            .upsert_profile(&mdwf_core::Profile::new(format!("p-{name}"), "ozon"))
            .unwrap();
        cat.upsert_schedule(&NewSchedule {
            id: None,
            name: name.into(),
            profile_id: pid,
            reports: vec!["ozon.realization".into()],
            cron_expr: "0 3 1 * *".into(),
            period_offset: -1,
            params: None,
            enabled,
            next_run_at_ts: next.map(str::to_string),
        })
        .unwrap()
    }

    /// Счётчик исполнений (fake executor).
    struct CountingExecutor(AtomicUsize);
    #[async_trait::async_trait]
    impl JobExecutor for CountingExecutor {
        async fn execute(&self, _req: JobRequest) -> CoreResult<JobResult> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(JobResult { files_count: 1, status: RunStatus::Ok, error: None })
        }
    }

    const PAST: &str = "2020-01-01T00:00:00+00:00";

    #[tokio::test]
    async fn run_due_executes_only_due_enabled() {
        let c = cat();
        mk_schedule(&c, "due", true, Some(PAST));
        mk_schedule(&c, "off", false, Some(PAST)); // выключено — не выполняется
        mk_schedule(&c, "future", true, Some("2099-01-01T00:00:00+00:00"));
        mk_schedule(&c, "none", true, None); // нет next — только pending

        let ex = CountingExecutor(AtomicUsize::new(0));
        let n = run_due_schedules(&c, &ex).await.unwrap();
        assert_eq!(n, 1, "только due+enabled");
        assert_eq!(ex.0.load(Ordering::SeqCst), 1);

        // Повторный прогон: next у «due» уже продвинут claim'ом — не дубль.
        let n2 = run_due_schedules(&c, &ex).await.unwrap();
        assert_eq!(n2, 0, "claim продвинул next_run — повтор не выполняется");
        assert_eq!(ex.0.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn none_next_computed_and_pending() {
        let c = cat();
        let id = mk_schedule(&c, "s", true, None);
        let ex = CountingExecutor(AtomicUsize::new(0));
        let n = run_due_schedules(&c, &ex).await.unwrap();
        assert_eq!(n, 0, "без next_run_at выполняться не должен");
        assert_eq!(ex.0.load(Ordering::SeqCst), 0);
        let rec = c.list_schedules().unwrap().into_iter().find(|s| s.id == id).unwrap();
        assert!(rec.next_run_at.is_some(), "next вычислен и сохранён (pending)");
    }

    #[tokio::test]
    async fn claim_is_atomic_single_winner() {
        let c = cat();
        let id = mk_schedule(&c, "s", true, Some(PAST));
        let now = "2026-01-01T00:00:00+00:00";
        let next = "2026-02-01T00:00:00+00:00";
        // Первый claim забирает, второй (тот же момент) — нет.
        assert!(c.claim_schedule(id, next, now).unwrap());
        assert!(!c.claim_schedule(id, next, now).unwrap(), "двойное выполнение запрещено");
    }

    #[tokio::test]
    async fn failed_executor_marks_status() {
        struct Failing;
        #[async_trait::async_trait]
        impl JobExecutor for Failing {
            async fn execute(&self, _req: JobRequest) -> CoreResult<JobResult> {
                Err(mdwf_core::CoreError::Internal("boom".into()))
            }
        }
        let c = cat();
        let id = mk_schedule(&c, "s", true, Some(PAST));
        let n = run_due_schedules(&c, &Failing).await.unwrap();
        assert_eq!(n, 1, "задача считается запущенной");
        let rec = c.list_schedules().unwrap().into_iter().find(|s| s.id == id).unwrap();
        assert_eq!(rec.last_run_status.as_deref(), Some("failed"));
        assert!(rec.next_run_at.is_some(), "next продвинут — не залипнет");
    }
}
