//! `App` — точка входа GUI. Создаёт GTK Application, tokio runtime,
//! доменный слой (registry + catalog + secrets), главное окно и маршрутизирует
//! события между UI и tokio.

use std::sync::Arc;

use anyhow::{Context, Result};
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use parking_lot::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use mdwf_config::ProvisionedConfig;
use mdwf_core::{ProviderRegistry, ProviderRef};
use mdwf_secrets::{InMemorySecretStore, OsKeychain, SecretStore};
use mdwf_storage::{Catalog, FileStore, FileStoreConfig, FileNameContext, FolderStructure};
use mdwf_test_provider::TestProvider;

use crate::channels::{
    CommandSender, EventForwarder, ProviderInfo, ReportInfo, UiCommand, UiEvent,
};
use crate::theme;

/// Идентификатор GTK-приложения.
pub const APP_ID: &str = "dev.mdwf.MDWF";

/// Корневой объект приложения (GUI + доменный слой).
pub struct App {
    gtk_app: adw::Application,
    /// Доменный слой (разделяемый между UI и tokio).
    domain: Arc<Domain>,
    command_sender: CommandSender,
    event_rx: async_channel::Receiver<UiEvent>,
}

/// Доменный слой: провайдеры + каталог + секреты + tokio runtime + config.
pub struct Domain {
    pub registry: ProviderRegistry,
    pub catalog: RwLock<Option<Catalog>>,
    pub secrets: Arc<dyn SecretStore>,
    pub runtime: RwLock<Option<tokio::runtime::Runtime>>,
    pub config: RwLock<ProvisionedConfig>,
    pub file_store: RwLock<FileStore>,
}

impl App {
    /// Создаёт приложение: инициализирует GTK, tokio, регистрирует провайдеров,
    /// провижинит конфиг, открывает SQLite-каталог.
    pub fn new() -> Result<Self> {
        // GTK init.
        gtk4::init().context("gtk4::init")?;
        adw::init().context("libadwaita::init")?;

        let gtk_app = adw::Application::new(Some(APP_ID), gtk4::gio::ApplicationFlags::FLAGS_NONE);

        // Конфиг + каталог + secrets.
        let prov = ProvisionedConfig::load_standard().context("load config")?;
        std::fs::create_dir_all(&prov.data_dir).ok();
        let catalog = Catalog::open(&prov.db_path).context("open catalog")?;

        let secrets: Arc<dyn SecretStore> = if prov.raw.security.use_keychain {
            Arc::new(OsKeychain::new())
        } else {
            Arc::new(InMemorySecretStore::new())
        };

        let file_store = FileStore::new(FileStoreConfig {
            output_dir: prov.output_dir.clone(),
            file_name_template: prov.raw.storage.file_name_template.clone(),
            folder_structure: parse_folder_structure(&prov.raw.storage.folder_structure),
            compute_hash: prov.raw.storage.compute_hash,
        });

        // Доменный слой.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("tokio runtime")?;

        let registry = ProviderRegistry::new();
        // Регистрируем провайдеров: TestProvider (mock) + Ozon + Wildberries.
        registry.register(Arc::new(TestProvider::new()) as ProviderRef)?;
        registry.register(Arc::new(mdwf_providers_ozon::OzonProvider::new()?) as ProviderRef)?;
        registry.register(Arc::new(
            mdwf_providers_wildberries::WildberriesProvider::new()?,
        ) as ProviderRef)?;

        info!(?prov.data_dir, ?prov.db_path, ?prov.output_dir, "config loaded");

        let domain = Arc::new(Domain {
            registry,
            catalog: RwLock::new(Some(catalog)),
            secrets,
            runtime: RwLock::new(Some(runtime)),
            config: RwLock::new(prov),
            file_store: RwLock::new(file_store),
        });

        // Каналы UI <-> tokio.
        let (command_sender, cmd_rx) = CommandSender::channel();
        let (event_tx, event_rx) = async_channel::bounded::<UiEvent>(256);

        let app = Self {
            gtk_app,
            domain,
            command_sender,
            event_rx,
        };

        // Брендовый CSS + схема цвета (спец. §2.5.4).
        theme::apply_brand_css();
        theme::set_color_scheme(theme::ColorScheme::System);

        let domain = app.domain.clone();
        let event_forwarder = EventForwarder::new(event_tx);

        // Запускаем обработчик команд в tokio runtime (через handle).
        let handle = {
            let guard = app.domain.runtime.read();
            guard.as_ref().expect("runtime initialized").handle().clone()
        };
        handle.spawn(async move {
            run_command_loop(cmd_rx, domain, event_forwarder).await;
        });

        let cs = app.command_sender.clone();
        let er = app.event_rx.clone();
        app.gtk_app.connect_activate(move |gtk_app| {
            crate::views::main_window::build_and_present(gtk_app, &cs, er.clone());
            // После построения окна — загружаем начальные данные.
            cs.send(UiCommand::LoadProviders);
            cs.send(UiCommand::LoadProfiles);
        });

        Ok(app)
    }

