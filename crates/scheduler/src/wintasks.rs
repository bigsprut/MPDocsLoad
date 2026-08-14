//! Фоновый планировщик через Windows Task Scheduler (спец. гибрид с in-process Runner).
//!
//! Создаёт одну системную задачу `MDWF_Scheduler`, которая каждые `POLL_MINUTES`
//! минут запускает CLI-бинарник (`mdwf.exe schedule run`) — он проверяет общий
//! каталог и выполняет наступившие по cron расписания. В отличие от in-process
//! Runner, работает без открытого GUI и переживает логаут/ребут (пока пользователь
//! залогинен). Защита от двойного выполнения с in-process Runner — через
//! `claim_schedule` (атомарный bump `next_run_at`) в общем каталоге.

#![cfg(windows)]

use std::process::Command;

use mdwf_core::{CoreError, CoreResult};

const TASK_NAME: &str = "MDWF_Scheduler";
/// Период опроса (минуты). Расписания выполняются с задержкой до POLL_MINUTES.
const POLL_MINUTES: u32 = 5;

/// Включает фоновый планировщик: создаёт задачу `MDWF_Scheduler`, запускающую
/// CLI `schedule run` каждые POLL_MINUTES минут. /F — перезапись существующей.
pub fn enable_windows_scheduler() -> CoreResult<()> {
    let cli = cli_exe_path()?;
    // /TR-действие: "<cli>" schedule run  (exe в кавычках на случай пробелов в пути).
    let action = format!("\"{}\" schedule run --by-task", cli.display());
    let out = Command::new("schtasks")
        .args(["/Create", "/TN", TASK_NAME, "/TR"])
        .arg(&action)
        .args(["/SC", "MINUTE", "/MO", &POLL_MINUTES.to_string(), "/F"])
        .output()
        .map_err(|e| CoreError::Internal(format!("schtasks create: {e}")))?;
    if !out.status.success() {
        return Err(CoreError::Internal(format!(
            "schtasks /Create failed: {} | {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(())
}

/// Отключает фоновый планировщик: удаляет задачу. Idempotent (нет задачи — OK):
/// сначала проверяем существование через Query (надёжнее парсинга вывода schtasks,
/// который на русской Windows идёт в кодировке cp866 и не матчится UTF-8-паттернами).
pub fn disable_windows_scheduler() -> CoreResult<()> {
    if !is_windows_scheduler_enabled() {
        return Ok(()); // задачи нет — ничего делать не нужно
    }
    let out = Command::new("schtasks")
        .args(["/Delete", "/TN", TASK_NAME, "/F"])
        .output()
        .map_err(|e| CoreError::Internal(format!("schtasks delete: {e}")))?;
    if !out.status.success() {
        return Err(CoreError::Internal(format!(
            "schtasks /Delete failed: {} | {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(())
}

/// Проверяет, создана ли задача `MDWF_Scheduler`.
pub fn is_windows_scheduler_enabled() -> bool {
    Command::new("schtasks")
        .args(["/Query", "/TN", TASK_NAME])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Путь к CLI-бинарнику (`mdwf.exe`) — в той же директории, что текущий exe.
/// Текущий exe — GUI (`mdwf-gui.exe`) или сам CLI; в обоих случаях сосед `mdwf.exe`.
fn cli_exe_path() -> CoreResult<std::path::PathBuf> {
    let cur = std::env::current_exe().map_err(|e| CoreError::Internal(format!("current_exe: {e}")))?;
    let dir = cur
        .parent()
        .ok_or_else(|| CoreError::Internal("не удалось определить директорию exe".into()))?;
    Ok(dir.join("mdwf.exe"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_path_resolves() {
        assert!(cli_exe_path().is_ok());
    }

    #[test]
    #[ignore = "меняет системные задачи; запускается вручную: cargo test -- --ignored"]
    fn enable_disable_idempotent() {
        assert!(enable_windows_scheduler().is_ok());
        assert!(is_windows_scheduler_enabled());
        assert!(disable_windows_scheduler().is_ok());
        assert!(!is_windows_scheduler_enabled());
        // Повторное удаление несуществующей — OK (idempotent через Query pre-check).
        assert!(disable_windows_scheduler().is_ok());
    }
}
