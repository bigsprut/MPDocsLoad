//! # mdwf-cli (библиотека)
//!
//! Логика CLI, переиспользуемая GUI-бинарником для скрытого фонового
//! запуска расписаний (`mdwf-gui.exe --schedule-run`): GUI-процесс имеет
//! windows-subsystem и не мигает консолью при срабатывании задачи
//! планировщика (в отличие от консольного `mdwf.exe schedule run`).

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

pub mod commands;
pub mod exit_code;
pub mod output;

pub mod cli_args;

pub use cli_args::{
    ArchiveCmd, Cli, Command, DownloadArgs, ProfilesCmd, ProvidersCmd, ReportsCmd,
    ScheduleCmd,
};
