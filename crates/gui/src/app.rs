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
#[cfg(debug_assertions)]
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

        // gresource-бандл (иконки маркетплейсов, PNG) уже зарегистрирован в main.rs
        // через gio::resources_register_include. Иконки грузятся напрямую через
        // Image::set_from_resource("resource:///org/mdwf/icons/<name>.png") —
        // без регистрации в IconTheme (надёжнее на Windows/MinGW).

        // Брендовая иконка ЗАПУЩЕННОГО окна (titlebar + таскбар). exe-иконку (winres)
        // видно в проводнике/ярлыках, а у запущенного окна без этого — generic-иконка.
        // Имя «mdwf» резолвится из on-disk темы hicolor бандла
        // (share/icons/hicolor/<size>/apps/mdwf.png — кладёт scripts/build-release.sh).
        // Путь GTK4 add_resource_path + hicolor-layout в gresource на Windows НЕ работает
        // (has_icon=false при всех проверенных паттернах) — поэтому только disk-hicolor.
        gtk4::Window::set_default_icon_name("mdwf");
        // Диагностика: нашлась ли app-иконка в теме (должна — из disk-hicolor бандла).
        tracing::debug!(
            has_mdwf = gtk4::IconTheme::default().has_icon("mdwf"),
            "app icon 'mdwf' in theme"
        );

        let gtk_app = adw::Application::new(Some(APP_ID), gtk4::gio::ApplicationFlags::FLAGS_NONE);

        // Конфиг + каталог + secrets.
        let domain = build_domain(ProvisionedConfig::load_standard().context("load config")?, false)
            .context("build domain")?;

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
        let sched_domain = app.domain.clone();
        let sched_fwd = event_forwarder.clone();
        handle.spawn(async move {
            run_command_loop(cmd_rx, domain, event_forwarder).await;
        });

        // Фоновый планировщик: автозагрузка по cron, пока GUI открыт.
        handle.spawn(async move {
            let cfg = sched_domain.config.read().clone();
            if !cfg.raw.scheduler.enabled_on_start {
                tracing::info!("scheduler: enabled_on_start=false — фоновый цикл не запущен");
                return;
            }
            let catalog = match sched_domain.catalog.read().clone() {
                Some(c) => c,
                None => return,
            };
            let executor = Arc::new(GuiJobExecutor {
                domain: sched_domain.clone(),
                fwd: sched_fwd,
                manual_run: false,
            }) as Arc<dyn mdwf_scheduler::JobExecutor>;
            let max_parallel = cfg.raw.scheduler.max_parallel_jobs;
            let runner = Arc::new(mdwf_scheduler::Runner::new(catalog, executor, max_parallel));
            runner.run_loop(std::time::Duration::from_secs(60)).await;
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
            // Архив (П.6): список report_types + восстановление фильтров.
            // Начальный ListArchive отправляется из on_archive_state_loaded
            // (с восстановленными значениями, либо None если состояния нет).
            cs.send(UiCommand::LoadArchiveReportTypes);
            // Журнал: история событий из БД (переживает перезапуск).
            cs.send(UiCommand::LoadJournal);
            cs.send(UiCommand::LoadArchiveState);
            // Автопроверка обновлений (GitHub Releases, анонимно): при
            // наличии новой версии — диалог; иначе молча.
            cs.send(UiCommand::CheckUpdates { manual: false });
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

/// Строит доменный слой (registry + catalog + secrets + runtime + config).
/// Без единого GTK-вызова — используется и `App::new`, и headless-драйвер
/// self-test (`--self-test`), которому окно не нужно.
///
/// `in_memory_secrets` — true для self-test: ключи keyring не трогаем,
/// тестовым профилям секреты не нужны.
pub(crate) fn build_domain(
    prov: ProvisionedConfig,
    in_memory_secrets: bool,
) -> Result<Arc<Domain>> {
    std::fs::create_dir_all(&prov.data_dir).ok();
    let catalog = Catalog::open(&prov.db_path).context("open catalog")?;

    let secrets: Arc<dyn SecretStore> = if in_memory_secrets || !prov.raw.security.use_keychain {
        Arc::new(InMemorySecretStore::new())
    } else {
        Arc::new(OsKeychain::new())
    };

    let file_store = FileStore::new(FileStoreConfig {
        output_dir: prov.output_dir.clone(),
        file_name_template: prov.raw.storage.file_name_template.clone(),
        folder_structure: parse_folder_structure(&prov.raw.storage.folder_structure),
        compute_hash: prov.raw.storage.compute_hash,
    });

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("tokio runtime")?;

    let registry = ProviderRegistry::new();
    // Регистрируем провайдеров: Ozon + Wildberries (+ TestProvider (mock)
    // только в debug-сборках — в релизе пользователю mock не нужен).
    #[cfg(debug_assertions)]
    registry.register(Arc::new(TestProvider::new()) as ProviderRef)?;
    registry.register(Arc::new(mdwf_providers_ozon::OzonProvider::new()?) as ProviderRef)?;
    registry.register(Arc::new(
        mdwf_providers_wildberries::WildberriesProvider::new()?,
    ) as ProviderRef)?;

    info!(?prov.data_dir, ?prov.db_path, ?prov.output_dir, "config loaded");

    Ok(Arc::new(Domain {
        registry,
        catalog: RwLock::new(Some(catalog)),
        secrets,
        runtime: RwLock::new(Some(runtime)),
        config: RwLock::new(prov),
        file_store: RwLock::new(file_store),
    }))
}

/// Цикл обработки команд UI в tokio-стороне. pub(crate): переиспользуется
/// headless-драйвером self-test (`--self-test`).
pub(crate) async fn run_command_loop(
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
                cancel,
            } => {
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
                cancel,
            } => {
                let outcome =
                    do_download(&domain, &provider_id, &profile_name, &report_type, documents, params, cancel, &fwd, &mdwf_core::LogOrigin::ManualGui)
                        .await;
                fwd.forward(UiEvent::DownloadFinished(outcome));
                // Обновляем значки «уже загружен» после скачивания (cross-session).
                let docs = (|| {
                    let cat = domain.catalog.read();
                    let cat = cat.as_ref()?;
                    let profile = cat.get_profile_by_name(&profile_name).ok()??;
                    let pid = profile.id?;
                    cat.list_downloaded_docs(pid, &report_type).ok()
                })()
                .unwrap_or_default();
                fwd.forward(UiEvent::DownloadsListed {
                    report_type,
                    docs,
                });
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
            UiCommand::ListDownloads {
                profile_name,
                report_type,
            } => {
                // Резолвим profile_name → profile_id, затем список скачанных документов.
                let docs = (|| {
                    let cat = domain.catalog.read();
                    let cat = cat.as_ref()?;
                    let profile = cat.get_profile_by_name(&profile_name).ok()??;
                    let pid = profile.id?;
                    cat.list_downloaded_docs(pid, &report_type).ok()
                })()
                .unwrap_or_default();
                fwd.forward(UiEvent::DownloadsListed {
                    report_type,
                    docs,
                });
            }
            UiCommand::ListArchive {
                profile_name,
                report_type,
                date_range,
            } => {
                // Архив: опциональный фильтр по профилю резолвим в profile_id.
                // date_range [from,to] из виджета интервала передаётся каталогу как есть;
                // совпадение — дата начала/конца отчёта попадает в интервал (catalog).
                let outcome = (|| {
                    let cat = domain.catalog.read();
                    let cat = cat.as_ref()?;
                    // None = не фильтровать (показать все профили).
                    let profile_id = match &profile_name {
                        Some(name) => Some(cat.get_profile_by_name(name).ok()??.id?),
                        None => None,
                    };
                    cat.list_downloads_filtered(profile_id, report_type.as_deref(), date_range)
                        .ok()
                })();
                match outcome {
                    Some(mut entries) => {
                        // Обогащаем человекочитаемыми именами отчётов и ссылками
                        // ЛК (storage не имеет доступа к реестру провайдеров —
                        // делаем здесь).
                        let dm = report_display_name_map(&domain);
                        let um = cabinet_url_map(&domain);
                        for e in &mut entries {
                            e.report_display_name = dm.get(&e.report_type).cloned();
                            e.cabinet_url = um.get(&e.report_type).cloned();
                        }
                        fwd.forward(UiEvent::ArchiveListed(Ok(entries)));
                    }
                    None => fwd.forward(UiEvent::ArchiveListed(Err(
                        "каталог недоступен".to_string(),
                    ))),
                }
            }
            UiCommand::LoadArchiveReportTypes => {
                let rts = domain
                    .catalog
                    .read()
                    .as_ref()
                    .and_then(|cat| cat.distinct_report_types().ok())
                    .unwrap_or_default();
                // Резолвим type_id → display_name через реестр провайдеров
                // (capabilities().reports, синхронно, без API). Fallback — сам type_id.
                let dm = report_display_name_map(&domain);
                let infos: Vec<crate::channels::ReportTypeInfo> = rts
                    .iter()
                    .map(|t| crate::channels::ReportTypeInfo {
                        display_name: dm.get(t).cloned().unwrap_or_else(|| t.clone()),
                        type_id: t.clone(),
                    })
                    .collect();
                fwd.forward(UiEvent::ArchiveReportTypesLoaded(infos));
            }
            UiCommand::SaveArchiveState(state) => {
                if let Some(cat) = domain.catalog.read().as_ref() {
                    let json = serde_json::to_string(&state).unwrap_or_default();
                    if let Err(e) = cat.set_ui_state("archive_screen", &json) {
                        tracing::warn!(error = %e, "failed to save archive state");
                    }
                }
            }
            UiCommand::LoadArchiveState => {
                let state = domain.catalog.read().as_ref().and_then(|cat| {
                    cat.get_ui_state("archive_screen").ok().flatten().and_then(
                        |json| serde_json::from_str::<crate::channels::ArchiveState>(&json).ok(),
                    )
                });
                fwd.forward(UiEvent::ArchiveStateLoaded(state));
            }
            UiCommand::DeleteDownload { id, file_path } => {
                // Деструктивно: удаляем запись из БД, затем файл с диска.
                // Ошибка удаления файла НЕ блокирует удаление записи — warn в лог
                // (запись уже удалена, осиротевший файл останется, это допустимо).
                let outcome = (|| {
                    let cat = domain.catalog.read();
                    let cat = cat.as_ref()?;
                    cat.delete_download(id).ok()
                })();
                match outcome {
                    Some(()) => {
                        // Удаляем файл с диска.
                        if let Err(e) = std::fs::remove_file(&file_path) {
                            // Нет файла — не ошибка (уже удалён/перемещён); прочее — warn.
                            if e.kind() != std::io::ErrorKind::NotFound {
                                tracing::warn!(error = %e, %file_path, "failed to delete file");
                            }
                        }
                        fwd.forward(UiEvent::DownloadDeleted(Ok(id)));
                    }
                    None => fwd.forward(UiEvent::DownloadDeleted(Err(
                        "каталог недоступен".to_string(),
                    ))),
                }
            }
            // ===== Планировщик =====
            UiCommand::ListSchedules => {
                fwd.forward(UiEvent::SchedulesListed(list_schedule_views(&domain)));
            }
            UiCommand::AddSchedule {
                name,
                profile_name,
                report_type,
                cron_expr,
                period_offset,
            } => {
                let outcome: Result<(), String> = (|| {
                    let cat = domain.catalog.read();
                    let cat = cat.as_ref().ok_or("каталог недоступен")?;
                    let next = mdwf_scheduler::next_run(&cron_expr, chrono::Utc::now())
                        .map_err(|_| "неверное выражение расписания".to_string())?;
                    let profile = cat
                        .get_profile_by_name(&profile_name)
                        .map_err(|e| e.to_string())?
                        .ok_or_else(|| format!("профиль «{profile_name}» не найден"))?;
                    let profile_id = profile.id.ok_or("профиль без id")?;
                    let new = mdwf_storage::NewSchedule {
                        id: None,
                        name: name.clone(),
                        profile_id,
                        reports: vec![report_type.clone()],
                        cron_expr: cron_expr.clone(),
                        period_offset,
                        params: None,
                        enabled: true,
                        next_run_at_ts: Some(next.to_rfc3339()),
                    };
                    cat.upsert_schedule(&new).map_err(|e| e.to_string())?;
                    Ok(())
                })();
                match outcome {
                    Ok(()) => log_event(
                        &domain,
                        &mdwf_core::LogOrigin::ManualGui,
                        &fwd,
                        crate::channels::LogKind::Success,
                        format!("Расписание «{name}» добавлено"),
                    ),
                    Err(e) => log_event(
                        &domain,
                        &mdwf_core::LogOrigin::ManualGui,
                        &fwd,
                        crate::channels::LogKind::Error,
                        format!("Не удалось добавить расписание: {e}"),
                    ),
                }
                fwd.forward(UiEvent::SchedulesListed(list_schedule_views(&domain)));
            }
            UiCommand::UpdateSchedule {
                id,
                name,
                cron_expr,
                period_offset,
            } => {
                let outcome: Result<(), String> = (|| {
                    let cat = domain.catalog.read();
                    let cat = cat.as_ref().ok_or("каталог недоступен")?;
                    let next = mdwf_scheduler::next_run(&cron_expr, chrono::Utc::now())
                        .map_err(|_| "неверное выражение расписания".to_string())?;
                    cat.update_schedule(
                        id,
                        &name,
                        &cron_expr,
                        period_offset,
                        Some(&next.to_rfc3339()),
                    )
                    .map_err(|e| e.to_string())?;
                    Ok(())
                })();
                match outcome {
                    Ok(()) => log_event(
                        &domain,
                        &mdwf_core::LogOrigin::ManualGui,
                        &fwd,
                        crate::channels::LogKind::Success,
                        format!("Расписание «{name}» изменено"),
                    ),
                    Err(e) => log_event(
                        &domain,
                        &mdwf_core::LogOrigin::ManualGui,
                        &fwd,
                        crate::channels::LogKind::Error,
                        format!("Не удалось изменить расписание: {e}"),
                    ),
                }
                fwd.forward(UiEvent::SchedulesListed(list_schedule_views(&domain)));
            }
            UiCommand::DeleteSchedule { name } => {
                {
                    let cat = domain.catalog.read();
                    if let Some(cat) = cat.as_ref() {
                        let _ = cat.delete_schedule(&name);
                    }
                }
                log_event(
                    &domain,
                    &mdwf_core::LogOrigin::ManualGui,
                    &fwd,
                    crate::channels::LogKind::Info,
                    format!("Расписание «{name}» удалено"),
                );
                fwd.forward(UiEvent::SchedulesListed(list_schedule_views(&domain)));
            }
            UiCommand::SetScheduleEnabled { name, enabled } => {
                {
                    let cat = domain.catalog.read();
                    if let Some(cat) = cat.as_ref() {
                        let _ = cat.set_schedule_enabled(&name, enabled);
                    }
                }
                fwd.forward(UiEvent::SchedulesListed(list_schedule_views(&domain)));
            }
            UiCommand::RunScheduleNow { name } => {
                // Выгрузка асинхронная (может занять время) — в отдельной задаче.
                let domain = domain.clone();
                let fwd = fwd.clone();
                tokio::spawn(async move {
                    run_schedule_by_name(domain, fwd, name).await;
                });
            }
            UiCommand::SetAutostart { enabled } => {
                let outcome = if enabled {
                    mdwf_scheduler::enable_autostart()
                } else {
                    mdwf_scheduler::disable_autostart()
                };
                match outcome {
                    Ok(()) => {
                        log_event(
                            &domain,
                            &mdwf_core::LogOrigin::ManualGui,
                            &fwd,
                            crate::channels::LogKind::Info,
                            format!(
                                "Автозапуск с ОС: {}",
                                if enabled { "включён" } else { "выключен" }
                            ),
                        );
                        fwd.forward(UiEvent::AutostartChanged(Ok(enabled)));
                    }
                    Err(e) => fwd.forward(UiEvent::AutostartChanged(Err(e.to_string()))),
                }
            }
            UiCommand::SetWinScheduler { enabled } => {
                let outcome = if enabled {
                    mdwf_scheduler::enable_windows_scheduler()
                } else {
                    mdwf_scheduler::disable_windows_scheduler()
                };
                match outcome {
                    Ok(()) => {
                        log_event(
                            &domain,
                            &mdwf_core::LogOrigin::ManualGui,
                            &fwd,
                            crate::channels::LogKind::Info,
                            format!(
                                "Фоновый планировщик Windows: {}",
                                if enabled { "включён" } else { "выключен" }
                            ),
                        );
                        fwd.forward(UiEvent::WinSchedulerChanged(Ok(enabled)));
                    }
                    Err(e) => fwd.forward(UiEvent::WinSchedulerChanged(Err(e.to_string()))),
                }
            }
            // ===== Журнал =====
            UiCommand::LoadJournal => {
                let result = domain.catalog.read().as_ref().map_or_else(
                    || Err("каталог недоступен".to_string()),
                    |cat| {
                        cat.list_journal(mdwf_storage::JOURNAL_KEEP as u32)
                            .map_err(|e| e.to_string())
                    },
                );
                match result {
                    Ok(rows) => {
                        // Свежие первыми (как отдаёт БД); битые created_at пропускаем.
                        // Ссылки ЛК резолвим из реестра на лету (не персистим).
                        let um = cabinet_url_map(&domain);
                        let entries: Vec<crate::channels::LogEntry> = rows
                            .into_iter()
                            .filter_map(|r| {
                                chrono::DateTime::parse_from_rfc3339(&r.created_at)
                                    .ok()
                                    .map(|dt| crate::channels::LogEntry {
                                        created_at: dt.with_timezone(&chrono::Utc),
                                        kind: crate::channels::LogKind::from_db_code(&r.kind),
                                        origin: r.origin,
                                        message: r.message,
                                        cabinet_url: if r.report_type.is_empty() {
                                            None
                                        } else {
                                            um.get(&r.report_type).cloned()
                                        },
                                        file_path: r.file_path,
                                        report_type: r.report_type,
                                    })
                            })
                            .collect();
                        fwd.forward(UiEvent::JournalLoaded(entries));
                    }
                    Err(e) => tracing::warn!(error = %e, "журнал: не удалось загрузить из БД"),
                }
            }
            UiCommand::ClearJournal => {
                if let Some(cat) = domain.catalog.read().as_ref() {
                    if let Err(e) = cat.clear_journal() {
                        tracing::warn!(error = %e, "журнал: не удалось очистить БД");
                    }
                }
                fwd.forward(UiEvent::JournalCleared);
            }
            UiCommand::LogCustom {
                kind,
                message,
                file_path,
                report_type,
            } => {
                log_event_report(
                    &domain,
                    &mdwf_core::LogOrigin::ManualGui,
                    &fwd,
                    kind,
                    message,
                    &file_path,
                    &report_type,
                );
            }
            UiCommand::CheckUpdates { manual } => {
                let result = match fetch_latest_release().await {
                    Ok((latest, url)) => {
                        let current = env!("CARGO_PKG_VERSION").to_string();
                        if version_is_newer(&latest, &current) {
                            Ok(Some(crate::channels::UpdateInfo {
                                current,
                                latest,
                                url,
                            }))
                        } else {
                            Ok(None)
                        }
                    }
                    Err(e) => Err(e),
                };
                fwd.forward(UiEvent::UpdatesChecked { manual, result });
            }
        }
    }
    warn!("command loop ended");
}

