//! Вкладка «Профили»: список + создание/редактирование/удаление.
//!
//! Форма добавления строится ДИНАМИЧЕСКИ из `AuthField[]` выбранного провайдера
//! (спец. §2.5.3), чтобы секрет сохранялся под правильным ключом
//! (`token` для WB, `client_id`+`api_key` для Ozon).

use std::cell::RefCell;
use std::rc::Rc;

use glib::clone;
use gtk4::prelude::*;
use gtk4::{
    gio, Box as GtkBox, Button, ComboBoxText, Entry, Grid, Label, ListItem, ListView, Orientation,
    PolicyType, ScrolledWindow, SignalListItemFactory, SingleSelection, StringObject,
};
use libadwaita as adw;
use libadwaita::prelude::MessageDialogExt;

use mdwf_core::Profile;

use crate::channels::{
    AuthFieldInfo, AuthFieldKindInfo, CommandSender, UiCommand, UiEvent,
};

/// Запись в списке профилей (для отображения).
#[derive(Clone)]
struct ProfileRow {
    name: String,
    provider_id: String,
}

/// Состояние вкладки профилей (живёт в thread_local, как и виджеты).
struct ProfilesState {
    /// Модель списка (Gio.ListStore<StringObject>).
    store: gio::ListStore,
    /// Текущие профили (параллельно store для поиска по имени).
    rows: Vec<ProfileRow>,
    /// Выбранный индекс.
    selected: usize,
    /// Текущий набор полей выбранного провайдера (для диалога добавления).
    pending_fields: Vec<AuthFieldInfo>,
}

impl ProfilesState {
    fn new() -> Self {
        Self {
            store: gio::ListStore::new::<StringObject>(),
            rows: Vec::new(),
            selected: 0,
            pending_fields: Vec::new(),
        }
    }

    fn clear(&mut self) {
        self.store.remove_all();
        self.rows.clear();
        self.selected = 0;
    }

    fn set_profiles(&mut self, profiles: &[Profile]) {
        self.clear();
        for p in profiles {
            let text = format!("{}  [{}]", p.name, p.provider_id);
            self.store.append(&StringObject::new(&text));
            self.rows.push(ProfileRow {
                name: p.name.clone(),
                provider_id: p.provider_id.clone(),
            });
        }
    }

    fn append_local(&mut self, name: &str, provider_id: &str) {
        let text = format!("{name}  [{provider_id}]");
        self.store.append(&StringObject::new(&text));
        self.rows.push(ProfileRow {
            name: name.to_string(),
            provider_id: provider_id.to_string(),
        });
    }

    fn name_at(&self, idx: u32) -> Option<&str> {
        self.rows.get(idx as usize).map(|r| r.name.as_str())
    }
}

thread_local! {
    static STATE: Rc<RefCell<ProfilesState>> = Rc::new(RefCell::new(ProfilesState::new()));
    /// Один «активный» диалог добавления (чтобы получать его поля по событию).
    static ACTIVE_DIALOG: Rc<RefCell<Option<Rc<AddDialogState>>>> = Rc::new(RefCell::new(None));
}

/// Состояние активного диалога добавления (виджеты полей + выбранный провайдер).
struct AddDialogState {
    dialog: adw::MessageDialog,
    name_entry: Entry,
    provider_combo: ComboBoxText,
    /// Динамически созданные поля: (field_info, entry_or_combo).
    fields: RefCell<Vec<(AuthFieldInfo, Entry)>>,
    grid: Grid,
    cs: CommandSender,
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

    let add_btn = Button::builder()
        .label("＋ Добавить")
        .css_classes(["suggested-action"])
        .build();
    let del_btn = Button::builder().label("🗑 Удалить").build();
    let check_btn = Button::builder().label("✓ Проверить").build();
    header.append(&title);
    header.append(&check_btn);
    header.append(&add_btn);
    header.append(&del_btn);
    root.append(&header);

    root.append(
        &Label::builder()
            .label("Профиль = один продавец на одном маркетплейсе. Секреты хранятся в OS keychain.")
            .css_classes(["dim-label"])
            .halign(gtk4::Align::Start)
            .build(),
    );

    // Список через Gio.ListStore (живая модель).
    let store = STATE.with(|s| s.borrow().store.clone());
    let selection = SingleSelection::new(Some(store.clone()));
    selection.set_autoselect(false);
    let list = ListView::new(Some(selection.clone()), None::<SignalListItemFactory>);

