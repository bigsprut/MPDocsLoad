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
#![allow(clippy::unused_async)]
#![allow(clippy::unused_self)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::used_underscore_binding)]
#![allow(clippy::print_literal)]
#![allow(clippy::items_after_statements)]
#![allow(dead_code)]

mod commands;
mod exit_code;
mod output;

use std::process::ExitCode as StdExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use crate::exit_code::ExitCode;

/// Marketplace Downloader Framework — CLI.
#[derive(Parser, Debug)]
#[command(
    name = "mdwf",
    version,
    about = "Выгрузка финансовых документов с маркетплейсов через официальные API",
    long_about = "Marketplace Downloader Framework (MDWF). Поддержка Ozon и Wildberries."
)]
struct Cli {
    /// Уровень логирования (trace, debug, info, warn, error).
    #[arg(long, global = true, default_value = "warn")]
    log_level: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Управление провайдерами.
    Providers {
        #[command(subcommand)]
        action: ProvidersCmd,
    },
    /// Управление профилями учётных данных.
    Profiles {
        #[command(subcommand)]
        action: ProfilesCmd,
    },
    /// Список отчётов провайдера.
    Reports {
        #[command(subcommand)]
        action: ReportsCmd,
    },
    /// Выгрузка отчётов.
    Download(DownloadArgs),
    /// Документы, недоступные через API (out-of-scope).
    OutOfScope {
        /// Фильтр по провайдеру (ozon/wildberries).
        #[arg(long)]
        provider: Option<String>,
    },
    /// Диагностика окружения и подключений.
    Doctor,
}

#[derive(Subcommand, Debug)]
enum ProvidersCmd {
    /// Список зарегистрированных провайдеров.
    List,
    /// Информация о провайдере.
    Info { provider_id: String },
}

#[derive(Subcommand, Debug)]
enum ProfilesCmd {
    /// Список профилей.
    List,
    /// Добавить профиль.
    Add {
        #[arg(long)]
        provider: String,
        #[arg(long)]
        name: String,
        /// Client-Id (для Ozon) или токен (для WB).
        #[arg(long)]
        client_id: Option<String>,
        /// Api-Key (Ozon).
        #[arg(long)]
        api_key: Option<String>,
        /// Токен (WB).
        #[arg(long)]
        token: Option<String>,
    },
    /// Удалить профиль.
    Delete {
        name: String,
        /// Не спрашивать подтверждение.
        #[arg(long)]
        yes: bool,
    },
    /// Проверить подключение профиля.
    Check { name: String },
}

#[derive(Subcommand, Debug)]
enum ReportsCmd {
    /// Список отчётов провайдера.
    List {
        #[arg(long)]
        provider: String,
    },
    /// Информация об отчёте.
    Info {
        provider_id: String,
        report_type: String,
    },
}

#[derive(Parser, Debug)]
struct DownloadArgs {
    /// Имя профиля.
    #[arg(long)]
    profile: String,
    /// Тип отчёта (можно несколько: --report ozon.realization --report ...).
    #[arg(long, required = true)]
    report: Vec<String>,
    /// Период (YYYY-MM для месячных, YYYY-MM-DD для дневных).
    #[arg(long)]
    period: Option<String>,
    /// Папка выгрузки (переопределяет config).
    #[arg(long)]
    output_dir: Option<String>,
    /// Для Browsable: id документов через запятую.
    #[arg(long)]
    ids: Option<String>,
    /// Категория документа (для WB documents).
    #[arg(long)]
    category: Option<String>,
}

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
    use commands::{doctor, download, out_of_scope, profiles, providers, reports, Context};
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
        Command::OutOfScope { provider } => out_of_scope::run(&ctx, provider).await,
        Command::Doctor => doctor::run(&ctx).await,
    }
}