/// Последний релиз на GitHub (анонимный API — репозиторий публичный).
/// Возвращает (tag, url страницы релиза). Таймаут короткий: автопроверка
/// не должна задерживать что-либо.
async fn fetch_latest_release() -> Result<(String, String), String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("mdwf/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get("https://api.github.com/repos/bigsprut/MPDocsLoad/releases/latest")
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("сервер ответил HTTP {}", resp.status()));
    }
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let tag = json["tag_name"]
        .as_str()
        .ok_or("в ответе нет tag_name")?
        .to_string();
    let url = json["html_url"]
        .as_str()
        .unwrap_or("https://github.com/bigsprut/MPDocsLoad/releases/latest")
        .to_string();
    Ok((tag, url))
}

/// «v1.10.3» / «1.9.0» → (1, 10, 3); None — не похоже на версию.
fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let t = s.trim().trim_start_matches('v');
    let mut it = t.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next().unwrap_or("0").parse().unwrap_or(0);
    Some((major, minor, patch))
}

/// True, если `latest` строго новее `current` (_semver-тройки_).
fn version_is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        // Не похоже на версию — не считаем обновлением (защита от мусора).
        _ => false,
    }
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
            category: r.category.display_ru().to_string(),
            is_browsable: r.acquisition_mode.is_browsable(),
            provider_id: provider_id.to_string(),
            period_kind: r.period_kind,
            description: r.description.clone(),
            max_range_days: r.max_range_days,
            cabinet_path: r.cabinet_path.clone(),
            cabinet_url: r.cabinet_url.clone(),
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
        .map_err(|e| friendly_error(&e))
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
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<String>,
}

