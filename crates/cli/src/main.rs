//! # mdwf (CLI)
//!
//! Подкоманды (спец. §2.6.1) и exit-коды (спец. §2.6.2).
//! Использует тот же доменный слой, что и GUI (registry/catalog/secrets/config).

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::unused_async)]
#![allow(clippy::unused_self)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::used_underscore_binding)]
#![allow(clippy::print_literal)]
#![allow(clippy::items_after_statements)]
#![allow(dead_code)]

use std::process::ExitCode as StdExitCode;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use mdwf_cli::exit_code::ExitCode;
use mdwf_cli::{Cli, Command};

#[tokio::main]
async fn main() -> StdExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            e.print().ok();
            return StdExitCode::from(ExitCode::UsageError);
        }
    };

    let filter =
        EnvFilter::try_new(&cli.log_level).unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    tracing::debug!(?cli.command, "mdwf CLI starting");

    match run(cli).await {
        Ok(code) => StdExitCode::from(code),
        Err(e) => {
            eprintln!("Ошибка: {e:#}");
            StdExitCode::from(ExitCode::GenericError)
        }
    }
}

async fn run(cli: Cli) -> Result<ExitCode> {
    use mdwf_cli::commands::{
        archive, doctor, download, out_of_scope, profiles, providers, reports, schedule, Context,
    };
    // Контекст создаётся один раз и переиспользуется всеми командами.
    let ctx = match Context::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Не удалось инициализировать контекст: {e:#}");
            return Ok(ExitCode::ConfigError);
        }
    };
    match cli.command {
        Command::Providers { action } => providers::run(&ctx, action).await,
        Command::Profiles { action } => profiles::run(&ctx, action).await,
        Command::Reports { action } => reports::run(&ctx, action).await,
        Command::Download(args) => download::run(&ctx, args).await,
        Command::Archive { action } => archive::run(&ctx, action).await,
        Command::OutOfScope { provider } => out_of_scope::run(&ctx, provider).await,
        Command::Schedule { action } => schedule::run(&ctx, action).await,
        Command::Doctor => doctor::run(&ctx).await,
    }
}
