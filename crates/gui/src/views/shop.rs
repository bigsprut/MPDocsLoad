//! Вкладка «Магазин» — единый источник правды выбора маркетплейса+профиля.
//!
//! Объединяет две функции (заменила отдельную вкладку «Профили»):
//! 1. **Активный магазин**: combo маркетплейса + combo профиля (фильтр по
//!    провайдеру). При смене профиля — `UiCommand::SelectShop` → persist в
//!    `ui_state` (ключ `"active_shop"`) + запрос имени продавца из API для
//!    заголовка окна. Все остальные вкладки (Загрузка, Отчёты) читают выбор
//!    отсюда, а не из собственных combo.
//! 2. **CRUD профилей**: список + Добавить/Удалить/Проверить (перенос из
//!    profiles.rs). Диалог добавления строится динамически из `AuthField[]`
//!    выбранного провайдера; список маркетплейсов в диалоге наполняется из
//!    реестра (без хардкода).

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
    ActiveShop, AuthFieldInfo, AuthFieldKindInfo, CommandSender, ProviderInfo, UiCommand,
};

// ===== thread_local состояние =====

thread_local! {
    /// Все провайдеры из реестра: (id, display_name).
    static PROVIDERS: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(Vec::new()));
    /// Все профили из БД.
    static PROFILES: Rc<RefCell<Vec<Profile>>> = Rc::new(RefCell::new(Vec::new()));
    /// Текущий активный магазин (восстанавливается из ui_state при старте,
    /// обновляется при смене выбора). Provider_id + profile_name.
    static ACTIVE_SHOP: Rc<RefCell<Option<ActiveShop>>> = Rc::new(RefCell::new(None));
    // Виджеты выбора магазина (сохраняем для обновления из событий).
    static W_PROVIDER: Rc<RefCell<Option<ComboBoxText>>> = Rc::new(RefCell::new(None));
    static W_PROFILE: Rc<RefCell<Option<ComboBoxText>>> = Rc::new(RefCell::new(None));
    /// Лейбл статуса активного магазина (имя продавца / подключение).
    static W_SHOP_STATUS: Rc<RefCell<Option<Label>>> = Rc::new(RefCell::new(None));
    /// Командный канал (для авто-запросов при смене выбора).
    static CMD: Rc<RefCell<Option<CommandSender>>> = Rc::new(RefCell::new(None));
    // --- CRUD профилей ---
    static STATE: Rc<RefCell<ProfilesState>> = Rc::new(RefCell::new(ProfilesState::new()));
    /// Empty-state: призыв создать первый профиль (когда профилей нет).
    static W_EMPTY: Rc<RefCell<Option<Label>>> = Rc::new(RefCell::new(None));
    /// Один «активный» диалог добавления (чтобы получать его поля по событию).
    static ACTIVE_DIALOG: Rc<RefCell<Option<Rc<AddDialogState>>>> = Rc::new(RefCell::new(None));
    /// Профиль в режиме изменения (предзаполнение полей + сохранение с id).
    static EDIT_TARGET: Rc<RefCell<Option<Profile>>> = Rc::new(RefCell::new(None));
    /// Список провайдеров для диалога добавления (передаётся из on_providers_loaded).
    static DIALOG_PROVIDERS: Rc<RefCell<Vec<ProviderInfo>>> = Rc::new(RefCell::new(Vec::new()));
}

// ===== Хуки (вызываются из dispatch_event) =====

/// Хук: провайдеры загружены → наполняем combo маркетплейсов.
pub fn on_providers_loaded(providers: &[ProviderInfo]) {
    PROVIDERS.with(|p| {
        *p.borrow_mut() = providers
            .iter()
            .map(|pr| (pr.id.clone(), pr.display_name.clone()))
            .collect();
    });
    DIALOG_PROVIDERS.with(|d| *d.borrow_mut() = providers.to_vec());
    W_PROVIDER.with(|w| {
        if let Some(combo) = w.borrow().as_ref() {
            combo.remove_all();
            for pr in providers {
                // display_name как видимый текст, id — как значение (active_id).
                combo.append(Some(pr.id.as_str()), pr.display_name.as_str());
            }
            combo.set_active(Some(0));
        }
    });
}

/// Хук: профили загружены → обновляем CRUD-список и combo профиля магазина.
pub fn on_profiles_loaded(profiles: &[Profile]) {
    PROFILES.with(|p| *p.borrow_mut() = profiles.to_vec());
    STATE.with(|s| s.borrow_mut().set_profiles(profiles));
    refresh_profile_combo();
    // Empty-state: показываем призыв, пока профилей нет.
    let empty = profiles.is_empty();
    W_EMPTY.with(|w| {
        if let Some(lbl) = w.borrow().as_ref() {
            lbl.set_visible(empty);
        }
    });
}