/// Перевод ошибки API (`CoreError` от вызовов провайдера — list/download) в
/// человекочитаемое сообщение с подсказкой, что делать. Вместо «HTTP 401»
/// пользователь видит причину и путь восстановления. Применяется в `do_list_*`
/// и `do_download` — покрывает и UI (notify), и Журнал (текст ошибки идёт туда же).
fn friendly_error(e: &mdwf_core::CoreError) -> String {
    if matches!(e, mdwf_core::CoreError::Cancelled) {
        return "Скачивание отменено.".into();
    }
    if e.is_auth_failure() {
        return format!(
            "{e}\n\nПричина: ключ или токен недействителен (истёк или отозван). \
             Перевыпустите его в личном кабинете маркетплейса и обновите профиль \
             во вкладке «Магазин»."
        );
    }
    if e.is_rate_limited() {
        return format!(
            "{e}\n\nПричина: превышен лимит запросов маркетплейса. Подождите 1–2 минуты и повторите."
        );
    }
    if matches!(e, mdwf_core::CoreError::Network(_)) {
        return format!(
            "{e}\n\nПричина: нет связи с маркетплейсом. Проверьте подключение к интернету и повторите."
        );
    }
    if matches!(e, mdwf_core::CoreError::Unavailable(_)) {
        // Circuit breaker: слишком много отказов подряд — клиент временно
        // перестал слать запросы, чтобы не усугублять. Пройдёт само (~5 минут).
        return format!(
            "{e}\n\nПричина: после серии ошибок запросы к маркетплейсу временно \
             приостановлены защитой от перегрузки. Подождите около 5 минут и повторите."
        );
    }
    if e.is_transient() {
        // 5xx — сбой на стороне маркетплейса (Network уже обработан выше).
        return format!("{e}\n\nПричина: временный сбой на стороне маркетплейса. Попробуйте позже.");
    }
    e.to_string()
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
    origin: &mdwf_core::LogOrigin,
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
                date: d.date.clone(),
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
        message: "начало скачивания…".into(),
    });
    let subject = journal_subject(domain, report_type, &params, !documents.is_empty());
    log_event(
        domain,
        origin,
        fwd,
        crate::channels::LogKind::Info,
        format!("Скачивание {subject} — {profile_name}"),
    );
    let files = report
        .download(auth.as_ref(), &params, progress, cancel)
        .await
        .map_err(|e| friendly_error(&e));

    fwd.forward(UiEvent::Progress {
        fraction: Some(0.9),
        message: "запись файлов на диск…".into(),
    });

    let files = match files {
        Ok(f) => f,
        Err(e) => {
            log_event(
                domain,
                origin,
                fwd,
                crate::channels::LogKind::Error,
                format!("{subject}: {e}"),
            );
            return Err(e);
        }
    };
    // Мапа display_name → serviceName (document_id) для записи document_id в каталог.
    // Нужна для значка «уже загружен»: сопоставляем DownloadedFile.source_id
    // (= display name) с DocumentSel.id (= serviceName).
    let doc_ids_by_name: std::collections::HashMap<String, String> = documents
        .iter()
        .filter_map(|d| d.name.as_ref().map(|n| (n.clone(), d.id.clone())))
        .collect();
    let saved = persist_files(
        domain,
        &files,
        provider_id,
        profile_name,
        report_type,
        &params,
        &doc_ids_by_name,
    )
    .await;

    fwd.forward(UiEvent::Progress {
        fraction: Some(1.0),
        message: "скачивание завершено".into(),
    });

    match saved {
        Ok(paths) => {
            // Особые пометки файлов (напр., отчёт собран программой — серверная
            // генерация Ozon не удалась) попадают в Журнал, чтобы было видно
            // происхождение файла.
            let note = files
                .iter()
                .find_map(|f| f.note.clone())
                .unwrap_or_default();
            let note_suffix = if note.is_empty() {
                String::new()
            } else {
                format!(" — внимание: {note}")
            };
            // Путь к файлу(ам) — в само сообщение журнала: фоновые запуски
            // расписаний видно только по журналу, путь сразу отвечает «куда».
            // Контекст (первый файл + отчёт) — для кнопок действий в Журнале.
            log_event_report(
                domain,
                origin,
                fwd,
                crate::channels::LogKind::Success,
                format!(
                    "{subject}: скачано {} файл(ов){}{}",
                    files.len(),
                    mdwf_core::journal::paths_suffix(&paths),
                    note_suffix
                ),
                paths.first().map_or("", |s| s.as_str()),
                report_type,
            );
            Ok(crate::channels::DownloadResult {
                files,
                saved_paths: paths,
            })
        }
        Err(e) => {
            log_event(
                domain,
                origin,
                fwd,
                crate::channels::LogKind::Error,
                format!("{subject}: запись на диск не удалась: {e}"),
            );
            Err(e)
        }
    }
}

