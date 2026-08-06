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
    ActiveShop, AuthFieldInfo, AuthFieldKindInfo, CommandSender, EventForwarder, ProviderInfo,
    ReportInfo, UiCommand, UiEvent,
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

        // Регистрируем gresource-бандл в теме иконок — чтобы иконки маркетплейсов
        // (mdwf-ozon, mdwf-wildberries, ...) грузились по имени через
        // Image::set_icon_name, как обычные theme-иконки. gresource-префикс
        // /org/mdwf/icons соответствует структуре theme (scalable/<name>.svg).
        if let Some(display) = gtk4::gdk::Display::default() {
            gtk4::IconTheme::for_display(&display).add_resource_path("/org/mdwf/icons");
        }

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
            // Загружаем сохранённый активный магазин (выбор из прошлого сеанса).
            cs.send(UiCommand::LoadActiveShop);
            // Загружаем сохранённое состояние экрана «Загрузка».
            cs.send(UiCommand::LoadDownloadState);
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
            UiCommand::SaveProfile(mut p) => {
                // Выносим секреты в keyring, в auth_metadata оставляем только
                // несекретные поля (напр. client_id). В SQLite секреты не пишем.
                let outcome: Result<i64, String> = async {
                    let provider = domain
                        .registry
                        .require(&p.provider_id)
                        .map_err(|e| e.to_string())?;
                    let caps = provider.capabilities();
                    let secret_fields = mdwf_secrets::secret_field_ids(caps);
                    mdwf_secrets::store_profile_secrets(
                        &mut p,
                        &secret_fields,
                        domain.secrets.as_ref(),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                    let cat = domain.catalog.read();
                    let cat = cat.as_ref().ok_or("каталог не открыт")?;
                    cat.upsert_profile(&p).map_err(|e| e.to_string())
                }
                .await;
                fwd.forward(UiEvent::ProfileSaved(outcome));
                // Перезагружаем список.
                let _ = reload_profiles(&domain, &fwd).await;
            }
            UiCommand::DeleteProfile(name) => {
                // Сначала удаляем секреты профиля из keyring, потом — строку из БД.
                let outcome: Result<(), String> = async {
                    // Читаем профиль, чтобы знать provider_id (для capabilities/key).
                    if let Ok(Some(profile)) = read_profile_opt(&domain, &name) {
                        let provider = domain.registry.require(&profile.provider_id).ok();
                        if let Some(provider) = provider {
                            let caps = provider.capabilities();
                            let secret_fields = mdwf_secrets::secret_field_ids(caps);
                            let _ = mdwf_secrets::delete_profile_secrets(
                                &profile.provider_id,
                                &profile.name,
                                &secret_fields,
                                domain.secrets.as_ref(),
                            )
                            .await;
                        }
                    }
                    let cat = domain.catalog.read();
                    let cat = cat.as_ref().ok_or("каталог не открыт")?;
                    cat.delete_profile(&name).map_err(|e| e.to_string())
                }
                .await;
                fwd.forward(UiEvent::ProfileDeleted(outcome));
                let _ = reload_profiles(&domain, &fwd).await;
            }
            UiCommand::CheckProfile(name) => {
                let outcome = check_profile(&domain, &name).await;
                fwd.forward(UiEvent::ProfileChecked(outcome));
            }
            UiCommand::SelectShop {
                provider_id,
                profile_name,
            } => {
                // Persist активного магазина в ui_state (единый источник правды).
                if let Some(cat) = domain.catalog.read().as_ref() {
                    let shop = ActiveShop {
                        provider_id: provider_id.clone(),
                        profile_name: profile_name.clone(),
                    };
                    let json = serde_json::to_string(&shop).unwrap_or_default();
                    if let Err(e) = cat.set_ui_state("active_shop", &json) {
                        tracing::warn!(error = %e, "failed to save active shop");
                    }
                }
                // Запрашиваем имя продавца из API для заголовка окна.
                // Ошибка fetch не блокирует смену магазина — seller_name = None.
                let seller_name = fetch_seller_name(&domain, &provider_id, &profile_name).await;
                let provider_display_name = match domain.registry.require(&provider_id) {
                    Ok(p) => p.display_name().to_string(),
                    Err(_) => provider_id.clone(),
                };
                fwd.forward(UiEvent::ActiveShopChanged {
                    provider_id,
                    provider_display_name,
                    seller_name,
                    profile_name,
                });
            }
            UiCommand::LoadActiveShop => {
                let shop = domain.catalog.read().as_ref().and_then(|cat| {
                    cat.get_ui_state("active_shop").ok().flatten().and_then(
                        |json| serde_json::from_str::<ActiveShop>(&json).ok(),
                    )
                });
                fwd.forward(UiEvent::ActiveShopLoaded(shop));
            }
            UiCommand::LoadReports(provider_id) => {
                let outcome = load_reports(&domain, &provider_id).await;
                fwd.forward(UiEvent::ReportsLoaded(outcome));
            }
            UiCommand::LoadDocumentCategories {
                provider_id,
                profile_name,
            } => {
                tracing::info!("LoadDocumentCategories: {provider_id} / {profile_name}");
                let outcome = load_document_categories(&domain, &provider_id, &profile_name).await;
                tracing::info!("LoadDocumentCategories result: {:?}", outcome.as_ref().map(std::vec::Vec::len));
                fwd.forward(UiEvent::DocumentCategoriesLoaded(outcome));
            }
            UiCommand::LoadAuthFields(provider_id) => {
                let fields = load_auth_fields(&domain, &provider_id);
                fwd.forward(UiEvent::AuthFieldsLoaded {
                    provider_id,
                    fields,
                });
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
                let progress = std::sync::Arc::new(ProgressForwarder {
                    fwd: fwd.clone(),
                }) as std::sync::Arc<dyn mdwf_core::ProgressCallback>;
                let outcome = list_documents(&domain, &provider_id, &profile_name, &report_type, filter, progress, cancel)
                    .await;
                fwd.forward(UiEvent::DocumentsListed(outcome));
            }
            UiCommand::Download {
                provider_id,
                profile_name,
                report_type,
                documents,
                params,
            } => {
                let cancel = CancellationToken::new();
                let outcome =
                    do_download(&domain, &provider_id, &profile_name, &report_type, documents, params, cancel, &fwd)
                        .await;
                fwd.forward(UiEvent::DownloadFinished(outcome));
            }
            UiCommand::Cancel => {
                fwd.forward(UiEvent::Notify("отмена не реализована в этом каркасе".into()));
            }
            UiCommand::SaveDownloadState(state) => {
                if let Some(cat) = domain.catalog.read().as_ref() {
                    let json = serde_json::to_string(&state).unwrap_or_default();
                    if let Err(e) = cat.set_ui_state("download_screen", &json) {
                        tracing::warn!(error = %e, "failed to save download state");
                    }
                }
            }
            UiCommand::LoadDownloadState => {
                let state = domain.catalog.read().as_ref().and_then(|cat| {
                    cat.get_ui_state("download_screen").ok().flatten().and_then(
                        |json| serde_json::from_str::<crate::channels::DownloadState>(&json).ok(),
                    )
                });
                fwd.forward(UiEvent::DownloadStateLoaded(state));
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
/// Возвращает профиль **без секретов** (в auth_metadata только несекретные поля).
/// Для API-вызовов используйте `read_profile_with_secrets` — она подмешает
/// секреты из keyring.
fn read_profile(domain: &Domain, name: &str) -> Result<mdwf_core::Profile, String> {
    let guard = domain.catalog.read();
    let cat = guard.as_ref().ok_or("каталог не открыт")?;
    cat.get_profile_by_name(name)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("профиль '{name}' не найден"))
}

/// Как `read_profile`, но возвращает Option (None если каталог закрыт или профиль
/// не найден). Удобно для необязательных операций (напр. удаление секрета при
/// удалении профиля — профиль мог быть уже удалён).
fn read_profile_opt(domain: &Domain, name: &str) -> Result<Option<mdwf_core::Profile>, String> {
    let guard = domain.catalog.read();
    let cat = match guard.as_ref() {
        Some(c) => c,
        None => return Ok(None),
    };
    cat.get_profile_by_name(name)
        .map_err(|e| e.to_string())
}

/// Читает профиль и подмешивает секреты из keyring (для передачи в
/// `provider.authenticator`). Секреты хранятся только в keyring, в БД их нет —
/// поэтому перед вызовом провайдера их нужно достать и вставить в auth_metadata
/// in-memory. Провайдеры читают секрет из auth_metadata как раньше.
async fn read_profile_with_secrets(
    domain: &Domain,
    name: &str,
) -> Result<mdwf_core::Profile, String> {
    let profile = read_profile(domain, name)?;
    let provider = domain
        .registry
        .require(&profile.provider_id)
        .map_err(|e| e.to_string())?;
    let caps = provider.capabilities();
    let secret_fields = mdwf_secrets::secret_field_ids(caps);
    mdwf_secrets::load_profile_secrets(profile, &secret_fields, domain.secrets.as_ref())
        .await
        .map_err(|e| e.to_string())
}

async fn check_profile(domain: &Domain, name: &str) -> Result<mdwf_core::HealthStatus, String> {
    let profile = read_profile_with_secrets(domain, name).await?;
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

/// Запрашивает имя продавца из API (Ozon `/v1/seller/info` → company.name).
/// При любой ошибке (нет эндпоинта у WB, сеть, auth) возвращает `None` —
/// заголовок покажет локальное имя профиля. НЕ блокирует смену магазина.
async fn fetch_seller_name(
    domain: &Domain,
    provider_id: &str,
    profile_name: &str,
) -> Option<String> {
    let profile = match read_profile_with_secrets(domain, profile_name).await {
        Ok(p) => p,
        Err(e) => {
            tracing::debug!(error = %e, "fetch_seller_name: profile read failed");
            return None;
        }
    };
    let provider = domain.registry.require(provider_id).ok()?;
    let auth = provider.authenticator(&profile).await.ok()?;
    match provider.account_display_name(auth.as_ref()).await {
        Ok(name) => name,
        Err(e) => {
            tracing::debug!(error = %e, "fetch_seller_name: API call failed");
            None
        }
    }
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
            provider_id: provider_id.to_string(),
        })
        .collect();
    Ok(reports)
}

