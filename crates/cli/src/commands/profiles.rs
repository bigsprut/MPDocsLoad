//! Команда `profiles`.

use anyhow::Result;

use mdwf_core::Profile;

use crate::commands::Context;
use crate::exit_code::ExitCode;
use crate::output::print_profiles;
use crate::ProfilesCmd;

pub async fn run(ctx: &Context, action: ProfilesCmd) -> Result<ExitCode> {
    match action {
        ProfilesCmd::List => {
            let list = ctx.catalog.list_profiles()?;
            print_profiles(&list);
            Ok(ExitCode::Success)
        }
        ProfilesCmd::Add {
            provider,
            name,
            client_id,
            api_key,
            token,
        } => {
            let mut profile = Profile::new(&name, &provider);
            if let Some(cid) = client_id {
                profile = profile.with_metadata("client_id", cid);
            }
            if let Some(key) = api_key {
                profile = profile.with_metadata("api_key", key);
            }
            if let Some(t) = token {
                profile = profile.with_metadata("token", t);
            }
            let id = ctx.save_profile(&profile)?;
            println!("Профиль '{}' сохранён (id={id}, провайдер={provider}).", profile.name);
            Ok(ExitCode::Success)
        }
        ProfilesCmd::Delete { name, yes } => {
            if !yes {
                print!("Удалить профиль '{name}'? [y/N] ");
                use std::io::Write;
                std::io::stdout().flush().ok();
                let mut input = String::new();
                std::io::stdin().read_line(&mut input).ok();
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Отменено.");
                    return Ok(ExitCode::Success);
                }
            }
            ctx.catalog.delete_profile(&name)?;
            println!("Профиль '{name}' удалён.");
            Ok(ExitCode::Success)
        }
        ProfilesCmd::Check { name } => {
            let profile = ctx
                .catalog
                .get_profile_by_name(&name)?
                .ok_or_else(|| anyhow::anyhow!("профиль '{name}' не найден"))?;
            let provider = ctx.registry.require(&profile.provider_id)?;
            let auth = provider.authenticator(&profile).await?;
            match provider.health_check(auth.as_ref()).await {
                Ok(status) => {
                    let level = match status.level {
                        mdwf_core::HealthLevel::Ok => "OK",
                        mdwf_core::HealthLevel::Degraded => "DEGRADED",
                        mdwf_core::HealthLevel::Down => "DOWN",
                    };
                    println!("Health: {level}");
                    if !status.message.is_empty() {
                        println!("  {}", status.message);
                    }
                    Ok(ExitCode::Success)
                }
                Err(e) => {
                    println!("Health: DOWN ({e})");
                    Ok(ExitCode::AuthError)
                }
            }
        }
    }
}