/// Шлёт запись в журнал (вкладка «Журнал») с локальной меткой времени ЧЧ:ММ:СС.
/// Событие без контекста файла (запуски, ошибки, расписания).
fn log_event(
    domain: &Domain,
    origin: &mdwf_core::LogOrigin,
    fwd: &EventForwarder,
    kind: crate::channels::LogKind,
    message: impl Into<String>,
) {
    log_event_report(domain, origin, fwd, kind, message, "", "");
}

/// Запись журнала с контекстом выгрузки: `file_path` (первый файл — кнопки
/// «открыть файл/папку») и `report_type` (резолв ссылки ЛК — кнопка перехода
/// в кабинет, как в «Отчётах»/«Архиве»).
fn log_event_report(
    domain: &Domain,
    origin: &mdwf_core::LogOrigin,
    fwd: &EventForwarder,
    kind: crate::channels::LogKind,
    message: impl Into<String>,
    file_path: &str,
    report_type: &str,
) {
    let message = message.into();
    // Ссылка ЛК не персистится (меняется со временем) — резолвим на выдаче.
    let cabinet_url = if report_type.is_empty() {
        None
    } else {
        cabinet_url_map(domain).get(report_type).cloned()
    };
    // Персист: журнал переживает перезапуск (таблица `journal`, кап 500),
    // вместе с источником события (вручную/CLI/расписание).
    // Best-effort — сбой записи не должен глушить UI-событие.
    let created_at = chrono::Utc::now();
    if let Some(cat) = domain.catalog.read().as_ref() {
        if let Err(e) = cat.add_journal_entry(
            created_at,
            kind.as_str(),
            &origin.as_str(),
            &message,
            file_path,
            report_type,
        ) {
            tracing::warn!(error = %e, "журнал: не удалось записать в БД");
        }
    }
    fwd.forward(crate::channels::UiEvent::Log(crate::channels::LogEntry {
        created_at,
        kind,
        origin: origin.as_str(),
        message,
        file_path: file_path.to_string(),
        report_type: report_type.to_string(),
        cabinet_url,
    }));
}

