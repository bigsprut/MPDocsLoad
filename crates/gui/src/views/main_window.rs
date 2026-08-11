//! Главное окно с боковой навигацией и стеком вкладок (спец. §2.5.2).
//!
//! Заголовок окна (header bar) показывает иконку активного маркетплейса и
//! имя продавца (из API) — обновляется по событию `ActiveShopChanged`.

use glib::clone;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Image, Label, Orientation, Separator, Stack, StackSidebar,
};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::channels::{CommandSender, UiEvent, ViewId};

/// Строит главное окно и показывает его.
pub fn build_and_present(
    app: &adw::Application,
    cs: &CommandSender,
    event_rx: async_channel::Receiver<UiEvent>,
) {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Marketplace Downloader — MDWF")
        .default_width(1100)
        .default_height(720)
        .build();

    // Корневой контейнер: [боковая навигация | разделитель | стек].
    let root = GtkBox::new(Orientation::Horizontal, 0);

    // Стек вкладок.
    let stack = Stack::new();
    stack.set_vexpand(true);
    stack.set_hexpand(true);

    // Каждая вкладка — своя вьюшка. «Магазин» — первая (источник правды выбора).
    let shop_view = crate::views::shop::build(cs);
    let reports_view = crate::views::reports::build(cs);
    let download_view = crate::views::download::build(cs);
    let archive_view = crate::views::archive::build(cs);
    let settings_view = crate::views::settings::build(cs);
    let scheduler_view = crate::views::scheduler::build(cs);
    let logs_view = crate::views::logs::build(cs);
    let about_view = crate::views::about::build();

    stack.add_titled(&shop_view, Some(ViewId::Shop.as_str()), "Магазин");
    stack.add_titled(&reports_view, Some(ViewId::Reports.as_str()), "Отчёты");
    stack.add_titled(&download_view, Some(ViewId::Download.as_str()), "Загрузка");
    stack.add_titled(&archive_view, Some(ViewId::Archive.as_str()), "Архив");
    stack.add_titled(&settings_view, Some(ViewId::Settings.as_str()), "Настройки");
    stack.add_titled(&scheduler_view, Some(ViewId::Scheduler.as_str()), "Планировщик");
    stack.add_titled(&logs_view, Some(ViewId::Logs.as_str()), "Журнал");
    stack.add_titled(&about_view, Some(ViewId::About.as_str()), "О программе");

    // Боковая навигация через StackSidebar.
    let sidebar = StackSidebar::builder()
        .stack(&stack)
        .width_request(200)
        .build();

    root.append(&sidebar);
    root.append(&Separator::new(Orientation::Vertical));
    root.append(&stack);

    // Статусбар снизу.
    let status = Label::builder()
        .label("Готово")
        .halign(gtk4::Align::Start)
        .margin_start(8)
        .margin_end(8)
        .margin_top(4)
        .margin_bottom(4)
        .css_classes(["dim-label"])
        .build();

    let bottom = GtkBox::new(Orientation::Horizontal, 0);
    bottom.set_margin_top(2);
    bottom.set_margin_bottom(2);
    bottom.append(&status);

    // --- ToolbarView: даёт полосу заголовка с кнопками управления окном ---
    // (свернуть/развернуть/закрыть). Без неё libadwaita на Windows не рисует
    // стандартные кнопки управления окном.
    let toolbar = adw::ToolbarView::new();

    // Верхняя панель заголовка.
    let header = adw::HeaderBar::builder().build();

    // Кастомный title-widget: иконка маркетплейса + имя продавца.
    // Обновляется по событию ActiveShopChanged. До выбора — плейсхолдер.
    // Иконки — PNG из gresource, грузятся напрямую через from_resource.
    let title_icon = Image::builder()
        .resource("/org/mdwf/icons/shop-placeholder.png")
        .icon_size(gtk4::IconSize::Normal)
        .margin_end(6)
        .build();
    let title_label = Label::builder()
        .label("Магазин не выбран")
        .css_classes(["heading"])
        .build();
    let title_box = GtkBox::new(Orientation::Horizontal, 0);
    title_box.append(&title_icon);
    title_box.append(&title_label);
    header.set_title_widget(Some(&title_box));

    // Меню «Приложение»: пункты «О программе» + «Выход».
    let menu = gtk4::gio::Menu::new();
    menu.append(Some("О программе"), Some("app.about"));
    menu.append(Some("Выход"), Some("app.quit"));
    let menu_btn = gtk4::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Меню")
        .menu_model(&menu)
        .build();
    header.pack_start(&menu_btn);

    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&root));
    toolbar.add_bottom_bar(&bottom);

    // Действия приложения: about и quit.
    setup_app_actions(app, &window);

    // Цикл обработки событий UI: читаем из async_channel receiver в main context.
    {
        let status = status.clone();
        let title_icon = title_icon.clone();
        let title_label = title_label.clone();
        let main_ctx = glib::MainContext::default();
        main_ctx.spawn_local(clone!(@strong event_rx => async move {
            // Порождаем таск, читающий receiver; обновляем UI по событию.
            loop {
                match event_rx.recv().await {
                    Ok(event) => dispatch_event(&event, &status, &title_icon, &title_label),
                    Err(async_channel::RecvError) => break,
                }
            }
        }));
    }

    window.set_content(Some(&toolbar));
    window.present();
}

