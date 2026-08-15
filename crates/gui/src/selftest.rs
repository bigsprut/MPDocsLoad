//! `--self-test <scenario.json>` — event-level headless-драйвер GUI.
//!
//! Прогоняет сценарий `UiCommand` через ТОТ ЖЕ `run_command_loop`, что и живое
//! окно (GUI-кодpath целиком), но без GTK-окна, скриншотов и кликов: события
//! `UiEvent` собираются в отчёт JSON, ожидания сверяются программно.
//! Заменяет связку «скриншот → OCR → чтение картинки» (уроки #51, #59) для
//! всех проверок, где важна логика, а не пиксели.
//!
//! Изоляция: сценарий по умолчанию работает на ОТДЕЛЬНОМ каталоге
//! `<сценарий>.data/` (своя SQLite + папка выгрузок, секреты in-memory) —
//! журнал/загрузки пользователя не затрагиваются. `"isolated": false` —
//! реальный профильный каталог (для live-сценариев с реальными API).
//!
//! Сценарий (JSON, UTF-8):
//! ```json
//! {
//!   "name": "smoke",
//!   "isolated": true,
//!   "ensure_profile": {"provider": "test", "name": "drv"},
//!   "steps": [
//!     {"cmd": "load_providers"},
//!     {"cmd": "download", "provider": "test", "profile": "drv",
//!      "report": "test.realization", "period": "2026-07",
//!      "wait_event": "DownloadFinished", "timeout_ms": 60000}
//!   ],
//!   "expect": [
//!     {"event": "DownloadFinished", "contains": "ok"},
//!     {"event": "Log", "contains": "скачано 1 файл(ов)"},
//!     {"journal_contains": "скачано 1 файл(ов)"}
//!   ]
//! }
//! ```
//!
//! Отчёт: `<сценарий>.report.json` (+ краткий итог в stderr). Exit-код 0 —
//! все ожидания сошлись, 1 — нет/ошибка. Запуск:
//! `mdwf-gui.exe --self-test scripts/selftest/smoke.json`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use mdwf_config::ProvisionedConfig;
use mdwf_core::{Profile, ReportParams};

use crate::app::build_domain;
use crate::channels::{CommandSender, EventForwarder, UiCommand, UiEvent};

#[derive(Deserialize, Debug)]
struct Scenario {
    name: String,
    /// true (по умолчанию) — отдельный каталог `<сценарий>.data/`;
    /// false — реальный пользовательский каталог (live-сценарии).
    #[serde(default = "default_true")]
    isolated: bool,
    /// Создать профиль, если его нет (для изолированного каталога — обязательно).
    #[serde(default)]
    ensure_profile: Option<EnsureProfile>,
    #[serde(default)]
    steps: Vec<Step>,
    #[serde(default)]
    expect: Vec<Expectation>,
    /// Куда писать отчёт (по умолчанию — рядом со сценарием).
    #[serde(default)]
    report_path: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize, Debug)]
struct EnsureProfile {
    provider: String,
    name: String,
}

#[derive(Deserialize, Debug, Default)]
struct Step {
    /// Команда: load_providers | load_profiles | load_reports | select_shop |
    /// download | check_profile | load_journal | clear_journal | list_archive |
    /// list_schedules | run_schedule_now.
    cmd: String,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    report: Option<String>,
    #[serde(default)]
    period: Option<String>,
    #[serde(default)]
    date_from: Option<String>,
    #[serde(default)]
    date_to: Option<String>,
    #[serde(default)]
    name: Option<String>,
    /// Ждать после команды событие этого типа (см. event_kind).
    #[serde(default)]
    wait_event: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    /// Тихое окно после команды, если wait_event не задан (мс; default 500).
    #[serde(default)]
    settle_ms: Option<u64>,
}

#[derive(Deserialize, Debug, Default)]
struct Expectation {
    /// Тип события (например "Log", "DownloadFinished").
    #[serde(default)]
    event: Option<String>,
    /// Подстрока в сводке события (или в journal_contains — в записи журнала).
    #[serde(default)]
    contains: Option<String>,
    /// Минимальное число подходящих событий/записей (default 1).
    #[serde(default)]
    min_count: Option<usize>,
    /// Проверять хвост журнала в БД (персист!), а не поток событий.
    #[serde(default)]
    journal_contains: Option<String>,
}

#[derive(serde::Serialize)]
struct EventRecord {
    t_ms: u128,
    kind: String,
    summary: String,
}