/// Список расписаний с человекочитаемыми именами (профиль, отчёты) для UI.
fn list_schedule_views(domain: &Domain) -> Result<Vec<crate::channels::ScheduleView>, String> {
    let cat = domain.catalog.read();
    let cat = cat.as_ref().ok_or("каталог недоступен")?;
    let schedules = cat.list_schedules().map_err(|e| e.to_string())?;
    let profiles: std::collections::HashMap<i64, String> = cat
        .list_profiles()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter_map(|p| p.id.map(|id| (id, p.name)))
        .collect();
    let report_map = report_display_name_map(domain);
    Ok(schedules
        .into_iter()
        .map(|s| {
            let profile_name = profiles
                .get(&s.profile_id)
                .cloned()
                .unwrap_or_else(|| format!("#{}", s.profile_id));
            let report_names: Vec<String> = s
                .reports
                .iter()
                .map(|r| report_map.get(r).cloned().unwrap_or_else(|| r.clone()))
                .collect();
            crate::channels::ScheduleView {
                id: s.id,
                name: s.name,
                profile_id: s.profile_id,
                profile_name,
                reports: s.reports,
                report_names,
                cron_expr: s.cron_expr,
                period_offset: s.period_offset,
                enabled: s.enabled,
                next_run_at: s.next_run_at,
                last_run_at: s.last_run_at,
                last_run_status: s.last_run_status,
            }
        })
        .collect())
}

