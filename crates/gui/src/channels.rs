//! Каналы между UI-потоком (GTK main loop) и tokio-задачами.
//!
//! UI отправляет `UiCommand` в tokio-сторону через `CommandSender`.
//! Tokio-задачи отправляют `UiEvent` обратно в UI через `glib::MainContext`
//! (см. `EventForwarder`).

use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use mdwf_core::{DocumentEntry, DocumentFilter, DownloadedFile, HealthStatus, Profile};

/// Идентификатор вкладки/раздела UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewId {
    /// Раздел «Магазин»: выбор маркетплейса+профиля + CRUD профилей.
    /// Заменил отдельную вкладку «Профили» — единый источник правды выбора.
    Shop,
    Reports,
    Download,
    /// Офлайн-архив скачанных документов (П.6): навигация по downloads с фильтрами.
    Archive,
    Settings,
    Scheduler,
    Logs,
    /// Справка/помощь для пользователя (инструкции по работе с программой).
    Help,
    About,
}

impl ViewId {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shop => "shop",
            Self::Reports => "reports",
            Self::Download => "download",
            Self::Archive => "archive",
            Self::Settings => "settings",
            Self::Scheduler => "scheduler",
            Self::Logs => "logs",
            Self::Help => "help",
            Self::About => "about",
        }
    }
}

/// Команды из UI в доменный слой (tokio).
#[derive(Debug)]
pub enum UiCommand {
    /// Загрузить список провайдеров.
    LoadProviders,
    /// Загрузить список профилей.
    LoadProfiles,
    /// Загрузить поля авторизации провайдера (для динамической формы профиля).
    LoadAuthFields(String),
    /// Сохранить профиль.
    SaveProfile(Profile),
    /// Удалить профиль.
    DeleteProfile(String),
    /// Проверить подключение профиля.
    CheckProfile(String),
    /// Выбрать активный магазин (provider + profile) — persist в ui_state
    /// и запросить имя продавца из API для заголовка окна.
    SelectShop {
        provider_id: String,
        profile_name: String,
    },
    /// Загрузить сохранённый активный магазин (при старте).
    LoadActiveShop,
    /// Загрузить список отчётов провайдера.
    LoadReports(String),
    /// Загрузить список категорий документов WB (для выпадающего списка).
    LoadDocumentCategories {
        provider_id: String,
        profile_name: String,
    },
    /// Получить список документов (Browsable-режим).
    ListDocuments {
        provider_id: String,
        profile_name: String,
        report_type: String,
        filter: DocumentFilter,
        /// Токен отмены (кнопка «Отмена» во вкладке «Загрузка»).
        cancel: CancellationToken,
    },
    /// Скачать выбранные документы (Browsable) или сгенерировать отчёт (Period).
    Download {
        provider_id: String,
        profile_name: String,
        report_type: String,
        /// Для Browsable: выбранные документы (с человекочитаемым именем и
        /// предпочтительным расширением). Для Period: пусто (params.period).
        documents: Vec<DocumentSel>,
        params: mdwf_core::ReportParams,
        /// Токен отмены (кнопка «Отмена» во вкладке «Загрузка»).
        cancel: CancellationToken,
    },
    /// Отмена текущей операции.
    Cancel,
    /// Сохранить состояние экрана «Загрузка» (автосохранение).
    SaveDownloadState(DownloadState),
    /// Загрузить сохранённое состояние экрана «Загрузка».
    LoadDownloadState,
    /// Получить список уже скачанных документов (для значка «уже загружен»).
    /// Ответ — UiEvent::DownloadsListed.
    ListDownloads {
        profile_name: String,
        report_type: String,
    },
    /// Архив (П.6): список скачанных файлов с опциональными фильтрами.
    /// `None` = фильтр не выбран («все»). Ответ — UiEvent::ArchiveListed.
    ListArchive {
        profile_name: Option<String>,
        report_type: Option<String>,
        /// Диапазон дат фильтра архива [from, to] ("YYYY-MM-DD") из виджета
        /// интервала. None = без фильтра по дате. Совпадение — дата начала ИЛИ
        /// конца отчёта попадает в интервал (см. Catalog::list_downloads_filtered).
        date_range: Option<(String, String)>,
    },
    /// Архив (П.6): список уникальных report_type среди скачанных файлов
    /// (для combo «Отчёт»). Ответ — UiEvent::ArchiveReportTypesLoaded.
    LoadArchiveReportTypes,
    /// Архив: сохранить состояние фильтров (автосохранение при смене combo).
    SaveArchiveState(ArchiveState),
    /// Архив: загрузить сохранённое состояние фильтров (при старте).
    LoadArchiveState,
    /// Архив: удалить запись о скачивании и файл с диска (деструктивно).
    /// `file_path` — абсолютный путь к файлу для удаления. Ответ —
    /// UiEvent::DownloadDeleted(Result<id, error>) для refresh списка.
    DeleteDownload {
        id: i64,
        file_path: String,
    },
    // ===== Планировщик =====
    /// Список расписаний. Ответ — UiEvent::SchedulesListed.
    ListSchedules,
    /// Добавить расписание (один отчёт). После — reload списка.
    AddSchedule {
        name: String,
        profile_name: String,
        report_type: String,
        cron_expr: String,
        period_offset: i32,
    },
    /// Удалить расписание по имени. После — reload списка.
    DeleteSchedule {
        name: String,
    },
    /// Изменить расписание (имя, cron-выражение, смещение периода) по id.
    /// profile_id/reports/enabled сохраняются. После — reload списка.
    UpdateSchedule {
        id: i64,
        name: String,
        cron_expr: String,
        period_offset: i32,
    },
    /// Включить/выключить расписание. После — reload списка.
    SetScheduleEnabled {
        name: String,
        enabled: bool,
    },
    /// Запустить расписание вручную сейчас (по имени). Лог — через UiEvent::Log.
    RunScheduleNow {
        name: String,
    },
    /// Включить/выключить автозапуск с ОС. Ответ — UiEvent::AutostartChanged.
    SetAutostart {
        enabled: bool,
    },
    /// Включить/выключить фоновый планировщик Windows Task Scheduler
    /// (polling-задача → `mdwf schedule run`). Ответ — UiEvent::WinSchedulerChanged.
    SetWinScheduler {
        enabled: bool,
    },
}

