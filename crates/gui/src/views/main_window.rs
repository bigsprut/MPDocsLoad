//! Главное окно с боковой навигацией и стеком вкладок (спец. §2.5.2).

use glib::clone;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Label, Orientation, Separator, Stack, StackSidebar,
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

    // Каждая вкладка — своя вьюшка.
    let profiles_view = crate::views::profiles::build(cs);
    let reports_view = crate::views::reports::build(cs);
    let download_view = crate::views::download::build(cs);
    let settings_view = crate::views::settings::build(cs);
    let scheduler_view = crate::views::scheduler::build(cs);
    let logs_view = crate::views::logs::build(cs);
    let about_view = crate::views::about::build();

    stack.add_titled(&profiles_view, Some(ViewId::Profiles.as_str()), "Профили");
    stack.add_titled(&reports_view, Some(ViewId::Reports.as_str()), "Отчёты");
    stack.add_titled(&download_view, Some(ViewId::Download.as_str()), "Загрузка");
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
    bottom.append(&status);

    let outer = GtkBox::new(Orientation::Vertical, 0);
    outer.set_vexpand(true);
    outer.set_hexpand(true);
    outer.append(&root);
    outer.append(&Separator::new(Orientation::Horizontal));
    outer.append(&bottom);

    // Цикл обработки событий UI: читаем из async_channel receiver в main context.
    {
        let status = status.clone();
        let main_ctx = glib::MainContext::default();
        main_ctx.spawn_local(clone!(@strong event_rx => async move {
            // Порождаем таск, читающий receiver; обновляем UI по событию.
            loop {
                match event_rx.recv().await {
                    Ok(event) => dispatch_event(&event, &status),
                    Err(async_channel::RecvError) => break,
                }
            }
        }));
    }

    window.set_content(Some(&outer));
    window.present();
}

/// Маршрутизация событий UI в нужные обработчики.
fn dispatch_event(event: &UiEvent, status: &Label) {
    match event {
        UiEvent::Notify(msg) => {
            status.set_text(msg);
        }
        UiEvent::Progress { message, .. } => {
            status.set_text(message);
        }
        UiEvent::ProvidersLoaded(list) => {
            status.set_text(&format!("Провайдеров: {}", list.len()));
            crate::views::reports::on_providers_loaded(list);
            crate::views::download::on_providers_loaded(list);
        }
        UiEvent::ProfilesLoaded(list) => {
            status.set_text(&format!("Профилей: {}", list.len()));
            crate::views::profiles::on_profiles_loaded(list);
            crate::views::download::on_profiles_loaded(list);
        }
        UiEvent::ReportsLoaded(res) => {
            crate::views::reports::on_reports_loaded(res);
            match res {
                Ok(r) => status.set_text(&format!("Отчётов: {}", r.len())),
                Err(e) => status.set_text(&format!("Ошибка: {e}")),
            }
        }
        UiEvent::DocumentsListed(res) => {
            crate::views::download::on_documents_listed(res);
            match res {
                Ok(d) => status.set_text(&format!("Документов: {}", d.len())),
                Err(e) => status.set_text(&format!("Ошибка: {e}")),
            }
        }
        UiEvent::DownloadFinished(res) => match res {
            Ok(files) => {
                status.set_text(&format!("Скачано файлов: {}", files.len()));
                crate::views::download::on_download_finished(files);
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

fn level_str(level: &mdwf_core::HealthLevel) -> &'static str {
    match level {
        mdwf_core::HealthLevel::Ok => "OK",
        mdwf_core::HealthLevel::Degraded => "Degraded",
        mdwf_core::HealthLevel::Down => "Down",
    }
}