/// Исполнитель задач расписания для GUI: выгрузку делает через do_download
/// (переиспользуем — он персистит файлы + пишет в каталог + логирует в Журнал).
struct GuiJobExecutor {
    domain: Arc<Domain>,
    fwd: EventForwarder,
    /// true — запуск кнопкой «▶ Выполнить сейчас» (run_schedule_by_name);
    /// false — фоновый цикл планировщика (автозапуск по cron). Влияет на
    /// источник (origin) записей журнала.
    manual_run: bool,
}

#[async_trait::async_trait]
impl mdwf_scheduler::JobExecutor for GuiJobExecutor {
    async fn execute(
        &self,
        req: mdwf_scheduler::JobRequest,
    ) -> mdwf_core::CoreResult<mdwf_scheduler::JobResult> {
        use mdwf_scheduler::{JobResult, RunStatus};
        // Резолвим profile_id → profile (name + provider_id).
        let (profile_name, provider_id) = {
            let cat = self.domain.catalog.read();
            let cat = cat.as_ref().ok_or_else(|| {
                mdwf_core::CoreError::Internal("каталог недоступен".into())
            })?;
            let p = cat
                .list_profiles()?
                .into_iter()
                .find(|p| p.id == Some(req.profile_id))
                .ok_or_else(|| {
                    mdwf_core::CoreError::Internal(format!("профиль {} не найден", req.profile_id))
                })?;
            (p.name, p.provider_id)
        };
        let period = mdwf_scheduler::period_for_offset(req.period_offset);
        let origin = if self.manual_run {
            mdwf_core::LogOrigin::ScheduleManualRun(req.schedule_name.clone())
        } else {
            mdwf_core::LogOrigin::ScheduleGuiLoop(req.schedule_name.clone())
        };
        let mut total = 0usize;
        let mut failed = 0usize;
        for report_type in &req.reports {
            let params = mdwf_core::ReportParams {
                period: Some(period.clone()),
                ..Default::default()
            };
            match do_download(
                &self.domain,
                &provider_id,
                &profile_name,
                report_type,
                Vec::new(),
                params,
                CancellationToken::new(),
                &self.fwd,
                &origin,
            )
            .await
            {
                Ok(r) => total += r.files.len(),
                Err(_) => failed += 1,
            }
        }
        let status = if failed == 0 {
            RunStatus::Ok
        } else if total > 0 {
            RunStatus::Partial
        } else {
            RunStatus::Failed
        };
        let kind = if failed == 0 {
            crate::channels::LogKind::Success
        } else {
            crate::channels::LogKind::Error
        };
        let detail = if failed > 0 {
            format!(", ошибок: {failed}")
        } else {
            String::new()
        };
        // Период расписания — человекочитаемо (offset → месяц).
        let period_desc = mdwf_core::describe_report_period(Some(period.as_str()))
            .map_or_else(String::new, |p| format!(" ({p})"));
        log_event(
            &self.domain,
            &origin,
            &self.fwd,
            kind,
            format!(
                "Расписание «{}»{period_desc}: {} файл(ов){detail}",
                req.schedule_name, total
            ),
        );
        Ok(JobResult {
            files_count: total,
            status,
            error: None,
        })
    }
}

