//! Автозапуск с Windows (спец. §2.7.1 [scheduler.autostart_with_os]).
//!
//! Реализация через ключ реестра `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
//! Добавляет/удаляет запись `MDWF`, указывающую на текущий исполняемый файл GUI.

#![cfg(windows)]

use std::process::Command;

use mdwf_core::{CoreError, CoreResult};

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const APP_NAME: &str = "MDWF";

/// Включает автозапуск: добавляет MDWF в ключ Run реестра.
pub fn enable_autostart() -> CoreResult<()> {
    let exe = current_exe()?;
    let value = format!("\"{}\"", exe.display());
    reg_add(APP_NAME, &value)
}

/// Отключает автозапуск: удаляет MDWF из ключа Run.
pub fn disable_autostart() -> CoreResult<()> {
    reg_delete(APP_NAME)
}

/// Проверяет, включён ли автозапуск.
pub fn is_autostart_enabled() -> bool {
    reg_query(APP_NAME).is_ok()
}

fn current_exe() -> CoreResult<std::path::PathBuf> {
    std::env::current_exe().map_err(|e| CoreError::Internal(format!("current_exe: {e}")))
}

fn reg_add(name: &str, value: &str) -> CoreResult<()> {
    let out = Command::new("reg")
        .args(["add", RUN_KEY, "/v", name, "/t", "REG_SZ", "/d", value, "/f"])
        .output()
        .map_err(|e| CoreError::Internal(format!("reg add: {e}")))?;
    if !out.status.success() {
        return Err(CoreError::Internal(format!(
            "reg add failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(())
}

fn reg_delete(name: &str) -> CoreResult<()> {
    let out = Command::new("reg")
        .args(["delete", RUN_KEY, "/v", name, "/f"])
        .output()
        .map_err(|e| CoreError::Internal(format!("reg delete: {e}")))?;
    // Если значения нет — reg delete возвращает ошибку, но это OK (idempotent).
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("unable to find") || stderr.contains("not find") {
            return Ok(());
        }
        return Err(CoreError::Internal(format!("reg delete failed: {stderr}")));
    }
    Ok(())
}

fn reg_query(name: &str) -> CoreResult<()> {
    let out = Command::new("reg")
        .args(["query", RUN_KEY, "/v", name])
        .output()
        .map_err(|e| CoreError::Internal(format!("reg query: {e}")))?;
    if !out.status.success() {
        return Err(CoreError::Internal("not found".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_exe_resolves() {
        assert!(current_exe().is_ok());
    }

    #[test]
    #[ignore = "требует доступа к HKCU реестра; запускается вручную: cargo test -- --ignored"]
    fn enable_disable_idempotent() {
        // Включаем и выключаем — должно пройти без ошибок.
        assert!(enable_autostart().is_ok());
        assert!(is_autostart_enabled());
        assert!(disable_autostart().is_ok());
        assert!(!is_autostart_enabled());
        // Повторное отключение (несуществующего) — тоже OK.
        assert!(disable_autostart().is_ok());
    }
}