/// Хук: активный магазин загружен из ui_state (при старте).
/// Восстанавливает выбор combos и инициирует SelectShop для fetch seller_name.
pub fn on_active_shop_loaded(shop: Option<&ActiveShop>) {
    let Some(shop) = shop else {
        // Нет сохранённого выбора — combo магазина уже на первом, профиль
        // обновится автоматически через on_profiles_loaded.
        return;
    };
    ACTIVE_SHOP.with(|a| *a.borrow_mut() = Some(shop.clone()));
    // Восстанавливаем combo провайдера.
    let pid = shop.provider_id.clone();
    W_PROVIDER.with(|w| {
        if let Some(combo) = w.borrow().as_ref() {
            combo.set_active_id(Some(&pid));
        }
    });
    // Восстанавливаем combo профиля (фильтруется по провайдеру в refresh).
    let pname = shop.profile_name.clone();
    W_PROFILE.with(|w| {
        if let Some(combo) = w.borrow().as_ref() {
            combo.set_active_id(Some(&pname));
        }
    });
    // Инициируем SelectShop — persist + fetch seller_name для заголовка.
    // (persist перезапишет то же значение — это ок.)
    if let Some(cs) = CMD.with(|c| c.borrow().clone()) {
        cs.send(UiCommand::SelectShop {
            provider_id: shop.provider_id.clone(),
            profile_name: shop.profile_name.clone(),
        });
    }
}

/// Хук: активный магазин изменён (выбор пользователя или восстановление).
/// Обновляем статус-лейбл. Заголовок окна обновляется в main_window.
pub fn on_active_shop_changed(
    _provider_id: &str,
    _provider_display_name: &str,
    seller_name: Option<&str>,
    profile_name: &str,
) {
    W_SHOP_STATUS.with(|w| {
        if let Some(l) = w.borrow().as_ref() {
            let text = match seller_name {
                Some(name) => format!("Продавец: {name}  (профиль «{profile_name}»)"),
                None => format!("Профиль активен: «{profile_name}». Имя продавца недоступно (WB или ошибка сети)."),
            };
            l.set_text(&text);
        }
    });
}

/// Обработчик события «поля авторизации загружены» — перерисовывает поля диалога.
pub fn on_auth_fields_loaded(provider_id: &str, fields: &[AuthFieldInfo]) {
    ACTIVE_DIALOG.with(|cell| {
        let state = cell.borrow().clone();
        let Some(state) = state else { return };
        let current = state
            .provider_combo
            .active_id()
            .map(|s| s.to_string())
            .unwrap_or_default();
        if current != provider_id {
            return;
        }
        render_dialog_fields(&state, fields);
    });
}

// ===== Построение вкладки =====