    /// Запускает GTK main loop. Возвращает код выхода процесса.
    pub fn run(self) -> std::process::ExitCode {
        let code = self.gtk_app.run();
        // Корректное завершение tokio runtime (забираем владение из Option).
        if let Some(rt) = self.domain.runtime.write().take() {
            rt.shutdown_background();
        }
        info!("MDWF GUI exited");
        std::process::ExitCode::from(code.value() as u8)
    }
}

/// Цикл обработки команд UI в tokio-стороне.
async fn run_command_loop(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<UiCommand>,
    domain: Arc<Domain>,
    fwd: EventForwarder,
) {
    info!("command loop started");
    while let Some(cmd) = rx.recv().await {
        match cmd {
            UiCommand::LoadProviders => {
                let providers = domain
                    .registry
                    .list()
                    .into_iter()
                    .map(|p| ProviderInfo {
                        id: p.id().to_string(),
                        display_name: p.display_name().to_string(),
                    })
                    .collect::<Vec<_>>();
                fwd.forward(UiEvent::ProvidersLoaded(providers));
            }
            UiCommand::LoadProfiles => {
                let result = match domain.catalog.read().as_ref() {
                    Some(cat) => cat.list_profiles().unwrap_or_default(),
                    None => Vec::new(),
                };
                fwd.forward(UiEvent::ProfilesLoaded(result));
            }
            UiCommand::SaveProfile(p) => {
                let outcome: Result<i64, String> = match domain.catalog.read().as_ref() {
                    Some(cat) => cat.upsert_profile(&p).map_err(|e| e.to_string()),
                    None => Err("каталог не открыт".into()),
                };
                fwd.forward(UiEvent::ProfileSaved(outcome));
                // Перезагружаем список.
                let _ = reload_profiles(&domain, &fwd).await;
            }
            UiCommand::DeleteProfile(name) => {
                let outcome: Result<(), String> = match domain.catalog.read().as_ref() {
                    Some(cat) => cat.delete_profile(&name).map_err(|e| e.to_string()),
                    None => Err("каталог не открыт".into()),
                };
                fwd.forward(UiEvent::ProfileDeleted(outcome));
                let _ = reload_profiles(&domain, &fwd).await;
            }
            UiCommand::CheckProfile(name) => {
                let outcome = check_profile(&domain, &name).await;
                fwd.forward(UiEvent::ProfileChecked(outcome));
            }
            UiCommand::LoadReports(provider_id) => {
                let outcome = load_reports(&domain, &provider_id).await;
                fwd.forward(UiEvent::ReportsLoaded(outcome));
            }
            UiCommand::ListDocuments {
                provider_id,
                profile_name,
                report_type,
                filter,
            } => {
                let cancel = CancellationToken::new();
                domain
                    .registry
                    .list()
                    .into_iter()
                    .find(|p| p.id() == provider_id)
                    .and_then(|_| None::<()>); // отфильтруем ниже
                let outcome = list_documents(&domain, &provider_id, &profile_name, &report_type, filter, cancel)
                    .await;
                fwd.forward(UiEvent::DocumentsListed(outcome));
            }
            UiCommand::Download {
                provider_id,
                profile_name,
                report_type,
                document_ids,
                params,
            } => {
                let cancel = CancellationToken::new();
                let outcome =
                    do_download(&domain, &provider_id, &profile_name, &report_type, document_ids, params, cancel, &fwd)
                        .await;
                fwd.forward(UiEvent::DownloadFinished(outcome));
            }
            UiCommand::Cancel => {
                fwd.forward(UiEvent::Notify("отмена не реализована в этом каркасе".into()));
            }
        }
    }
    warn!("command loop ended");
}