/// Запускает одно расписание по имени (ручной «Выполнить сейчас»): исполняет,
/// обновляет last_run/next_run, логирует, перезагружает список в UI.
async fn run_schedule_by_name(domain: Arc<Domain>, fwd: EventForwarder, name: String) {
    let schedule = {
        let cat = domain.catalog.read();
        match cat.as_ref().and_then(|c| c.get_schedule(&name).ok().flatten()) {
            Some(s) => s,
            None => {
                log_event(&domain, &mdwf_core::LogOrigin::ManualGui, &fwd, crate::channels::LogKind::Error, format!("Расписание «{name}» не найдено"));
                return;
            }
        }
    };
    let executor = GuiJobExecutor {
        domain: domain.clone(),
        fwd: fwd.clone(),
        manual_run: true,
    };
    let req = mdwf_scheduler::JobRequest {
        schedule_id: schedule.id,
        schedule_name: schedule.name.clone(),
        profile_id: schedule.profile_id,
        reports: schedule.reports.clone(),
        period_offset: schedule.period_offset,
    };
    let status = match mdwf_scheduler::JobExecutor::execute(&executor, req).await {
        Ok(r) => r.status,
        Err(_) => mdwf_scheduler::RunStatus::Failed,
    };
    // Обновим last_run/next_run в каталоге.
    if let Some(cat) = domain.catalog.read().as_ref() {
        let next = mdwf_scheduler::next_run(&schedule.cron_expr, chrono::Utc::now()).ok();
        let _ = cat.update_schedule_run(
            schedule.id,
            Some(chrono::Utc::now().to_rfc3339()),
            status.as_str(),
            next.map(|t| t.to_rfc3339()),
        );
    }
    fwd.forward(crate::channels::UiEvent::SchedulesListed(list_schedule_views(&domain)));
}