    // Фабрика ячеек: рисует текст из StringObject.
    let factory = SignalListItemFactory::new();
    factory.connect_setup(move |_f, item| {
        let item = item.downcast_ref::<ListItem>().expect("ListItem");
        let label = Label::new(None);
        label.set_halign(gtk4::Align::Start);
        label.set_margin_start(8);
        label.set_margin_end(8);
        label.set_margin_top(4);
        label.set_margin_bottom(4);
        item.set_child(Some(&label));
    });
    factory.connect_bind(move |_f, item| {
        let item = item.downcast_ref::<ListItem>().expect("ListItem");
        let Some(string_obj) = item.item().and_then(|o| o.downcast::<StringObject>().ok()) else {
            return;
        };
        if let Some(label) = item.child().and_then(|c| c.downcast::<Label>().ok()) {
            label.set_text(&string_obj.string());
        }
    });
    list.set_factory(Some(&factory));

    let scroll = ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(PolicyType::Never)
        .vexpand(true)
        .build();
    root.append(&scroll);

    // Отслеживание выбора.
    selection.connect_selection_changed(clone!(@weak selection => move |_, _, _| {
        let idx = selection.selected();
        STATE.with(|s| s.borrow_mut().selected = idx as usize);
    }));

    // Кнопка «Добавить».
    let cs_add = cs.clone();
    add_btn.connect_clicked(move |_| {
        show_add_dialog(&cs_add);
    });

    // Кнопка «Удалить».
    let cs_del = cs.clone();
    del_btn.connect_clicked(move |_| {
        let idx = STATE.with(|s| s.borrow().selected as u32);
        let name = STATE.with(|s| s.borrow().name_at(idx).map(str::to_string));
        if let Some(name) = name {
            cs_del.send(UiCommand::DeleteProfile(name));
        }
    });

    // Кнопка «Проверить».
    let cs_chk = cs.clone();
    check_btn.connect_clicked(move |_| {
        let idx = STATE.with(|s| s.borrow().selected as u32);
        let name = STATE.with(|s| s.borrow().name_at(idx).map(str::to_string));
        if let Some(name) = name {
            cs_chk.send(UiCommand::CheckProfile(name));
        }
    });

    root
}

/// Открывает диалог добавления профиля.
fn show_add_dialog(cs: &CommandSender) {
    let dialog = adw::MessageDialog::builder()
        .heading("Новый профиль")
        .body("Выберите маркетплейс и заполните данные доступа")
        .build();

    let name_entry = Entry::builder().placeholder_text("Например: WB-основной").build();
    let provider_combo = ComboBoxText::new();
    provider_combo.append_text("ozon");
    provider_combo.append_text("wildberries");
    provider_combo.append_text("test");
    provider_combo.set_active(Some(0));

    let grid = Grid::new();
    grid.set_column_spacing(8);
    grid.set_row_spacing(8);
    grid.set_margin_start(12);
    grid.set_margin_end(12);
    grid.set_margin_top(12);
    grid.set_margin_bottom(12);
    grid.attach(&Label::new(Some("Имя профиля:")), 0, 0, 1, 1);
    grid.attach(&name_entry, 1, 0, 1, 1);
    grid.attach(&Label::new(Some("Маркетплейс:")), 0, 1, 1, 1);
    grid.attach(&provider_combo, 1, 1, 1, 1);

    dialog.set_extra_child(Some(&grid));

    dialog.add_response("cancel", "Отмена");
    dialog.add_response("ok", "Сохранить");
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);

    let state = Rc::new(AddDialogState {
        dialog: dialog.clone(),
        name_entry: name_entry.clone(),
        provider_combo: provider_combo.clone(),
        fields: RefCell::new(Vec::new()),
        grid: grid.clone(),
        cs: cs.clone(),
    });

    // Запоминаем активный диалог.
    ACTIVE_DIALOG.with(|cell| *cell.borrow_mut() = Some(state.clone()));

    // При смене провайдера — запрашиваем его поля авторизации.
    let cs_fields = cs.clone();
    provider_combo.connect_changed(move |combo| {
        if let Some(pid) = combo.active_text() {
            cs_fields.send(UiCommand::LoadAuthFields(pid.to_string()));
        }
    });
    // Запрос для изначально выбранного провайдера.
    if let Some(pid) = provider_combo.active_text() {
        cs.send(UiCommand::LoadAuthFields(pid.to_string()));
    }

    // Обработчик ответа.
    let state_for_close = state.clone();
    dialog.connect_response(None, move |_dlg, response: &str| {
        if response == "ok" {
            save_profile_from_dialog(&state_for_close);
        } else {
            // Очищаем активный диалог при отмене.
            ACTIVE_DIALOG.with(|cell| {
                if cell
                    .borrow()
                    .as_ref()
                    .is_some_and(|s| Rc::ptr_eq(s, &state_for_close))
                {
                    *cell.borrow_mut() = None;
                }
            });
        }
        state_for_close.dialog.close();
    });

    dialog.present();
}