/// Состояние фильтров экрана «Архив» для автосохранения между запусками.
/// Persist в `ui_state` по ключу `"archive_screen"`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ArchiveState {
    pub profile_name: Option<String>,
    pub report_type: Option<String>,
    /// Диапазон дат фильтра [from, to] ("YYYY-MM-DD") из виджета интервала.
    /// None = без фильтра по дате (все записи). (Прежнее поле `period` YYYY-MM
    /// удалено — архив теперь фильтруется стандартным интервалом, а совпадение —
    /// по дате начала/конца отчёта, попадающей в интервал.)
    pub date_range: Option<(String, String)>,
}

/// Соостояние экрана «Загрузка» для автосохранения между запусками.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct DownloadState {
    pub provider_id: Option<String>,
    pub profile_name: Option<String>,
    pub report_type: Option<String>,
    pub category: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub month: Option<String>,
    pub limit: Option<String>,
}

/// Активный магазин (выбор маркетплейса + профиля). Сохраняется в `ui_state`
/// (SQLite `mdwf.db`, ключ `"active_shop"`) — единый источник правды выбора
/// для всех вкладок (Загрузка, Отчёты). `profile_name` уникален в рамках БД.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ActiveShop {
    pub provider_id: String,
    pub profile_name: String,
}