/// Карта type_id → человекочитаемое имя по всем провайдерам реестра.
/// Источник — `capabilities().reports` (статический список в памяти, без API).
/// Используется для отображения понятных названий в Архиве (вместо технических
/// type_id вроде `ozon.products`) и combo фильтра «Отчёт».
fn report_display_name_map(
    domain: &Domain,
) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for p in domain.registry.list() {
        for r in &p.capabilities().reports {
            map.insert(r.type_id.clone(), r.display_name.clone());
        }
    }
    map
}

/// Карта type_id → ссылка на раздел ЛК (кнопка «Открыть в ЛК» в Архиве).
/// Есть только у Ozon-отчётов — у WB ссылок нет (только cabinet_path),
/// поэтому WB-строки архива кнопку не получают.
fn cabinet_url_map(domain: &Domain) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for p in domain.registry.list() {
        for r in &p.capabilities().reports {
            if let Some(u) = &r.cabinet_url {
                map.insert(r.type_id.clone(), u.clone());
            }
        }
    }
    map
}

/// «Субъект» события журнала: человекочитаемое название отчёта + период
/// выгрузки (урок #54: пользователь видит имена, не type_id). Browsable-
/// документы — без периода (он к ним не относится); Month-отчёты — месяц из
/// `period` (цикл по месяцам шлёт полный диапазон в date_from/date_to, а
/// качает конкретный месяц); прочие — фактический диапазон дат с fallback
/// на `period`.
fn journal_subject(
    domain: &Domain,
    report_type: &str,
    params: &mdwf_core::ReportParams,
    has_documents: bool,
) -> String {
    let name = report_display_name_map(domain)
        .get(report_type)
        .cloned()
        .unwrap_or_else(|| report_type.to_string());
    if has_documents {
        return format!("«{name}»");
    }
    let mut kind = mdwf_core::PeriodKind::Range;
    for p in domain.registry.list() {
        if let Some(r) = p
            .capabilities()
            .reports
            .iter()
            .find(|r| r.type_id == report_type)
        {
            kind = r.period_kind;
            break;
        }
    }
    let parse = |v: Option<&String>| {
        v.and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
    };
    let period_desc = match kind {
        mdwf_core::PeriodKind::Month => {
            crate::views::describe_report_period(params.period.as_deref())
        }
        _ => crate::views::describe_range(
            parse(params.values.get("date_from")),
            parse(params.values.get("date_to")),
        )
        .or_else(|| crate::views::describe_report_period(params.period.as_deref())),
    };
    match period_desc {
        Some(p) => format!("«{name}» ({p})"),
        None => format!("«{name}»"),
    }
}

/// Записывает скачанные файлы на диск через FileStore и регистрирует в каталоге.
/// Возвращает вектор полных путей к сохранённым файлам.
///
/// `doc_ids_by_name` — мапа display_name → serviceName (document_id), для записи
/// `document_id` в каталог (значок «уже загружен»). Пуста для Period-отчётов.
async fn persist_files(
    domain: &Domain,
    files: &[mdwf_core::DownloadedFile],
    provider_id: &str,
    profile_name: &str,
    report_type: &str,
    params: &mdwf_core::ReportParams,
    doc_ids_by_name: &std::collections::HashMap<String, String>,
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
            // П.6 фикс: дата документа (WB creationTime) — для плейсхолдера {doc_date}
            // в имени файла. Раньше жёстко None → {doc_date} всегда давал «nodate».
            document_date: f.document_date.as_deref(),
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
            // document_id (serviceName) — через мапу source_id(=name) → serviceName.
            let document_id = f
                .source_id
                .as_deref()
                .and_then(|sid| doc_ids_by_name.get(sid).cloned());
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
                document_id,
                // П.6 фикс: дата документа (WB creationTime → YYYY-MM-DD) — для
                // фильтра периода Архива. None для Period-отчётов.
                document_date: f.document_date.clone(),
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

#[cfg(test)]
mod tests {
    use super::{parse_version, version_is_newer};

    #[test]
    fn parse_versions_with_prefix_and_defaults() {
        assert_eq!(parse_version("v1.10.3"), Some((1, 10, 3)));
        assert_eq!(parse_version("1.9"), Some((1, 9, 0)));
        assert_eq!(parse_version("x.y.z"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn newer_compares_numerically_not_lexically() {
        assert!(version_is_newer("v1.10.0", "1.9.9"));
        assert!(!version_is_newer("v1.6.0", "1.6.0"));
        assert!(!version_is_newer("v1.5.9", "1.6.0"));
        // Мусор — не обновление.
        assert!(!version_is_newer("latest", "1.6.0"));
    }
}