/// Строит вкладку «Магазин».
pub fn build(cs: &CommandSender) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.set_margin_top(16);
    root.set_margin_bottom(16);

    root.append(&crate::widgets::tab_help::title_row_with_help(
        "Магазин",
        "title-2",
        SHOP_HELP,
    ));

    root.append(&Label::builder()
        .label("Выберите маркетплейс и профиль — это активный магазин для всех вкладок (Загрузка, Отчёты). Профили ниже можно создавать, проверять и удалять.")
        .css_classes(["dim-label"])
        .halign(gtk4::Align::Start)
        .wrap(true)
        .build());

    // --- Блок «Активный магазин» ---
    root.append(&section_header("Активный магазин"));

    let shop_row = GtkBox::new(Orientation::Horizontal, 8);
    let provider_combo = ComboBoxText::new();
    provider_combo.set_tooltip_text(Some("Маркетплейс"));
    shop_row.append(&Label::new(Some("Маркетплейс:")));
    shop_row.append(&provider_combo);

    let profile_combo = ComboBoxText::new();
    profile_combo.set_tooltip_text(Some("Профиль учётных данных"));
    shop_row.append(&Label::new(Some("Профиль:")));
    shop_row.append(&profile_combo);
    root.append(&shop_row);

    let shop_status = Label::builder()
        .label("Магазин не выбран.")
        .css_classes(["dim-label"])
        .halign(gtk4::Align::Start)
        .wrap(true)
        .build();
    root.append(&shop_status);

    // --- Блок «Профили» (CRUD) ---
    root.append(&section_header("Профили учётных данных"));

    let cruds_row = GtkBox::new(Orientation::Horizontal, 8);
    let add_btn = Button::builder()
        .label("＋ Добавить")
        .css_classes(["suggested-action"])
        .build();
    let edit_btn = Button::builder()
        .label("✎ Изменить")
        .tooltip_text("Изменить выбранный профиль")
        .build();
    let check_btn = Button::builder().label("✓ Проверить").build();
    let del_btn = Button::builder().label("🗑 Удалить").build();
    cruds_row.append(&add_btn);
    cruds_row.append(&edit_btn);
    cruds_row.append(&check_btn);
    cruds_row.append(&del_btn);
    root.append(&cruds_row);

    root.append(&Label::builder()
        .label("Профиль = один продавец на одном маркетплейсе. Секреты хранятся в OS keychain.")
        .css_classes(["dim-label"])
        .halign(gtk4::Align::Start)
        .build());

    // Empty-state: призыв создать первый профиль (виден, пока профилей нет).
    let empty_state = Label::builder()
        .label("📋 Профилей пока нет. Нажмите «＋ Добавить» выше — создайте первый профиль (магазин + API-ключ); без него выгрузка отчётов недоступна.")
        .wrap(true)
        .xalign(0.0)
        .halign(gtk4::Align::Start)
        .margin_top(10)
        .css_classes(["heading"])
        .build();
    root.append(&empty_state);
    W_EMPTY.with(|w| *w.borrow_mut() = Some(empty_state.clone()));

    // Список профилей через gio.ListStore.
    let store = STATE.with(|s| s.borrow().store.clone());
    let selection = SingleSelection::new(Some(store.clone()));
    selection.set_autoselect(false);
    let list = ListView::new(Some(selection.clone()), None::<SignalListItemFactory>);

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

    // Сохраняем виджеты и состояние.
    W_PROVIDER.with(|w| *w.borrow_mut() = Some(provider_combo.clone()));
    W_PROFILE.with(|w| *w.borrow_mut() = Some(profile_combo.clone()));
    W_SHOP_STATUS.with(|w| *w.borrow_mut() = Some(shop_status.clone()));
    CMD.with(|c| *c.borrow_mut() = Some(cs.clone()));

    // Смена маркетплейса → обновляем combo профиля (фильтр по провайдеру).
    // Сами изменения магазина обрабатываются в смене профиля (SelectShop),
    // т.к. профиль однозначно определяет provider_id.
    provider_combo.connect_changed(move |_| {
        refresh_profile_combo();
    });

    // Смена профиля → SelectShop (persist + fetch seller_name).
    let cs_shop = cs.clone();
    let pc_for_shop = profile_combo.clone();
    let prov_combo_for_shop = provider_combo.clone();
    profile_combo.connect_changed(move |_| {
        let Some((pid, pname)) = current_shop(&prov_combo_for_shop, &pc_for_shop) else {
            // Нет профиля для провайдера — сбрасываем статус.
            W_SHOP_STATUS.with(|w| {
                if let Some(l) = w.borrow().as_ref() {
                    l.set_text("Нет профилей для этого маркетплейса. Создайте профиль ниже.");
                }
            });
            return;
        };
        ACTIVE_SHOP.with(|a| {
            *a.borrow_mut() = Some(ActiveShop {
                provider_id: pid.clone(),
                profile_name: pname.clone(),
            });
        });
        cs_shop.send(UiCommand::SelectShop {
            provider_id: pid,
            profile_name: pname,
        });
    });

    // Отслеживание выбора в CRUD-списке.
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

    // Кнопка «Изменить» — открыть диалог с предзаполнением выбранного профиля.
    let cs_edit = cs.clone();
    edit_btn.connect_clicked(move |_| {
        let idx = STATE.with(|s| s.borrow().selected);
        let profile = PROFILES.with(|p| p.borrow().get(idx).cloned());
        if let Some(profile) = profile {
            show_edit_dialog(&cs_edit, &profile);
        }
    });

    root
}

// ===== Хелперы выбора магазина =====

/// id активного маркетплейса из combo (значение элемента, не видимый текст).
fn provider_id_from_combo(combo: &ComboBoxText) -> Option<String> {
    combo.active_id().map(|s| s.to_string())
}

/// Возвращает (provider_id, profile_name) активного выбора combos.
fn current_shop(
    provider_combo: &ComboBoxText,
    profile_combo: &ComboBoxText,
) -> Option<(String, String)> {
    let pid = provider_combo.active_id()?.to_string();
    let pname = profile_combo.active_id()?.to_string();
    Some((pid, pname))
}