/// Точка входа: читает сценарий, прогоняет, пишет отчёт.
/// Возврат: true = PASS. Ошибки инфраструктуры — Err (не «FAIL ожиданий»).
pub(crate) fn run(scenario_path: &Path) -> Result<bool> {
    let raw = std::fs::read_to_string(scenario_path)
        .with_context(|| format!("чтение сценария {}", scenario_path.display()))?;
    let scenario: Scenario =
        serde_json::from_str(&raw).with_context(|| "парсинг сценария JSON")?;

    // Изолированный каталог: <сценарий>.data/{mdwf.db, downloads/}.
    let prov = if scenario.isolated {
        let base: PathBuf = scenario_path.with_extension("data");
        let mut p = ProvisionedConfig::load_standard()?;
        std::fs::create_dir_all(&base).ok();
        p.data_dir = base.clone();
        p.db_path = base.join("mdwf.db");
        p.output_dir = base.join("downloads");
        p
    } else {
        ProvisionedConfig::load_standard()?
    };

    let domain = build_domain(prov, scenario.isolated)?;

    // Профиль для сценария (изолированный каталог пуст — создаём).
    if let Some(EnsureProfile { provider, name }) = &scenario.ensure_profile {
        let cat = domain.catalog.read();
        let cat = cat.as_ref().context("каталог недоступен")?;
        if cat.get_profile_by_name(name)?.is_none() {
            let p = Profile::new(name.clone(), provider.clone());
            cat.upsert_profile(&p).context("создание тестового профиля")?;
        }
    }

    // Каналы + цикл команд — как в App::new, но без gtk.
    let (cs, cmd_rx) = CommandSender::channel();
    let (event_tx, event_rx) = async_channel::bounded::<UiEvent>(1024);
    let fwd = EventForwarder::new(event_tx);
    let loop_domain = domain.clone();
    let handle = {
        let guard = domain.runtime.read();
        guard.as_ref().context("runtime")?.handle().clone()
    };
    handle.spawn(async move {
        crate::app::run_command_loop(cmd_rx, loop_domain, fwd).await;
    });

    let scenario_name = scenario.name.clone();
    let expectations = scenario.expect;
    let steps = scenario.steps;
    let report_path: PathBuf = scenario
        .report_path
        .map_or_else(
            || scenario_path.with_extension("report.json"),
            PathBuf::from,
        );

    let outcome = handle.block_on(async move {
        let mut events: Vec<EventRecord> = Vec::new();
        let t0 = Instant::now();
        let mut step_log: Vec<serde_json::Value> = Vec::new();

        for (i, step) in steps.iter().enumerate() {
            let cmd = match build_command(step) {
                Ok(c) => c,
                Err(e) => {
                    step_log.push(json!({"i": i, "cmd": step.cmd, "error": e.to_string()}));
                    bail!("шаг {i} («{}»): {e}", step.cmd);
                }
            };
            cs.send(cmd);
            let waited = match &step.wait_event {
                Some(target) => {
                    let budget = Duration::from_millis(step.timeout_ms.unwrap_or(
                        // Скачивания/расписания ходят в сеть — им нужен запас.
                        if step.cmd == "download" || step.cmd == "run_schedule_now" {
                            180_000
                        } else {
                            20_000
                        },
                    ));
                    let deadline = Instant::now() + budget;
                    let mut got = false;
                    loop {
                        let now = Instant::now();
                        if now >= deadline {
                            break;
                        }
                        match tokio::time::timeout_at(
                            tokio::time::Instant::from_std(deadline),
                            event_rx.recv(),
                        )
                        .await
                        {
                            Ok(Ok(ev)) => {
                                let kind = event_kind(&ev);
                                let summary = event_summary(&ev);
                                events.push(EventRecord {
                                    t_ms: t0.elapsed().as_millis(),
                                    kind: kind.to_string(),
                                    summary,
                                });
                                if kind == target {
                                    got = true;
                                    break;
                                }
                            }
                            Ok(Err(_)) => bail!("канал событий закрыт"),
                            Err(_) => break, // дедлайн
                        }
                    }
                    if !got {
                        step_log.push(json!({
                            "i": i, "cmd": step.cmd,
                            "timeout_waiting": target,
                        }));
                        bail!("шаг {i}: не дождались события {target}");
                    }
                    target.clone()
                }
                None => {
                    // Тихое окно: собираем всё, что приходит, пока не станет тихо.
                    let quiet = Duration::from_millis(step.settle_ms.unwrap_or(500));
                    let deadline = Instant::now() + quiet;
                    loop {
                        let now = Instant::now();
                        if now >= deadline {
                            break;
                        }
                        match tokio::time::timeout_at(
                            tokio::time::Instant::from_std(deadline),
                            event_rx.recv(),
                        )
                        .await
                        {
                            Ok(Ok(ev)) => events.push(EventRecord {
                                t_ms: t0.elapsed().as_millis(),
                                kind: event_kind(&ev).to_string(),
                                summary: event_summary(&ev),
                            }),
                            Ok(Err(_)) | Err(_) => break,
                        }
                    }
                    String::new()
                }
            };
            step_log.push(json!({"i": i, "cmd": step.cmd, "waited": waited}));
        }

        // Финальный дренаж.
        let deadline = Instant::now() + Duration::from_millis(800);
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            match tokio::time::timeout_at(
                tokio::time::Instant::from_std(deadline),
                event_rx.recv(),
            )
            .await
            {
                Ok(Ok(ev)) => events.push(EventRecord {
                    t_ms: t0.elapsed().as_millis(),
                    kind: event_kind(&ev).to_string(),
                    summary: event_summary(&ev),
                }),
                Ok(Err(_)) | Err(_) => break,
            }
        }

        // Хвост журнала из БД (проверка персиста).
        let journal_tail: Vec<serde_json::Value> = domain
            .catalog
            .read()
            .as_ref()
            .and_then(|c| c.list_journal(50).ok())
            .map_or_else(Vec::new, |rows| {
                rows.into_iter()
                    .map(|r| {
                        json!({
                            "created_at": r.created_at,
                            "kind": r.kind,
                            "origin": r.origin,
                            "message": r.message,
                        })
                    })
                    .collect()
            });

        // Ожидания.
        let mut checks: Vec<serde_json::Value> = Vec::new();
        let mut all_pass = true;
        for (i, e) in expectations.iter().enumerate() {
            let pass = expectation_pass(e, &events, &journal_tail);
            checks.push(json!({
                "i": i,
                "event": e.event,
                "contains": e.contains.clone().or_else(|| e.journal_contains.clone()),
                "min_count": e.min_count,
                "pass": pass,
            }));
            if !pass {
                all_pass = false;
            }
        }
        Ok((all_pass, checks, journal_tail, events, step_log))
    });

    match outcome {
        Ok((all_pass, checks, journal_tail, events, step_log)) => {
            let n_events = events.len();
            let report = json!({
                "scenario": scenario_name,
                "result": if all_pass { "PASS" } else { "FAIL" },
                "steps": step_log,
                "events": events,
                "journal_tail": journal_tail,
                "expectations": checks,
            });
            std::fs::write(&report_path, serde_json::to_vec_pretty(&report)?)?;
            eprintln!(
                "[self-test] {} → {} (событий: {}, отчёт: {})",
                scenario_name,
                if all_pass { "PASS" } else { "FAIL" },
                n_events,
                report_path.display(),
            );
            Ok(all_pass)
        }
        Err(e) => {
            let report = json!({
                "scenario": scenario_name,
                "result": "ERROR",
                "error": e.to_string(),
            });
            std::fs::write(&report_path, serde_json::to_vec_pretty(&report)?)?;
            eprintln!("[self-test] {scenario_name} → ERROR: {e}");
            Ok(false)
        }
    }
}