/// События из доменного слоя в UI.
#[derive(Debug, Clone)]
pub enum UiEvent {
    /// Провайдеры загружены.
    ProvidersLoaded(Vec<ProviderInfo>),
    /// Профили загружены.
    ProfilesLoaded(Vec<Profile>),
    /// Поля авторизации провайдера загружены (для динамической формы профиля).
    AuthFieldsLoaded {
        provider_id: String,
        fields: Vec<AuthFieldInfo>,
    },
    /// Профиль сохранён.
    ProfileSaved(Result<i64, String>),
    /// Профиль удалён.
    ProfileDeleted(Result<(), String>),
    /// Результат проверки подключения.
    ProfileChecked(Result<HealthStatus, String>),
    /// Активный магазин изменился (выбор пользователя или восстановление).
    /// `seller_name` — имя продавца из API (Ozon company.name), может быть None
    /// (WB не предоставляет, или ошибка сети — тогда в заголовке имя профиля).
    ActiveShopChanged {
        provider_id: String,
        /// Статичное имя маркетплейса (напр. «Ozon»).
        provider_display_name: String,
        seller_name: Option<String>,
        /// Имя профиля (локальное имя, заданное пользователем) — fallback
        /// для заголовка, если seller_name = None.
        profile_name: String,
    },
    /// Сохранённый активный магазин загружен (при старте, из ui_state).
    ActiveShopLoaded(Option<ActiveShop>),
    /// Список отчётов загружен.
    ReportsLoaded(Result<Vec<ReportInfo>, String>),
    /// Список документов получен.
    DocumentsListed(Result<Vec<DocumentEntry>, String>),
    /// Список категорий документов WB получен.
    DocumentCategoriesLoaded(Result<Vec<DocumentCategoryInfo>, String>),
    /// Скачивание завершено (с полными путями к сохранённым файлам).
    DownloadFinished(Result<DownloadResult, String>),
    /// Прогресс операции.
    Progress {
        fraction: Option<f64>,
        message: String,
    },
    /// Текстовое уведомление (для статусбара/лога UI).
    Notify(String),
    /// Сохранённое состояние экрана «Загрузка» загружено (при старте).
    DownloadStateLoaded(Option<DownloadState>),
    /// Список уже скачанных документов (для значка «уже загружен»).
    /// `report_type` — для сопоставления с активным отчётом (устойчивость к гонке).
    DownloadsListed {
        report_type: String,
        docs: Vec<mdwf_storage::DownloadedDocInfo>,
    },
    /// Архив (П.6): результат запроса отфильтрованного списка скачиваний.
    ArchiveListed(Result<Vec<mdwf_storage::ArchiveEntry>, String>),
    /// Архив: список уникальных report_type с человекочитаемыми именами (combo «Отчёт»).
    /// combo показывает display_name, фильтр в БД — по type_id.
    ArchiveReportTypesLoaded(Vec<ReportTypeInfo>),
    /// Архив: сохранённое состояние фильтров загружено (при старте).
    ArchiveStateLoaded(Option<ArchiveState>),
    /// Архив: запись о скачивании удалена (id — для контекста; ошибка, если файл
    /// или БД не удалились). Список архива обновляется по этому событию.
    DownloadDeleted(Result<i64, String>),
    /// Журнал: новая запись (выгрузки/ошибки/запуски расписаний)._append во вкладку.
    Log(LogEntry),
    /// Планировщик: список расписаний (с резолвом имён профиля/отчётов).
    SchedulesListed(Result<Vec<ScheduleView>, String>),
    /// Планировщик: изменился автозапуск с ОС (новое состояние или ошибка).
    AutostartChanged(Result<bool, String>),
    /// Планировщик: изменился фоновый планировщик Windows Task Scheduler.
    WinSchedulerChanged(Result<bool, String>),
}

/// Расписание для отображения в Планировщике. Как `ScheduleRecord`, но с
/// человекочитаемыми именами (профиль, отчёты), резолвятся в app-loop.
#[derive(Debug, Clone)]
pub struct ScheduleView {
    pub id: i64,
    pub name: String,
    pub profile_id: i64,
    pub profile_name: String,
    pub reports: Vec<String>,
    pub report_names: Vec<String>,
    pub cron_expr: String,
    pub period_offset: i32,
    pub enabled: bool,
    pub next_run_at: Option<String>,
    pub last_run_at: Option<String>,
    pub last_run_status: Option<String>,
}

/// Результат скачивания: файлы + пути на диске.
#[derive(Debug, Clone)]
pub struct DownloadResult {
    pub files: Vec<DownloadedFile>,
    pub saved_paths: Vec<String>,
}

/// Уровень записи журнала (для цветового отличия строк).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    Info,
    Success,
    Error,
}

/// Одна запись журнала приложения. `timestamp` — локальное время «ЧЧ:ММ:СС».
/// Капается во вкладке «Журнал» (cap 500, вытеснение старых).
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub kind: LogKind,
    pub message: String,
}

/// Краткая информация о провайдере для UI.
#[derive(Debug, Clone)]
pub struct ProviderInfo {
    pub id: String,
    pub display_name: String,
}

/// Тип поля формы авторизации (упрощённое отображение для UI).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthFieldKindInfo {
    Text,
    Password,
    Number,
    Select(Vec<String>),
}

/// Поле формы авторизации для динамической отрисовки.
#[derive(Debug, Clone)]
pub struct AuthFieldInfo {
    pub id: String,
    pub label: String,
    pub kind: AuthFieldKindInfo,
    pub required: bool,
    pub placeholder: Option<String>,
    pub help_text: Option<String>,
    pub secret: bool,
}

/// Краткая информация об отчёте для UI.
#[derive(Debug, Clone)]
pub struct ReportInfo {
    pub type_id: String,
    pub display_name: String,
    pub category: String,
    pub is_browsable: bool,
    /// Какому провайдеру принадлежит отчёт (чтобы не угадывать из префикса type_id).
    pub provider_id: String,
    /// Какой период принимает отчёт (для «Скачать по периоду» и инфо-панели).
    pub period_kind: mdwf_core::PeriodKind,
    /// Человекочитаемое описание отчёта (для инфо-панели GUI).
    pub description: Option<String>,
    /// Жёсткий кап диапазона дат API (в днях). GUI режет длинные интервалы на окна.
    pub max_range_days: Option<u32>,
}

