//! Команда `reports`.

use anyhow::Result;

use crate::commands::Context;
use crate::exit_code::ExitCode;
use crate::output::print_reports;
use crate::ReportsCmd;

pub async fn run(ctx: &Context, action: ReportsCmd) -> Result<ExitCode> {
    match action {
        ReportsCmd::List { provider } => {
            let p = ctx.registry.require(&provider)?;
            print_reports(&provider, &p.capabilities().reports);
            Ok(ExitCode::Success)
        }
        ReportsCmd::Info {
            provider_id,
            report_type,
        } => {
            let p = ctx.registry.require(&provider_id)?;
            let desc = p
                .capabilities()
                .reports
                .iter()
                .find(|r| r.type_id == report_type)
                .ok_or_else(|| {
                    anyhow::anyhow!("отчёт '{report_type}' не найден у провайдера '{provider_id}'")
                })?;
            println!("Тип:        {}", desc.type_id);
            println!("Название:   {}", desc.display_name);
            println!("Категория:  {:?}", desc.category);
            println!(
                "Режим:      {}",
                if desc.acquisition_mode.is_browsable() {
                    "Browsable (список→выбор→скачать)"
                } else {
                    "Period (тип+период→генерация)"
                }
            );
            if !desc.parameters.is_empty() {
                println!("\nПараметры:");
                for param in &desc.parameters {
                    let req = if param.required { "*" } else { " " };
                    println!("  {req} {} ({:?})", param.label, param.kind);
                }
            }
            Ok(ExitCode::Success)
        }
    }
}