/// Обновить combo профилей под текущего провайдера (магазин).
fn refresh_profile_combo() {
    let combo = W_PROFILE.with(|w| w.borrow().clone());
    let Some(combo) = combo else { return };
    let pid = W_PROVIDER.with(|wp| {
        wp.borrow()
            .as_ref()
            .and_then(|c| provider_id_from_combo(c))
    });
    let profiles = PROFILES.with(|p| p.borrow().clone());

    // Блокируем connect_changed на время перестройки, чтобы не слать лишних SelectShop.
    // (Иначе при очистке combo срабатывал бы обработчик с «нет профилей».)
    // Простой способ: флаг.
    combo.remove_all();
    let mut any = false;
    for p in &profiles {
        if pid.as_deref() == Some(p.provider_id.as_str()) {
            // Имя профиля — и видимый текст, и значение (active_id → profile_name).
            combo.append(Some(p.name.as_str()), p.name.as_str());
            any = true;
        }
    }
    if !any {
        combo.append_text("(нет профилей — создайте ниже)");
    }
    combo.set_active(Some(0));
}

fn section_header(text: &str) -> Label {
    Label::builder()
        .label(text)
        .css_classes(["heading"])
        .halign(gtk4::Align::Start)
        .margin_top(8)
        .build()
}

// ===== CRUD профилей (перенос из profiles.rs) =====

/// Запись в списке профилей (для отображения).
#[derive(Clone)]
struct ProfileRow {
    name: String,
    provider_id: String,
}