/// Собирает UiCommand из шага сценария.
fn build_command(step: &Step) -> Result<UiCommand> {
    let need = |field: &str, v: &Option<String>| {
        v.clone()
            .with_context(|| format!("шаг «{}»: нужно поле «{field}»", step.cmd))
    };
    match step.cmd.as_str() {
        "load_providers" => Ok(UiCommand::LoadProviders),
        "load_profiles" => Ok(UiCommand::LoadProfiles),
        "load_reports" => Ok(UiCommand::LoadReports(need("provider", &step.provider)?)),
        "select_shop" => Ok(UiCommand::SelectShop {
            provider_id: need("provider", &step.provider)?,
            profile_name: need("profile", &step.profile)?,
        }),
        "check_profile" => Ok(UiCommand::CheckProfile(need("profile", &step.profile)?)),
        "download" => {
            let mut params = ReportParams::new();
            params.period = step.period.clone();
            if let Some(df) = &step.date_from {
                params = params.with("date_from", df);
            }
            if let Some(dt) = &step.date_to {
                params = params.with("date_to", dt);
            }
            Ok(UiCommand::Download {
                provider_id: need("provider", &step.provider)?,
                profile_name: need("profile", &step.profile)?,
                report_type: need("report", &step.report)?,
                documents: Vec::new(),
                params,
                cancel: CancellationToken::new(),
            })
        }
        "load_journal" => Ok(UiCommand::LoadJournal),
        "clear_journal" => Ok(UiCommand::ClearJournal),
        "list_archive" => Ok(UiCommand::ListArchive {
            profile_name: None,
            report_type: None,
            date_range: None,
        }),
        "list_schedules" => Ok(UiCommand::ListSchedules),
        "run_schedule_now" => Ok(UiCommand::RunScheduleNow {
            name: need("name", &step.name)?,
        }),
        other => bail!("неизвестная команда «{other}»"),
    }
}

