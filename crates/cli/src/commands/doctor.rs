//! Команда `doctor` — диагностика окружения.

use anyhow::Result;

use crate::commands::Context;
use crate::exit_code::ExitCode;

pub async fn run(ctx: &Context) -> Result<ExitCode> {
    println!("MDWF диагностика (v{})\n", env!("CARGO_PKG_VERSION"));

    // Конфиг.
    println!("Конфигурация:");
    println!("  Папка данных:  {}", ctx.config.data_dir.display());
    println!("  config.toml:   {}", ctx.config.config_path.display());
    println!("  SQLite:        {}", ctx.config.db_path.display());
    println!("  Папка выгрузки:{}", ctx.config.output_dir.display());

    // Провайдеры.
    let providers = ctx.registry.list();
    println!("\nПровайдеры ({}):", providers.len());
    for p in &providers {
        println!("  {} {} — {} отчётов", p.id(), p.display_name(), p.capabilities().reports.len());
    }

    // Профили.
    let profiles = ctx.catalog.list_profiles()?;
    println!("\nПрофили ({}):", profiles.len());
    for p in &profiles {
        println!("  {} — {}", p.name, p.provider_id);
    }

    // Выгрузки.
    println!("\nКаталог выгрузок: OK (SQLite {} байт)",
        std::fs::metadata(&ctx.config.db_path).map_or(0, |m| m.len()));

    // Проверка подключений профилей.
    if !profiles.is_empty() {
        println!("\nПроверка подключений:");
        for p in &profiles {
            // Подмешиваем секреты из keyring перед authenticator.
            let profile = match ctx.profile_with_secrets(&p.name).await {
                Ok(prof) => prof,
                Err(e) => {
                    println!("  {} ({}) → load secrets error: {e}", p.name, p.provider_id);
                    continue;
                }
            };
            match ctx.registry.require(&profile.provider_id) {
                Ok(provider) => match provider.authenticator(&profile).await {
                    Ok(auth) => match provider.health_check(auth.as_ref()).await {
                        Ok(status) => {
                            let level = match status.level {
                                mdwf_core::HealthLevel::Ok => "OK",
                                mdwf_core::HealthLevel::Degraded => "DEGRADED",
                                mdwf_core::HealthLevel::Down => "DOWN",
                            };
                            println!("  {} ({}) → {level} {}", p.name, p.provider_id, status.message);
                        }
                        Err(e) => println!("  {} ({}) → DOWN: {e}", p.name, p.provider_id),
                    },
                    Err(e) => println!("  {} ({}) → auth error: {e}", p.name, p.provider_id),
                },
                Err(_) => println!("  {} ({}) → провайдер не зарегистрирован", p.name, p.provider_id),
            }
        }
    }

    println!("\nДиагностика завершена.");
    Ok(ExitCode::Success)
}
