//! Команда `providers`.

use anyhow::Result;

use crate::commands::Context;
use crate::exit_code::ExitCode;
use crate::output::print_providers;
use crate::ProvidersCmd;

pub async fn run(_ctx: &Context, action: ProvidersCmd) -> Result<ExitCode> {
    match action {
        ProvidersCmd::List => {
            let list = _ctx.registry.list();
            print_providers(&list);
            Ok(ExitCode::Success)
        }
        ProvidersCmd::Info { provider_id } => {
            let p = _ctx.registry.require(&provider_id)?;
            let caps = p.capabilities();
            println!("ID:          {}", p.id());
            println!("Имя:         {}", p.display_name());
            println!("Версия:      {}", p.version());
            println!("Документация:{}", p.docs_url());
            println!("Авторизация: {:?}", caps.auth_type);
            println!("Отчётов:     {}", caps.reports.len());
            println!("\nПоля авторизации:");
            for f in &caps.auth_fields {
                let mark = if f.required { "*" } else { " " };
                println!("  {mark} {} ({:?})", f.label, f.kind);
            }
            Ok(ExitCode::Success)
        }
    }
}