/// Состояние CRUD-списка профилей.
struct ProfilesState {
    store: gio::ListStore,
    rows: Vec<ProfileRow>,
    selected: usize,
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

/// Состояние активного диалога добавления.
struct AddDialogState {
    dialog: adw::MessageDialog,
    name_entry: Entry,
    provider_combo: ComboBoxText,
    /// Динамически созданные поля: (field_info, entry).
    fields: RefCell<Vec<(AuthFieldInfo, Entry)>>,
    grid: Grid,
    cs: CommandSender,
}

/// Открывает диалог добавления профиля. Список маркетплейсов наполняется
/// из реестра (DIALOG_PROVIDERS), без хардкода.
fn show_add_dialog(cs: &CommandSender) {
    show_profile_dialog(cs, None);
}

fn show_edit_dialog(cs: &CommandSender, profile: &Profile) {
    show_profile_dialog(cs, Some(profile.clone()));
}

/// Диалог создания/изменения профиля. `edit` = Some → изменение (предзаполнение
/// полей + сохранение с id → upsert UPDATE); None — создание нового.
fn show_profile_dialog(cs: &CommandSender, edit: Option<Profile>) {
    let is_edit = edit.is_some();
    let dialog = adw::MessageDialog::builder()
        .heading(if is_edit {
            "Изменить профиль"
        } else {
            "Новый профиль"
        })
        .body(if is_edit {
            "Измените нужные поля. Секретное поле (ключ/токен) оставьте пустым — тогда оно не изменится."
        } else {
            "Выберите маркетплейс и заполните данные доступа"
        })
        .build();

    let name_entry = Entry::builder().placeholder_text("Например: WB-основной").build();
    let provider_combo = ComboBoxText::new();
    // Наполняем из реестра (динамически), fallback — если ещё не загружен.
    let providers = DIALOG_PROVIDERS.with(|d| d.borrow().clone());
    if providers.is_empty() {
        provider_combo.append(Some("ozon"), "Ozon");
        provider_combo.append(Some("wildberries"), "Wildberries");
    } else {
        for pr in &providers {
            // Показываем человекочитаемое имя; id (=значение) получаем через active_id().
            provider_combo.append(Some(&pr.id), &pr.display_name);
        }
    }
    provider_combo.set_active(Some(0));

    // Режим изменения: предзаполняем имя и выбираем провайдер профиля.
    EDIT_TARGET.with(|e| *e.borrow_mut() = edit.clone());
    if let Some(p) = &edit {
        name_entry.set_text(&p.name);
        provider_combo.set_active_id(Some(&p.provider_id));
    }

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

    ACTIVE_DIALOG.with(|cell| *cell.borrow_mut() = Some(state.clone()));

    // При смене провайдера — запрашиваем его поля авторизации.
    let cs_fields = cs.clone();
    provider_combo.connect_changed(move |combo| {
        if let Some(pid) = combo.active_id() {
            cs_fields.send(UiCommand::LoadAuthFields(pid.to_string()));
        }
    });
    // Запрос для изначально выбранного провайдера.
    if let Some(pid) = provider_combo.active_id() {
        cs.send(UiCommand::LoadAuthFields(pid.to_string()));
    }

    // Обработчик ответа.
    let state_for_close = state.clone();
    dialog.connect_response(None, move |_dlg, response: &str| {
        if response == "ok" {
            save_profile_from_dialog(&state_for_close);
        } else {
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

/// Сохраняет профиль из диалога.
fn save_profile_from_dialog(state: &Rc<AddDialogState>) {
    let name = state.name_entry.text().to_string();
    let provider_id = state
        .provider_combo
        .active_id().map_or_else(|| "test".into(), |s| s.to_string());

    if name.is_empty() {
        return;
    }

    // Режим изменения: сохраняем id профиля (→ upsert UPDATE вместо INSERT).
    let editing = EDIT_TARGET.with(|e| e.borrow_mut().take());
    let mut profile = Profile::new(&name, &provider_id);
    if let Some(p) = &editing {
        profile.id = p.id;
    }
    for (field, entry) in state.fields.borrow().iter() {
        let value = entry.text().to_string();
        if !value.is_empty() {
            profile.auth_metadata.insert(field.id.clone(), value);
        }
    }

    // append_local — только для нового профиля (для существующего — update в БД).
    if editing.is_none() {
        STATE.with(|s| s.borrow_mut().append_local(&name, &provider_id));
    }
    state.cs.send(UiCommand::SaveProfile(profile));

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

/// Перерисовывает динамические поля в диалоге.
fn render_dialog_fields(state: &Rc<AddDialogState>, fields: &[AuthFieldInfo]) {
    let mut child = state.grid.first_child();
    while let Some(c) = child {
        let next = c.next_sibling();
        state.grid.remove(&c);
        child = next;
    }
    state.fields.borrow_mut().clear();

    state.grid.attach(&Label::new(Some("Имя профиля:")), 0, 0, 1, 1);
    state.grid.attach(&state.name_entry, 1, 0, 1, 1);
    state.grid.attach(&Label::new(Some("Маркетплейс:")), 0, 1, 1, 1);
    state.grid.attach(&state.provider_combo, 1, 1, 1, 1);

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
        // Режим изменения (EDIT_TARGET задан): несекретные поля — из профиля,
        // секретное — пусто с подсказкой (keyring не трогается: пустое поле не
        // перезаписывает существующий секрет, см. store_profile_secrets).
        // В режиме создания — поле не трогаем (свой placeholder из match выше).
        let target = EDIT_TARGET.with(|e| e.borrow().clone());
        if let Some(p) = &target {
            if field.secret {
                entry.set_placeholder_text(Some("оставьте пустым, чтобы не менять"));
            } else if let Some(v) = p.auth_metadata.get(&field.id) {
                entry.set_text(v);
            }
        }
        state.fields.borrow_mut().push((field.clone(), entry));
    }
    state.grid.show();
}

/// Контекстная помощь вкладки «Магазин» (кнопка «?» в заголовке).
const SHOP_HELP: &[crate::widgets::tab_help::HelpBlock] = &[
    crate::widgets::tab_help::HelpBlock::H("Что здесь"),
    crate::widgets::tab_help::HelpBlock::T("Выбор маркетплейса и профиля — это <b>активный магазин</b> для всех вкладок (Отчёты, Загрузка, Расписания)."),
    crate::widgets::tab_help::HelpBlock::H("Как добавить профиль"),
    crate::widgets::tab_help::HelpBlock::B(&[
        "Выберите маркетплейс (Ozon / Wildberries).",
        "Нажмите «Добавить профиль» и введите имя.",
        "Впишите API-ключ (см. ниже) и сохраните.",
        "Нажмите «Проверить» — статус должен стать OK: ключ верный.",
    ]),
    crate::widgets::tab_help::HelpBlock::H("Где взять API-ключ"),
    crate::widgets::tab_help::HelpBlock::B(&[
        "Ozon: кабинет → Настройки → API-ключи (Seller API). Нужны Client-Id и Api-Key; ключ живёт 6 месяцев.",
        "Wildberries: кабинет → Профиль → Доступ к API → «Создать токен» (одно значение).",
    ]),
    crate::widgets::tab_help::HelpBlock::H("Частые вопросы"),
    crate::widgets::tab_help::HelpBlock::B(&[
        "«401/403» при проверке — ключ неверный или истёк: перевыпустите в кабинете и обновите профиль.",
        "Ключи хранятся в Диспетчере учётных данных Windows, не в файлах программы.",
        "Профили пропали? Запускайте программу под тем же пользователем Windows.",
    ]),
];