async fn reload_profiles(domain: &Domain, fwd: &EventForwarder) -> Result<(), ()> {
    let result = list_profiles_sync(domain);
    fwd.forward(UiEvent::ProfilesLoaded(result));
    Ok(())
}

/// Синхронное чтение списка профилей (guard живёт только внутри функции).
fn list_profiles_sync(domain: &Domain) -> Vec<mdwf_core::Profile> {
    let guard = domain.catalog.read();
    match guard.as_ref() {
        Some(cat) => cat.list_profiles().unwrap_or_default(),
        None => Vec::new(),
    }
}

/// Синхронное чтение профиля по имени (guard живёт только внутри функции).
fn read_profile(domain: &Domain, name: &str) -> Result<mdwf_core::Profile, String> {
    let guard = domain.catalog.read();
    let cat = guard.as_ref().ok_or("каталог не открыт")?;
    cat.get_profile_by_name(name)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("профиль '{name}' не найден"))
}

async fn check_profile(domain: &Domain, name: &str) -> Result<mdwf_core::HealthStatus, String> {
    let profile = read_profile(domain, name)?;
    let provider = domain
        .registry
        .require(&profile.provider_id)
        .map_err(|e| e.to_string())?;
    let auth = provider
        .authenticator(&profile)
        .await
        .map_err(|e| e.to_string())?;
    provider
        .health_check(auth.as_ref())
        .await
        .map_err(|e| e.to_string())
}

async fn load_reports(domain: &Domain, provider_id: &str) -> Result<Vec<ReportInfo>, String> {
    let provider = domain
        .registry
        .require(provider_id)
        .map_err(|e| e.to_string())?;
    let caps = provider.capabilities();
    let reports = caps
        .reports
        .iter()
        .map(|r| ReportInfo {
            type_id: r.type_id.clone(),
            display_name: r.display_name.clone(),
            category: format!("{:?}", r.category),
            is_browsable: r.acquisition_mode.is_browsable(),
        })
        .collect();
    Ok(reports)
}

async fn list_documents(
    domain: &Domain,
    provider_id: &str,
    profile_name: &str,
    report_type: &str,
    filter: mdwf_core::DocumentFilter,
    cancel: CancellationToken,
) -> Result<Vec<mdwf_core::DocumentEntry>, String> {
    let profile = read_profile(domain, profile_name)?;
    let provider = domain
        .registry
        .require(provider_id)
        .map_err(|e| e.to_string())?;
    let auth = provider
        .authenticator(&profile)
        .await
        .map_err(|e| e.to_string())?;
    let report = provider.report(report_type).await.map_err(|e| e.to_string())?;
    report
        .list(auth.as_ref(), &filter, cancel)
        .await
        .map_err(|e| e.to_string())
}

