//! Представления (views) приложения.

pub mod about;
pub mod archive;
pub mod download;
pub mod logs;
pub mod main_window;
pub mod reports;
pub mod scheduler;
pub mod settings;
pub mod shop;

/// Открывает файл ассоциированным приложением (напр. Excel — для .xlsx).
/// Если файл не существует — возвращает ошибку (UI предложит «Перекачать»).
/// Общий хелпер для вкладок «Загрузка» и «Архив» (П.6).
pub(crate) fn open_file(path: &str) -> std::io::Result<()> {
    if !std::path::Path::new(path).exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "файл не найден (возможно, удалён/перемещён) — перекачайте",
        ));
    }
    #[cfg(target_os = "windows")]
    {
        // cmd /c start "" "<path>" — открывает ассоциированным приложением.
        std::process::Command::new("cmd")
            .args(["/C", "start", "", path])
            .spawn()?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("xdg-open").arg(path).spawn()?;
    }
    Ok(())
}

/// Открывает папку в проводнике (Windows). Общий хелпер (П.6).
///
/// Использует `cmd /c start "" <path>` (как open_file) — надёжнее прямого
/// `explorer <path>`, который при уже запущенном проводнике иногда открывает
/// 2 окна (handoff в работающий экземпляр).
pub(crate) fn open_folder(path: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", path])
            .spawn()?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
    }
    Ok(())
}
