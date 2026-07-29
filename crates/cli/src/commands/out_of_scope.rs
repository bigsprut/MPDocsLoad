//! Команда `out-of-scope`.

use anyhow::Result;

use crate::commands::Context;
use crate::exit_code::ExitCode;
use crate::output::print_out_of_scope;

pub async fn run(ctx: &Context, provider: Option<String>) -> Result<ExitCode> {
    let providers: Vec<String> = match &provider {
        Some(p) => vec![p.clone()],
        None => vec!["ozon".into(), "wildberries".into()],
    };

    for pid in providers {
        let docs: Vec<(&str, &str)> = match pid.as_str() {
            "ozon" => mdwf_providers_ozon::out_of_scope(),
            "wildberries" => mdwf_providers_wildberries::out_of_scope(),
            other => {
                println!("Неизвестный провайдер: {other}");
                return Ok(ExitCode::UsageError);
            }
        };
        print_out_of_scope(&pid, &docs);
        println!();
    }

    // Проверяем, что провайдер реально зарегистрирован.
    if let Some(p) = &provider {
        if ctx.registry.get(p).is_none() {
            println!("Внимание: провайдер '{p}' не зарегистрирован.");
        }
    }

    Ok(ExitCode::Success)
}