#[allow(clippy::too_many_arguments)]
async fn do_download(
    domain: &Domain,
    provider_id: &str,
    profile_name: &str,
    report_type: &str,
    document_ids: Vec<String>,
    mut params: mdwf_core::ReportParams,
    cancel: CancellationToken,
    fwd: &EventForwarder,
) -> Result<Vec<mdwf_core::DownloadedFile>, String> {
    let profile = read_profile(domain, profile_name)?;
    let provider = domain
        .registry
        .require(provider_id)
        .map_err(|e| e.to_string())?;
    let auth = provider
        .authenticator(&profile)
        .await
        .map_err(|e| e.to_string())?;
    let report = provider.report(report_type).await.map_err(|e| e.to_string())?;

    if !document_ids.is_empty() {
        params.values.insert("ids".into(), document_ids.join(","));
    }
    params.provider_id = Some(provider_id.into());
    params.profile_name = Some(profile_name.into());
    params.report_type = Some(report_type.into());

    let progress = std::sync::Arc::new(ProgressForwarder {
        fwd: fwd.clone(),
    }) as std::sync::Arc<dyn mdwf_core::ProgressCallback>;

    fwd.forward(UiEvent::Progress {
        fraction: Some(0.0),
        message: "начало выгрузки…".into(),
    });
    let files = report
        .download(auth.as_ref(), &params, progress, cancel)
        .await
        .map_err(|e| e.to_string());

    fwd.forward(UiEvent::Progress {
        fraction: Some(0.9),
        message: "запись файлов на диск…".into(),
    });

    let files = files?;
    let saved = persist_files(domain, &files, provider_id, profile_name, report_type, &params).await;
    match saved {
        Ok(count) => fwd.forward(UiEvent::Notify(format!("Сохранено файлов: {count}"))),
        Err(e) => fwd.forward(UiEvent::Notify(format!("ошибка записи: {e}"))),
    }

    fwd.forward(UiEvent::Progress {
        fraction: Some(1.0),
        message: "выгрузка завершена".into(),
    });
    Ok(files)
}

/// Записывает скачанные файлы на диск через FileStore и регистрирует в каталоге.
async fn persist_files(
    domain: &Domain,
    files: &[mdwf_core::DownloadedFile],
    provider_id: &str,
    profile_name: &str,
    report_type: &str,
    params: &mdwf_core::ReportParams,
) -> Result<usize, String> {
    let profile = read_profile(domain, profile_name)?;
    let profile_id = profile.id.ok_or("профиль без id")?;

    let mut count = 0usize;
    for f in files {
        let content = f.content.as_ref().ok_or("файл без контента")?;
        let period = params.period.as_deref();
        let ctx = FileNameContext {
            provider_id,
            profile_name,
            report_type,
            period,
            extension: &f.extension,
            document_id: f.source_id.as_deref(),
            document_date: None,
        };

        // Запись на диск (клонируем FileStore-конфиг, т.к. RwLock).
        let stored = {
            let store = domain.file_store.read().clone();
            store.save(content, &ctx).map_err(|e| e.to_string())?
        };

        // Регистрация в каталоге (с дедупликацией по хэшу).
        if let Some(cat) = domain.catalog.read().as_ref() {
            let new_dl = mdwf_storage::NewDownload {
                profile_id,
                report_type: report_type.to_string(),
                period: period.map(str::to_string),
                params: Some(serde_json::to_string(params).unwrap_or_default()),
                file_path: format!("{} ({} байт)", stored.file_name, stored.size),
                file_size: {
                    #[allow(clippy::cast_possible_wrap)]
                    {
                        stored.size as i64
                    }
                },
                file_hash: stored.sha256.clone(),
                file_format: stored.extension.clone(),
                rows_count: None,
                downloader_kind: "Api".to_string(),
                source_url: stored.source_url.clone(),
            };
            let _ = cat.record_download(&new_dl);
        }
        count += 1;
    }
    Ok(count)
}

/// Разбор строкового имени структуры папок.
fn parse_folder_structure(s: &str) -> FolderStructure {
    match s {
        "flat" => FolderStructure::Flat,
        "by_provider_profile_period" => FolderStructure::ByProviderProfilePeriod,
        _ => FolderStructure::ByProviderPeriod,
    }
}

/// Адаптер: `ProgressCallback` -> пересылает прогресс в UI через `EventForwarder`.
struct ProgressForwarder {
    fwd: EventForwarder,
}

impl mdwf_core::ProgressCallback for ProgressForwarder {
    fn report(&self, update: mdwf_core::ProgressUpdate) {
        self.fwd.forward(UiEvent::Progress {
            fraction: update.fraction,
            message: update.message,
        });
    }
}