/// Пара type_id → человекочитаемое имя для combo «Отчёт» в Архиве.
/// `display_name` — что видит пользователь; `type_id` — технический фильтр в БД
/// (combo хранит label→value, как WB-категории). app-слой резолвит из реестра
/// провайдеров; если type_id неизвестен — display_name = type_id (fallback).
#[derive(Debug, Clone)]
pub struct ReportTypeInfo {
    pub type_id: String,
    pub display_name: String,
}

/// Категория документа для выпадающего списка в UI.
/// `label` — то, что видит пользователь (русское название, напр. «УПД»);
/// `value` — технический идентификатор, который WB ожидает в параметре category
/// (напр. «upd»). Разделение нужно, чтобы показывать понятные имена, не ломая API.
#[derive(Debug, Clone)]
pub struct DocumentCategoryInfo {
    pub label: String,
    pub value: String,
}

/// Выбранный пользователем документ для скачивания (Browsable-режим).
///
/// `id` — технический идентификатор провайдера (для WB это `serviceName`,
/// передаётся в `/documents/download`). `name` — человекочитаемое имя
/// (напр. «УПД №123»), используется как базовое имя файла на диске.
/// `extension` — предпочтительный формат из ответа `/documents/list`
/// (напр. `xml`), переопределяется реальным из ответа `/documents/download`.
#[derive(Debug, Clone)]
pub struct DocumentSel {
    pub id: String,
    pub name: Option<String>,
    pub extension: Option<String>,
    /// Дата документа (WB creationTime → YYYY-MM-DD). Пробрасывается до каталога
    /// (document_date) для фильтра периода Архива и плейсхолдера {doc_date}.
    pub date: Option<String>,
}

/// Отправитель команд в tokio-сторону (клонируется для виджетов).
#[derive(Clone)]
pub struct CommandSender {
    tx: mpsc::UnboundedSender<UiCommand>,
    /// Текущий токен отмены (общий для всех активных операций).
    cancel: Arc<Mutex<Option<CancellationToken>>>,
}

impl CommandSender {
    /// Создаёт пару (sender, receiver).
    pub fn channel() -> (Self, mpsc::UnboundedReceiver<UiCommand>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Self {
                tx,
                cancel: Arc::new(Mutex::new(None)),
            },
            rx,
        )
    }

    /// Отправляет команду (молча игнорирует ошибку, если UI закрыт).
    pub fn send(&self, cmd: UiCommand) {
        let _ = self.tx.send(cmd);
    }

    /// Регистрирует новый токен отмены для операции.
    pub fn set_cancel_token(&self, token: CancellationToken) {
        *self.cancel.lock() = Some(token);
    }

    /// Отменяет текущую операцию, если она есть.
    pub fn cancel_current(&self) {
        if let Some(t) = self.cancel.lock().take() {
            t.cancel();
        }
    }
}

/// Бридж: позволяет tokio-задачам слать `UiEvent` в GTK-поток.
///
/// Внутри хранит `async_channel::Sender<UiEvent>` (Sync, межпоточный).
/// UI-сторона читает события из парного receiver через `glib::MainContext::spawn_local`.
#[derive(Clone)]
pub struct EventForwarder {
    tx: async_channel::Sender<UiEvent>,
}

impl EventForwarder {
    #[must_use]
    pub fn new(tx: async_channel::Sender<UiEvent>) -> Self {
        Self { tx }
    }

    /// Отправляет событие в UI-поток.
    ///
    /// Канал bounded(256); GTK-поток непрерывно его читает, но при потоке
    /// Progress-событий возможно переполнение. Прежде try_send молча ронял ЛЮБОЕ
    /// событие — включая терминальные (DownloadFinished → UI навсегда «скачивание…»).
    /// Теперь: Progress (высокочастотный, низкая ценность) жертвуем при переполнении;
    /// всё остальное — ретраим с короткими паузами (главный поток быстро освобождает).
    pub fn forward(&self, event: UiEvent) {
        if matches!(event, UiEvent::Progress { .. }) {
            let _ = self.tx.try_send(event);
            return;
        }
        let mut ev = event;
        for _ in 0..100 {
            match self.tx.try_send(ev) {
                Ok(()) => return,
                Err(async_channel::TrySendError::Full(back)) => {
                    ev = back;
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                // UI закрыт — доставлять некому.
                Err(async_channel::TrySendError::Closed(_)) => return,
            }
        }
        tracing::error!("UiEvent потеряно: канал событий переполнен (ретраи исчерпаны)");
    }
}
