//! Вкладка «Профили»: список + создание/редактирование/удаление.

use std::cell::RefCell;
use std::rc::Rc;

use glib::clone;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, ComboBoxText, Entry, Grid, Label, ListView, Orientation,
    PolicyType, ScrolledWindow, SingleSelection, StringList,
};
use libadwaita as adw;
use libadwaita::prelude::MessageDialogExt;

use mdwf_core::Profile;

use crate::channels::CommandSender;

/// Глобальное (на время жизни приложения) хранилище профилей для UI.
thread_local! {
    static PROFILES: Rc<RefCell<Vec<Profile>>> = Rc::new(RefCell::new(Vec::new()));
}

/// Строит вкладку профилей.
pub fn build(cs: &CommandSender) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.set_margin_top(16);
    root.set_margin_bottom(16);

    let header = GtkBox::new(Orientation::Horizontal, 8);
    let title = Label::builder()
        .label("Профили учётных данных")
        .css_classes(["title-2"])
        .build();
    title.set_hexpand(true);
    title.set_halign(gtk4::Align::Start);

    let add_btn = Button::builder().label("＋ Добавить").css_classes(["suggested-action"]).build();
    let del_btn = Button::builder().label("🗑 Удалить").build();
    header.append(&title);
    header.append(&add_btn);
    header.append(&del_btn);
    root.append(&header);

    // Описание.
    root.append(&Label::builder()
        .label("Профиль = один продавец на одном маркетплейсе. Секреты хранятся в OS keychain.")
        .css_classes(["dim-label"])
        .halign(gtk4::Align::Start)
        .build());

    // Список профилей (простой StringList + ListView).
    let model = StringList::new(&[]);
    let selection = SingleSelection::new(Some(model.clone()));
    let list = ListView::new(Some(selection.clone()), Some(gtk4::SignalListItemFactory::new()));

    // Фабрика ячеек.
    let factory = gtk4::SignalListItemFactory::new();
    factory.connect_setup(clone!(@weak model => move |_factory, item| {
        let item = item.downcast_ref::<gtk4::ListItem>().expect("ListItem");
        let label = Label::new(None);
        label.set_halign(gtk4::Align::Start);
        item.set_child(Some(&label));
        let idx = item.position();
        if let Some(text) = model.string(idx) {
            if let Some(label) = item.child().and_then(|c| c.downcast::<Label>().ok()) {
                label.set_text(&text);
            }
        }
    }));
    list.set_factory(Some(&factory));

    let scroll = ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(PolicyType::Never)
        .vexpand(true)
        .build();
    root.append(&scroll);

    // Кнопка «Добавить» открывает упрощённый диалог (имя + провайдер + секрет).
    let cs2 = cs.clone();
    let model_clone = model.clone();
    add_btn.connect_clicked(move |_| {
        show_add_dialog(&cs2, &model_clone);
    });

    // Кнопка «Удалить».
    let cs3 = cs.clone();
    let selection_clone = selection.clone();
    let model_clone2 = model.clone();
    del_btn.connect_clicked(move |_| {
        let idx = selection_clone.selected();
        if let Some(name) = model_clone2.string(idx) {
            let name = name.to_string();
            cs3.send(crate::channels::UiCommand::DeleteProfile(name));
        }
    });

    root
}

/// Простой диалог добавления профиля (для быстрого старта без реальных провайдеров).
fn show_add_dialog(cs: &CommandSender, model: &StringList) {
    let dialog = adw::MessageDialog::builder()
        .heading("Новый профиль")
        .body("Введите данные профиля")
        .build();

    // Поля.
    let grid = Grid::new();
    grid.set_column_spacing(8);
    grid.set_row_spacing(8);
    grid.set_margin_start(12);
    grid.set_margin_end(12);
    grid.set_margin_top(12);
    grid.set_margin_bottom(12);

    let name_entry = Entry::builder().placeholder_text("Ozon-1").build();
    let provider_combo = ComboBoxText::new();
    provider_combo.append_text("test");
    provider_combo.append_text("ozon");
    provider_combo.append_text("wildberries");
    provider_combo.set_active(Some(0));

    let secret_entry = Entry::builder()
        .placeholder_text("токен/ключ")
        .visibility(false)
        .build();

    grid.attach(&Label::new(Some("Имя:")), 0, 0, 1, 1);
    grid.attach(&name_entry, 1, 0, 1, 1);
    grid.attach(&Label::new(Some("Провайдер:")), 0, 1, 1, 1);
    grid.attach(&provider_combo, 1, 1, 1, 1);
    grid.attach(&Label::new(Some("Секрет:")), 0, 2, 1, 1);
    grid.attach(&secret_entry, 1, 2, 1, 1);

    dialog.set_extra_child(Some(&grid));

    dialog.add_response("cancel", "Отмена");
    dialog.add_response("ok", "Сохранить");
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);

    let cs = cs.clone();
    let model = model.clone();
    let dialog_for_close = dialog.clone();
    dialog.connect_response(None, move |_dlg: &adw::MessageDialog, response: &str| {
        if response != "ok" {
            return;
        }
        let name = name_entry.text().to_string();
        if name.is_empty() {
            return;
        }
        let provider_id = provider_combo
            .active_text().map_or_else(|| "test".into(), |s| s.to_string());
        let mut profile = Profile::new(&name, &provider_id);
        // Секрет пока кладём в metadata (на этапе настроек переключим на keychain).
        let secret = secret_entry.text().to_string();
        if !secret.is_empty() {
            profile.auth_metadata.insert("secret".into(), secret);
        }
        // Локальное обновление модели.
        model.append(&format!("{name} [{provider_id}]"));
        cs.send(crate::channels::UiCommand::SaveProfile(profile));
        dialog_for_close.close();
    });

    dialog.present();
}

/// Обработчик события «профили загружены» — обновляет локальное состояние.
pub fn on_profiles_loaded(profiles: &[Profile]) {
    PROFILES.with(|p| {
        *p.borrow_mut() = profiles.to_vec();
    });
}