/// Короткое имя типа события (для wait_event/expect).
fn event_kind(ev: &UiEvent) -> &'static str {
    match ev {
        UiEvent::ProvidersLoaded(_) => "ProvidersLoaded",
        UiEvent::ProfilesLoaded(_) => "ProfilesLoaded",
        UiEvent::AuthFieldsLoaded { .. } => "AuthFieldsLoaded",
        UiEvent::ProfileSaved(_) => "ProfileSaved",
        UiEvent::ProfileDeleted(_) => "ProfileDeleted",
        UiEvent::ProfileChecked(_) => "ProfileChecked",
        UiEvent::ActiveShopChanged { .. } => "ActiveShopChanged",
        UiEvent::ActiveShopLoaded(_) => "ActiveShopLoaded",
        UiEvent::ReportsLoaded(_) => "ReportsLoaded",
        UiEvent::DocumentsListed(_) => "DocumentsListed",
        UiEvent::DocumentCategoriesLoaded(_) => "DocumentCategoriesLoaded",
        UiEvent::DownloadFinished(_) => "DownloadFinished",
        UiEvent::Progress { .. } => "Progress",
        UiEvent::Notify(_) => "Notify",
        UiEvent::DownloadStateLoaded(_) => "DownloadStateLoaded",
        UiEvent::DownloadsListed { .. } => "DownloadsListed",
        UiEvent::ArchiveListed(_) => "ArchiveListed",
        UiEvent::ArchiveReportTypesLoaded(_) => "ArchiveReportTypesLoaded",
        UiEvent::ArchiveStateLoaded(_) => "ArchiveStateLoaded",
        UiEvent::DownloadDeleted(_) => "DownloadDeleted",
        UiEvent::Log(_) => "Log",
        UiEvent::JournalLoaded(_) => "JournalLoaded",
        UiEvent::JournalCleared => "JournalCleared",
        UiEvent::SchedulesListed(_) => "SchedulesListed",
        UiEvent::AutostartChanged(_) => "AutostartChanged",
        UiEvent::WinSchedulerChanged(_) => "WinSchedulerChanged",
    }
}

