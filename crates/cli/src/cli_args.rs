//! Определения аргументов командной строки (clap). Живут в библиотеке,
//! чтобы модули команд (lib) и бинарник `mdwf` делили одни типы.

use clap::{Parser, Subcommand};

/// Marketplace Downloader Framework — CLI.
#[derive(Parser, Debug)]
#[command(
    name = "mdwf",
    version,
    about = "Выгрузка финансовых документов с маркетплейсов через официальные API",
    long_about = "Marketplace Downloader Framework (MDWF). Поддержка Ozon и Wildberries."
)]
pub struct Cli {
    /// Уровень логирования (trace, debug, info, warn, error).
    #[arg(long, global = true, default_value = "warn")]
    pub log_level: String,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
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
    /// Архив скачанных файлов (офлайн-просмотр из локального каталога).
    Archive {
        #[command(subcommand)]
        action: ArchiveCmd,
    },
    /// Документы, недоступные через API (out-of-scope).
    OutOfScope {
        /// Фильтр по провайдеру (ozon/wildberries).
        #[arg(long)]
        provider: Option<String>,
    },
    /// Управление расписаниями планировщика.
    Schedule {
        #[command(subcommand)]
        action: ScheduleCmd,
    },
    /// Диагностика окружения и подключений.
    Doctor,
}

#[derive(Subcommand, Debug)]
pub enum ScheduleCmd {
    /// Список расписаний.
    List,
    /// Добавить расписание.
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        profile: String,
        #[arg(long, required = true)]
        report: Vec<String>,
        #[arg(long, default_value = "0 2 1 * *")]
        cron: String,
        /// Сдвиг периода в месяцах (0 = текущий, -1 = прошлый).
        #[arg(long, default_value_t = 0)]
        period_offset: i32,
        #[arg(long, default_value_t = false)]
        disabled: bool,
    },
    /// Удалить расписание.
    Delete { name: String },
    /// Запустить все просроченные расписания.
    Run {
        /// Запуск фоновой задачей Windows Task Scheduler (ставится самой
        /// задачей; помечает записи журнала источником «задача Windows»).
        #[arg(long)]
        by_task: bool,
    },
    /// Управление автозапуском с Windows.
    Autostart {
        #[arg(long)]
        enable: bool,
        #[arg(long)]
        disable: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum ProvidersCmd {
    /// Список зарегистрированных провайдеров.
    List,
    /// Информация о провайдере.
    Info { provider_id: String },
}

#[derive(Subcommand, Debug)]
pub enum ProfilesCmd {
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
pub enum ReportsCmd {
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

#[derive(Subcommand, Debug)]
pub enum ArchiveCmd {
    /// Список скачанных файлов с фильтрами (офлайн, без обращения к API).
    List {
        /// Фильтр по имени профиля (без --profile = все профили).
        #[arg(long)]
        profile: Option<String>,
        /// Фильтр по типу отчёта (без --report = все отчёты).
        #[arg(long)]
        report: Option<String>,
        /// Фильтр по периоду (YYYY-MM; без --period = все периоды).
        #[arg(long)]
        period: Option<String>,
    },
}

#[derive(Parser, Debug)]
pub struct DownloadArgs {
    /// Имя профиля.
    #[arg(long)]
    pub profile: String,
    /// Тип отчёта (можно несколько: --report ozon.realization --report ...).
    #[arg(long, required = true)]
    pub report: Vec<String>,
    /// Период (YYYY-MM для месячных, YYYY-MM-DD для дневных).
    #[arg(long)]
    pub period: Option<String>,
    /// Папка выгрузки (переопределяет config).
    #[arg(long)]
    pub output_dir: Option<String>,
    /// Для Browsable: id документов через запятую.
    #[arg(long)]
    pub ids: Option<String>,
    /// Категория документа (для WB documents).
    #[arg(long)]
    pub category: Option<String>,
    /// Номера отправлений через запятую (для ozon.accrual_postings, 1–200).
    #[arg(long)]
    pub posting_numbers: Option<String>,
    /// Идентификаторы складов через запятую (для ozon.warehouse_stock, ≤50).
    #[arg(long)]
    pub warehouse_ids: Option<String>,
    /// SKU товаров через запятую (для ozon.analytics_stocks, ≤100).
    #[arg(long)]
    pub skus: Option<String>,
}