/// Регистрирует действия приложения `app.about` и `app.quit`.
fn setup_app_actions(app: &adw::Application, window: &adw::ApplicationWindow) {
    // Действие «quit»: закрывает окно и завершает приложение.
    let quit_action = gtk4::gio::SimpleAction::new("quit", None);
    {
        let win = window.clone();
        quit_action.connect_activate(move |_, _| {
            win.close();
        });
    }
    app.add_action(&quit_action);
    // Горячая клавиша Ctrl+Q.
    app.set_accels_for_action("app.quit", &["<Ctrl>Q"]);

    // Действие «about»: показываем вкладку «О программе».
    let about_action = gtk4::gio::SimpleAction::new("about", None);
    about_action.connect_activate({
        let win = window.clone();
        move |_, _| {
            // Переключаем стек на «about» — ищем его в content.
            show_about_in_window(&win);
        }
    });
    app.add_action(&about_action);
}

/// Находит стек в окне и переключает на вкладку «О программе».
fn show_about_in_window(win: &adw::ApplicationWindow) {
    // Идём от content (ToolbarView) -> content (GtkBox) -> stack.
    let Some(content) = win.content() else {
        return;
    };
    // Рекурсивный поиск Stack среди детей.
    if let Some(stack) = find_stack(&content) {
        stack.set_visible_child_name(ViewId::About.as_str());
    }
}

/// Рекурсивный поиск первого виджета типа gtk4::Stack в дереве.
fn find_stack(widget: &gtk4::Widget) -> Option<gtk4::Stack> {
    if let Ok(stack) = widget.clone().downcast::<gtk4::Stack>() {
        return Some(stack);
    }
    if let Some(bin) = widget.downcast_ref::<gtk4::Box>() {
        let mut child = bin.first_child();
        while let Some(c) = child {
            if let Some(found) = find_stack(&c) {
                return Some(found);
            }
            child = c.next_sibling();
        }
    }
    None
}

