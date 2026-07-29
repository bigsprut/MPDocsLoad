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
    Profiles,
    Reports,
    Download,
    Settings,
    Scheduler,
    Logs,
    About,
}

impl ViewId {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Profiles => "profiles",
            Self::Reports => "reports",
            Self::Download => "download",
            Self::Settings => "settings",
            Self::Scheduler => "scheduler",
            Self::Logs => "logs",
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
    /// Загрузить список отчётов провайдера.
    LoadReports(String),
    /// Получить список документов (Browsable-режим).
    ListDocuments {
        provider_id: String,
        profile_name: String,
        report_type: String,
        filter: DocumentFilter,
    },
    /// Скачать выбранные документы (Browsable) или сгенерировать отчёт (Period).
    Download {
        provider_id: String,
        profile_name: String,
        report_type: String,
        /// Для Browsable: id выбранных документов.
        /// Для Period: пусто (использует params.period).
        document_ids: Vec<String>,
        params: mdwf_core::ReportParams,
    },
    /// Отмена текущей операции.
    Cancel,
    /// Сохранить состояние экрана «Загрузка» (автосохранение).
    SaveDownloadState(DownloadState),
    /// Загрузить сохранённое состояние экрана «Загрузка».
    LoadDownloadState,
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
    /// Список отчётов загружен.
    ReportsLoaded(Result<Vec<ReportInfo>, String>),
    /// Список документов получен.
    DocumentsListed(Result<Vec<DocumentEntry>, String>),
    /// Скачивание завершено.
    DownloadFinished(Result<Vec<DownloadedFile>, String>),
    /// Прогресс операции.
    Progress {
        fraction: Option<f64>,
        message: String,
    },
    /// Текстовое уведомление (для статусбара/лога UI).
    Notify(String),
    /// Сохранённое состояние экрана «Загрузка» загружено (при старте).
    DownloadStateLoaded(Option<DownloadState>),
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

    /// Отправляет событие в UI-поток (молча игнорирует ошибку).
    pub fn forward(&self, event: UiEvent) {
        // try_send: не блокируем tokio-задачу; канал bounded.
        let _ = self.tx.try_send(event);
    }
}
