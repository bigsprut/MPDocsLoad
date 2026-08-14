//! # mdwf-scheduler
//!
//! Cron-планировщик + очередь задач (спец. §2.8, гл. 08).
//! Персистентность — в SQLite (`schedules`), автозапуск в Windows.

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::future_not_send)]
#![allow(clippy::manual_strip)]
#![allow(clippy::unused_async)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::similar_names)]
#![allow(clippy::match_same_arms)]

pub mod autostart;
pub mod cron;
pub mod runner;
pub mod wintasks;

pub use autostart::{disable_autostart, enable_autostart, is_autostart_enabled};
pub use cron::{fmt_local, next_run, parse, period_for_offset, DAILY, MONTHLY, QUARTERLY, WEEKLY};
pub use runner::{run_due_schedules, JobExecutor, JobRequest, JobResult, RunStatus, Runner};
pub use wintasks::{
    disable_windows_scheduler, enable_windows_scheduler, is_windows_scheduler_enabled,
};