/// Сохраняет профиль из диалога: имя + провайдер + значения динамических полей.
fn save_profile_from_dialog(state: &Rc<AddDialogState>) {
    let name = state.name_entry.text().to_string();
    let provider_id = state
        .provider_combo
        .active_text().map_or_else(|| "test".into(), |s| s.to_string());

    if name.is_empty() {
        return;
    }

    let mut profile = Profile::new(&name, &provider_id);
    for (field, entry) in state.fields.borrow().iter() {
        let value = entry.text().to_string();
        if !value.is_empty() {
            profile.auth_metadata.insert(field.id.clone(), value);
        }
    }

    // Локально добавляем в список (финальную синхронизацию сделает ProfileSaved event).
    STATE.with(|s| s.borrow_mut().append_local(&name, &provider_id));
    state.cs.send(UiCommand::SaveProfile(profile));

    // Очищаем активный диалог.
    ACTIVE_DIALOG.with(|cell| {
        if cell
            .borrow()
            .as_ref()
            .is_some_and(|s| Rc::ptr_eq(s, state))
        {
            *cell.borrow_mut() = None;
        }
    });
}

/// Обработчик события «поля авторизации загружены» — перерисовывает поля диалога.
pub fn on_auth_fields_loaded(provider_id: &str, fields: &[AuthFieldInfo]) {
    ACTIVE_DIALOG.with(|cell| {
        let state = cell.borrow().clone();
        let Some(state) = state else { return };
        // Проверяем, что событие относится к текущему выбору провайдера в диалоге.
        let current = state
            .provider_combo
            .active_text()
            .map(|s| s.to_string())
            .unwrap_or_default();
        if current != provider_id {
            return;
        }
        render_dialog_fields(&state, fields);
    });
}

/// Перерисовывает динамические поля в диалоге.
fn render_dialog_fields(state: &Rc<AddDialogState>, fields: &[AuthFieldInfo]) {
    // Удаляем старые поля (строки 2..) из grid.
    // Проще: убираем всех детей grid и перерисовываем заново.
    let mut child = state.grid.first_child();
    while let Some(c) = child {
        let next = c.next_sibling();
        state.grid.remove(&c);
        child = next;
    }
    state.fields.borrow_mut().clear();

    // Восстанавливаем шапку (имя, провайдер).
    state.grid.attach(&Label::new(Some("Имя профиля:")), 0, 0, 1, 1);
    state.grid.attach(&state.name_entry, 1, 0, 1, 1);
    state.grid.attach(&Label::new(Some("Маркетплейс:")), 0, 1, 1, 1);
    state.grid.attach(&state.provider_combo, 1, 1, 1, 1);

    // Добавляем поля провайдера.
    for (row, field) in fields.iter().enumerate() {
        let r = (row + 2) as i32;
        let label_text = if field.required {
            format!("{} *", field.label)
        } else {
            field.label.clone()
        };
        let label = Label::new(Some(&label_text));
        label.set_halign(gtk4::Align::Start);
        label.set_tooltip_text(field.help_text.as_deref());
        state.grid.attach(&label, 0, r, 1, 1);

        let entry = match &field.kind {
            AuthFieldKindInfo::Password => {
                let e = Entry::builder().visibility(false).build();
                if let Some(p) = &field.placeholder {
                    e.set_placeholder_text(Some(p));
                }
                e
            }
            AuthFieldKindInfo::Number => {
                let e = Entry::builder().build();
                e.set_input_purpose(gtk4::InputPurpose::Number);
                if let Some(p) = &field.placeholder {
                    e.set_placeholder_text(Some(p));
                }
                e
            }
            AuthFieldKindInfo::Select(_opts) => {
                // Упрощённо: для select используем обычный Entry (текст).
                // Полноценная отрисовка combo — в следующей итерации.
                let e = Entry::builder().build();
                e.set_placeholder_text(Some("введите значение"));
                e
            }
            AuthFieldKindInfo::Text => {
                let e = Entry::builder().build();
                if let Some(p) = &field.placeholder {
                    e.set_placeholder_text(Some(p));
                }
                e
            }
        };
        state.grid.attach(&entry, 1, r, 1, 1);
        state.fields.borrow_mut().push((field.clone(), entry));
    }
    state.grid.show();
}

/// Обработчик события «профили загружены» — полностью перерисовывает список.
pub fn on_profiles_loaded(profiles: &[Profile]) {
    STATE.with(|s| s.borrow_mut().set_profiles(profiles));
}