/// Маршрутизация событий UI в нужные обработчики.
fn dispatch_event(
    event: &UiEvent,
    status: &Label,
    title_icon: &Image,
    title_label: &Label,
) {
    match event {
        UiEvent::Notify(msg) => {
            status.set_text(msg);
        }
        UiEvent::DownloadStateLoaded(state) => {
            crate::views::download::on_download_state_loaded(state.as_ref());
        }
        UiEvent::DownloadsListed { report_type, docs } => {
            crate::views::download::on_downloads_listed(report_type, docs.clone());
        }
        UiEvent::ArchiveReportTypesLoaded(rts) => {
            crate::views::archive::on_report_types_loaded(rts);
        }
        UiEvent::ArchiveListed(res) => {
            crate::views::archive::on_archive_listed(res);
            match res {
                Ok(d) => status.set_text(&format!("Записей в архиве: {}", d.len())),
                Err(e) => status.set_text(&format!("Ошибка: {e}")),
            }
        }
        UiEvent::ArchiveStateLoaded(state) => {
            crate::views::archive::on_archive_state_loaded(state.as_ref());
        }
        UiEvent::DownloadDeleted(res) => {
            crate::views::archive::on_download_deleted(res);
            match res {
                Ok(_) => status.set_text("Запись удалена"),
                Err(e) => status.set_text(&format!("Ошибка удаления: {e}")),
            }
        }
        UiEvent::Progress { message, .. } => {
            status.set_text(message);
        }
        UiEvent::ProvidersLoaded(list) => {
            status.set_text(&format!("Провайдеров: {}", list.len()));
            crate::views::shop::on_providers_loaded(list);
        }
        UiEvent::ProfilesLoaded(list) => {
            status.set_text(&format!("Профилей: {}", list.len()));
            crate::views::shop::on_profiles_loaded(list);
            crate::views::archive::on_profiles_loaded(list);
        }
        UiEvent::AuthFieldsLoaded { provider_id, fields } => {
            crate::views::shop::on_auth_fields_loaded(provider_id, fields);
        }
        UiEvent::ActiveShopLoaded(shop) => {
            crate::views::shop::on_active_shop_loaded(shop.as_ref());
        }
        UiEvent::ActiveShopChanged {
            provider_id,
            provider_display_name,
            seller_name,
            profile_name,
        } => {
            // Обновляем заголовок окна: иконка + имя.
            update_title(
                title_icon,
                title_label,
                provider_id,
                provider_display_name,
                seller_name.as_deref(),
                profile_name,
            );
            // Оповещаем зависимые вкладки (Загрузка, Отчёты).
            crate::views::download::on_active_shop_changed(
                provider_id,
                provider_display_name,
                seller_name.as_deref(),
                profile_name,
            );
            crate::views::reports::on_active_shop_changed(
                provider_id,
                provider_display_name,
                seller_name.as_deref(),
                profile_name,
            );
            // Локальный статус в shop-вкладке.
            crate::views::shop::on_active_shop_changed(
                provider_id,
                provider_display_name,
                seller_name.as_deref(),
                profile_name,
            );
        }
        UiEvent::ReportsLoaded(res) => {
            match res {
                Ok(r) => {
                    status.set_text(&format!("Отчётов: {}", r.len()));
                    crate::views::reports::on_reports_loaded(&Ok(r.clone()));
                    crate::views::download::on_reports_loaded(r);
                }
                Err(e) => {
                    status.set_text(&format!("Ошибка: {e}"));
                    crate::views::reports::on_reports_loaded(&Err(e.clone()));
                }
            }
        }
        UiEvent::DocumentsListed(res) => {
            crate::views::download::on_documents_listed(res);
            match res {
                Ok(d) => status.set_text(&format!("Документов: {}", d.len())),
                Err(e) => status.set_text(&format!("Ошибка: {e}")),
            }
        }
        UiEvent::DocumentCategoriesLoaded(res) => {
            crate::views::download::on_document_categories_loaded(res);
        }
        UiEvent::DownloadFinished(res) => match res {
            Ok(result) => {
                status.set_text(&format!("Скачано файлов: {}", result.files.len()));
                crate::views::download::on_download_finished(result);
            }
            Err(e) => {
                status.set_text(&format!("Ошибка выгрузки: {e}"));
                crate::views::download::on_download_error(e);
            }
        },
        UiEvent::ProfileSaved(res) => match res {
            Ok(id) => status.set_text(&format!("Профиль сохранён (id={id})")),
            Err(e) => status.set_text(&format!("Ошибка: {e}")),
        },
        UiEvent::ProfileDeleted(res) => match res {
            Ok(()) => status.set_text("Профиль удалён"),
            Err(e) => status.set_text(&format!("Ошибка: {e}")),
        },
        UiEvent::ProfileChecked(res) => match res {
            Ok(hs) => status.set_text(&format!("Health: {} ({})", level_str(&hs.level), hs.message)),
            Err(e) => status.set_text(&format!("Ошибка: {e}")),
        },
    }
}

/// Обновляет иконку и текст заголовка окна по активному магазину.
fn update_title(
    title_icon: &Image,
    title_label: &Label,
    provider_id: &str,
    provider_display_name: &str,
    seller_name: Option<&str>,
    profile_name: &str,
) {
    // Иконка-маркер по provider_id. PNG из gresource, грузится напрямую через
    // from_resource (без IconTheme — надёжнее на Windows). Fallback — плейсхолдер.
    let icon_path = match provider_id {
        "ozon" => "/org/mdwf/icons/ozon.png",
        "wildberries" => "/org/mdwf/icons/wildberries.png",
        "test" => "/org/mdwf/icons/test.png",
        _ => "/org/mdwf/icons/shop-placeholder.png",
    };
    title_icon.set_resource(Some(icon_path));

    // Текст: «Маркетплейс — ИмяПродавца» или «Маркетплейс — Профиль» (fallback).
    let display = seller_name.unwrap_or(profile_name);
    title_label.set_text(&format!("{provider_display_name} — {display}"));
}

fn level_str(level: &mdwf_core::HealthLevel) -> &'static str {
    match level {
        mdwf_core::HealthLevel::Ok => "OK",
        mdwf_core::HealthLevel::Degraded => "Degraded",
        mdwf_core::HealthLevel::Down => "Down",
    }
}