/// Загружает список категорий документов WB через API.
async fn load_document_categories(
    domain: &Domain,
    provider_id: &str,
    profile_name: &str,
) -> Result<Vec<crate::channels::DocumentCategoryInfo>, String> {
    let profile = read_profile_with_secrets(domain, profile_name).await?;
    let provider = domain
        .registry
        .require(provider_id)
        .map_err(|e| e.to_string())?;
    let auth = provider
        .authenticator(&profile)
        .await
        .map_err(|e| e.to_string())?;
    let report = provider
        .report("wb.documents_categories")
        .await
        .map_err(|e| e.to_string())?;
    let entries = report
        .list(
            auth.as_ref(),
            &mdwf_core::DocumentFilter::default(),
            std::sync::Arc::new(mdwf_core::NoopProgress) as std::sync::Arc<dyn mdwf_core::ProgressCallback>,
            mdwf_core::CancelToken::new(),
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(entries
        .into_iter()
        .map(|e| crate::channels::DocumentCategoryInfo {
            // display_name у WbCategoriesReport = title (если есть) либо name.
            label: if e.display_name.is_empty() {
                e.id.clone()
            } else {
                e.display_name
            },
            value: e.id,
        })
        .collect())
}

/// Загружает поля формы авторизации провайдера (для динамической отрисовки).
fn load_auth_fields(domain: &Domain, provider_id: &str) -> Vec<AuthFieldInfo> {
    let Ok(provider) = domain.registry.require(provider_id) else {
        return Vec::new();
    };
    provider
        .capabilities()
        .auth_fields
        .iter()
        .map(|f| AuthFieldInfo {
            id: f.id.clone(),
            label: f.label.clone(),
            kind: match &f.kind {
                mdwf_core::AuthFieldKind::Text => AuthFieldKindInfo::Text,
                mdwf_core::AuthFieldKind::Password => AuthFieldKindInfo::Password,
                mdwf_core::AuthFieldKind::Number => AuthFieldKindInfo::Number,
                mdwf_core::AuthFieldKind::Select(opts) => {
                    AuthFieldKindInfo::Select(opts.clone())
                }
            },
            required: f.required,
            placeholder: f.placeholder.clone(),
            help_text: f.help_text.clone(),
            secret: f.secret,
        })
        .collect()
}

async fn list_documents(
    domain: &Domain,
    provider_id: &str,
    profile_name: &str,
    report_type: &str,
    filter: mdwf_core::DocumentFilter,
    progress: std::sync::Arc<dyn mdwf_core::ProgressCallback>,
    cancel: CancellationToken,
) -> Result<Vec<mdwf_core::DocumentEntry>, String> {
    let profile = read_profile_with_secrets(domain, profile_name).await?;
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
        .list(auth.as_ref(), &filter, progress, cancel)
        .await
        .map_err(|e| e.to_string())
}

/// Сериализуемая мета выбранного документа: передаётся в провайдер через
/// `params.values["doc_meta"]` (JSON-массив). Провайдер использует `name`
/// как базовое имя файла, `extension` — как предпочтительный формат.
#[derive(serde::Serialize)]
struct DocMetaItem {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    extension: Option<String>,
}

#[allow(clippy::too_many_arguments)]
async fn do_download(
    domain: &Domain,
    provider_id: &str,
    profile_name: &str,
    report_type: &str,
    documents: Vec<crate::channels::DocumentSel>,
    mut params: mdwf_core::ReportParams,
    cancel: CancellationToken,
    fwd: &EventForwarder,
) -> Result<crate::channels::DownloadResult, String> {
    let profile = read_profile_with_secrets(domain, profile_name).await?;
    let provider = domain
        .registry
        .require(provider_id)
        .map_err(|e| e.to_string())?;
    let auth = provider
        .authenticator(&profile)
        .await
        .map_err(|e| e.to_string())?;
    let report = provider.report(report_type).await.map_err(|e| e.to_string())?;

    if !documents.is_empty() {
        // "ids" — CSV для совместимости (напр. CLI-логика).
        let ids: Vec<&str> = documents.iter().map(|d| d.id.as_str()).collect();
        params.values.insert("ids".into(), ids.join(","));
        // "doc_meta" — JSON-массив {id,name,extension}, чтобы провайдер
        // знал человекочитаемое имя (для имени файла) и предпочтительное
        // расширение. Сериализация не должна падать на корректных данных.
        let meta: Vec<DocMetaItem> = documents
            .iter()
            .map(|d| DocMetaItem {
                id: d.id.clone(),
                name: d.name.clone(),
                extension: d.extension.clone(),
            })
            .collect();
        if let Ok(json) = serde_json::to_string(&meta) {
            params.values.insert("doc_meta".into(), json);
        }
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

    fwd.forward(UiEvent::Progress {
        fraction: Some(1.0),
        message: "выгрузка завершена".into(),
    });

    match saved {
        Ok(paths) => Ok(crate::channels::DownloadResult {
            files,
            saved_paths: paths,
        }),
        Err(e) => Err(e),
    }
}

/// Записывает скачанные файлы на диск через FileStore и регистрирует в каталоге.
/// Возвращает вектор полных путей к сохранённым файлам.
async fn persist_files(
    domain: &Domain,
    files: &[mdwf_core::DownloadedFile],
    provider_id: &str,
    profile_name: &str,
    report_type: &str,
    params: &mdwf_core::ReportParams,
) -> Result<Vec<String>, String> {
    let profile = read_profile(domain, profile_name)?;
    let profile_id = profile.id.ok_or("профиль без id")?;

    let mut saved_paths = Vec::new();
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
        let (stored, dir) = {
            let store = domain.file_store.read().clone();
            store.save_with_dir(content, &ctx).map_err(|e| e.to_string())?
        };
        let full_path = dir.join(&stored.file_name);
        let full_path_str = full_path.display().to_string();
        saved_paths.push(full_path_str.clone());

        // Регистрация в каталоге (с дедупликацией по хэшу).
        if let Some(cat) = domain.catalog.read().as_ref() {
            let new_dl = mdwf_storage::NewDownload {
                profile_id,
                report_type: report_type.to_string(),
                period: period.map(str::to_string),
                params: Some(serde_json::to_string(params).unwrap_or_default()),
                file_path: full_path_str,
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
    }
    Ok(saved_paths)
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