/// Однострочная сводка события (без тяжёлого контента файлов).
fn event_summary(ev: &UiEvent) -> String {
    match ev {
        UiEvent::ProvidersLoaded(v) => {
            format!("{} провайдеров: {}", v.len(), v.iter().map(|p| p.id.clone()).collect::<Vec<_>>().join(","))
        }
        UiEvent::ProfilesLoaded(v) => {
            format!("{} профилей: {}", v.len(), v.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(","))
        }
        UiEvent::AuthFieldsLoaded { provider_id, fields } => {
            format!("{provider_id}: {} полей", fields.len())
        }
        UiEvent::ProfileSaved(r) => format!("saved: {r:?}"),
        UiEvent::ProfileDeleted(r) => format!("deleted: {r:?}"),
        UiEvent::ProfileChecked(r) => match r {
            Ok(h) => format!("ok: {:?}", h.level),
            Err(e) => format!("err: {e}"),
        },
        UiEvent::ActiveShopChanged {
            provider_id,
            profile_name,
            seller_name,
            ..
        } => format!("{provider_id}/{profile_name} seller={seller_name:?}"),
        UiEvent::ActiveShopLoaded(a) => format!("{a:?}"),
        UiEvent::ReportsLoaded(r) => match r {
            Ok(v) => format!("ok: {} отчётов", v.len()),
            Err(e) => format!("err: {e}"),
        },
        UiEvent::DocumentsListed(r) => match r {
            Ok(v) => format!("ok: {} документов", v.len()),
            Err(e) => format!("err: {e}"),
        },
        UiEvent::DocumentCategoriesLoaded(r) => match r {
            Ok(v) => format!("ok: {} категорий", v.len()),
            Err(e) => format!("err: {e}"),
        },
        UiEvent::DownloadFinished(r) => match r {
            Ok(d) => format!(
                "ok: {} файл(ов): {}",
                d.files.len(),
                d.saved_paths.join("; ")
            ),
            Err(e) => format!("err: {e}"),
        },
        UiEvent::Progress { fraction, message } => {
            format!("{fraction:?}: {message}")
        }
        UiEvent::Notify(s) => s.clone(),
        UiEvent::DownloadStateLoaded(s) => format!("{s:?}"),
        UiEvent::DownloadsListed { report_type, docs } => {
            format!("{report_type}: {} док.", docs.len())
        }
        UiEvent::ArchiveListed(r) => match r {
            Ok(v) => format!("ok: {} записей", v.len()),
            Err(e) => format!("err: {e}"),
        },
        UiEvent::ArchiveReportTypesLoaded(v) => format!("{} типов", v.len()),
        UiEvent::ArchiveStateLoaded(s) => format!("{s:?}"),
        UiEvent::DownloadDeleted(r) => format!("{r:?}"),
        UiEvent::Log(e) => format!("[{}] {}", e.origin, e.message),
        UiEvent::JournalLoaded(v) => format!("{} записей из БД", v.len()),
        UiEvent::JournalCleared => "журнал очищен".into(),
        UiEvent::SchedulesListed(r) => match r {
            Ok(v) => format!("ok: {} расписаний", v.len()),
            Err(e) => format!("err: {e}"),
        },
        UiEvent::AutostartChanged(r) | UiEvent::WinSchedulerChanged(r) => {
            format!("{r:?}")
        }
    }
}

/// Проверка одного ожидания по собранным событиям/журналу.
fn expectation_pass(
    e: &Expectation,
    events: &[EventRecord],
    journal_tail: &[serde_json::Value],
) -> bool {
    let min = e.min_count.unwrap_or(1);
    if let Some(jneedle) = &e.journal_contains {
        let n = journal_tail
            .iter()
            .filter(|row| {
                row["message"]
                    .as_str()
                    .is_some_and(|m| m.contains(jneedle.as_str()))
            })
            .count();
        return n >= min;
    }
    let (Some(kind), needle) = (&e.event, &e.contains) else {
        return false;
    };
    let n = events
        .iter()
        .filter(|ev| {
            &ev.kind == kind
                && needle.as_ref().map_or(true, |n| ev.summary.contains(n.as_str()))
        })
        .count();
    n >= min
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expectation_matching() {
        let events = vec![
            EventRecord {
                t_ms: 1,
                kind: "Log".into(),
                summary: "[CLI] «Баланс» (июль 2026): скачано 1 файл(ов)".into(),
            },
            EventRecord {
                t_ms: 2,
                kind: "Log".into(),
                summary: "[CLI] Скачивание «Баланс» — oz_prof1".into(),
            },
        ];
        let journal = vec![json!({"message": "Расписание «x»: 1 файл(ов)"})];

        assert!(expectation_pass(
            &Expectation {
                event: Some("Log".into()),
                contains: Some("скачано 1 файл(ов)".into()),
                min_count: None,
                journal_contains: None,
            },
            &events,
            &journal
        ));
        // min_count не достигнут.
        assert!(!expectation_pass(
            &Expectation {
                event: Some("Log".into()),
                contains: Some("скачано".into()),
                min_count: Some(3),
                journal_contains: None,
            },
            &events,
            &journal
        ));
        // Журнал (персист).
        assert!(expectation_pass(
            &Expectation {
                event: None,
                contains: None,
                min_count: None,
                journal_contains: Some("Расписание «x»".into()),
            },
            &events,
            &journal
        ));
        // Несовпадение по kind.
        assert!(!expectation_pass(
            &Expectation {
                event: Some("DownloadFinished".into()),
                contains: None,
                min_count: None,
                journal_contains: None,
            },
            &events,
            &journal
        ));
    }

    #[test]
    fn step_command_build() {
        let mut s = Step {
            cmd: "download".into(),
            provider: Some("test".into()),
            profile: Some("drv".into()),
            report: Some("test.realization".into()),
            period: Some("2026-07".into()),
            ..Default::default()
        };
        assert!(build_command(&s).is_ok());
        s.report = None;
        assert!(build_command(&s).is_err());
        let bad = Step {
            cmd: "no_such".into(),
            ..Default::default()
        };
        assert!(build_command(&bad).is_err());
    }
}
