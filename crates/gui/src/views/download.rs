//! Вкладка «Загрузка» — самодостаточный интерактивный цикл:
//! провайдер → профиль → отчёт → фильтры → список/генерация → скачивание.
//!
//! Поддерживает оба режима (спец. AcquisitionMode):
//!  * Browsable: список → выбор чекбоксами → «Скачать выбранные».
//!  * Period: период → «Скачать по периоду».

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use chrono::{Datelike, NaiveDate};
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, CheckButton, ComboBoxText, Entry, Image, Label, ListBox, Orientation,
    PolicyType, ScrolledWindow,
};

use mdwf_core::{DocumentEntry, DocumentFilter, DownloadedFile, ReportParams};
use mdwf_storage::DownloadedDocInfo;

use crate::channels::{
    ActiveShop, CommandSender, DocumentCategoryInfo, DocumentSel, DownloadState, ReportInfo,
};

thread_local! {
    static REPORTS: Rc<RefCell<Vec<ReportInfo>>> = Rc::new(RefCell::new(Vec::new()));
    static DOCS: Rc<RefCell<Vec<DocumentEntry>>> = Rc::new(RefCell::new(Vec::new()));
    static CHECKS: Rc<RefCell<Vec<(DocumentSel, CheckButton)>>> = Rc::new(RefCell::new(Vec::new()));
    /// Скачанные документы активного магазина+отчёта (document_id → info).
    /// Заполняется из UiEvent::DownloadsListed; используется для значка «уже загружен».
    static DOWNLOADED: Rc<RefCell<HashMap<String, DownloadedDocInfo>>> = Rc::new(RefCell::new(HashMap::new()));
    // Командный канал (для авто-запросов при смене выбора).
    static CMD: Rc<RefCell<Option<CommandSender>>> = Rc::new(RefCell::new(None));
    /// Активный магазин (из вкладки «Магазин») — единый источник правды выбора.
    /// None — магазин ещё не выбран, операции выгрузки недоступны.
    static ACTIVE_SHOP: Rc<RefCell<Option<ActiveShop>>> = Rc::new(RefCell::new(None));
    // Виджеты (сохраняем после build для обновления из событий).
    static W_REPORT: Rc<RefCell<Option<ComboBoxText>>> = Rc::new(RefCell::new(None));
    /// Read-only лейбл активного магазина (обновляется из ActiveShopChanged).
    static W_SHOP_LABEL: Rc<RefCell<Option<Label>>> = Rc::new(RefCell::new(None));
    static W_LIST: Rc<RefCell<Option<ListBox>>> = Rc::new(RefCell::new(None));
    static W_RESULT: Rc<RefCell<Option<Label>>> = Rc::new(RefCell::new(None));
    static W_RESULT_BOX: Rc<RefCell<Option<GtkBox>>> = Rc::new(RefCell::new(None));
    static W_MODE_HINT: Rc<RefCell<Option<Label>>> = Rc::new(RefCell::new(None));
    /// URL раздела ЛК текущего отчёта + кнопка «Открыть в ЛК» (у инфо-панели).
    static W_LK_BTN: Rc<RefCell<Option<gtk4::Button>>> = Rc::new(RefCell::new(None));
    static LK_URL: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    static W_LIST_BTN: Rc<RefCell<Option<Button>>> = Rc::new(RefCell::new(None));
    static W_PERIOD_BTN: Rc<RefCell<Option<Button>>> = Rc::new(RefCell::new(None));
    static W_DOWNLOAD_BTN: Rc<RefCell<Option<Button>>> = Rc::new(RefCell::new(None));
    static W_CATEGORY_COMBO: Rc<RefCell<Option<ComboBoxText>>> = Rc::new(RefCell::new(None));
    static W_CATEGORY_LABEL: Rc<RefCell<Option<Label>>> = Rc::new(RefCell::new(None));
    static W_DATE_FROM: Rc<RefCell<Option<Entry>>> = Rc::new(RefCell::new(None));
    static W_DATE_TO: Rc<RefCell<Option<Entry>>> = Rc::new(RefCell::new(None));
    static W_LIMIT: Rc<RefCell<Option<Entry>>> = Rc::new(RefCell::new(None));
    /// Описание выбранного периода («январь 2025», «3 квартал 2025», «с … по …»).
    static W_RANGE_DESC: Rc<RefCell<Option<Label>>> = Rc::new(RefCell::new(None));
    /// Карта: отображаемое имя категории → технический идентификатор (для WB API).
    /// Заполняется при загрузке категорий, используется в build_filter.
    static CATEGORIES: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(Vec::new()));
}

/// Названия месяцев по-русски (индекс 0 = Январь = месяц 1).
const MONTH_NAMES: [&str; 12] = [
    "Январь",
    "Февраль",
    "Март",
    "Апрель",
    "Май",
    "Июнь",
    "Июль",
    "Август",
    "Сентябрь",
    "Октябрь",
    "Ноябрь",
    "Декабрь",
];

/// Возвращает provider_id активного магазина (из вкладки «Магазин»).
fn active_provider_id() -> Option<String> {
    ACTIVE_SHOP.with(|a| a.borrow().as_ref().map(|s| s.provider_id.clone()))
}

/// Возвращает (provider_id, profile_name) активного магазина.
fn active_target() -> Option<(String, String)> {
    ACTIVE_SHOP.with(|a| {
        a.borrow().as_ref().map(|s| (s.provider_id.clone(), s.profile_name.clone()))
    })
}

/// Хук: активный магазин изменён (из вкладки «Магазин» или восстановление).
/// Обновляем read-only лейбл, перезагружаем список отчётов провайдера.
pub fn on_active_shop_changed(
    provider_id: &str,
    provider_display_name: &str,
    seller_name: Option<&str>,
    profile_name: &str,
) {
    ACTIVE_SHOP.with(|a| {
        *a.borrow_mut() = Some(ActiveShop {
            provider_id: provider_id.to_string(),
            profile_name: profile_name.to_string(),
        });
    });
    // Read-only лейбл магазина.
    W_SHOP_LABEL.with(|w| {
        if let Some(l) = w.borrow().as_ref() {
            let display = seller_name.unwrap_or(profile_name);
            l.set_text(&format!("Магазин: {provider_display_name} — {display}"));
        }
    });
    // Перезагружаем отчёты нового провайдера (очистит combo + авто-запрос).
    if let Some(cs) = CMD.with(|c| c.borrow().clone()) {
        // Очищаем combo отчётов (покажем «загрузка…»).
        if let Some(combo) = W_REPORT.with(|w| w.borrow().clone()) {
            combo.remove_all();
            combo.append_text("(загрузка…)");
            combo.set_active(Some(0));
        }
        cs.send(crate::channels::UiCommand::LoadReports(provider_id.to_string()));
    }
}

/// Хук: категории документов WB загружены → заполняем combo.
///
/// В combo показываем человекочитаемый `label` (русское название, напр. «УПД»),
/// а в `CATEGORIES` храним карту `label → value`, чтобы при сборке фильтра
/// переводить выбранное имя обратно в технический идентификатор (`value`),
/// который WB ожидает в параметре `category`.
pub fn on_document_categories_loaded(res: &Result<Vec<DocumentCategoryInfo>, String>) {
    let combo = W_CATEGORY_COMBO.with(|w| w.borrow().clone());
    let Some(combo) = combo else { return };
    combo.remove_all();
    combo.append_text("(все)");
    // Очищаем карту перед заполнением — список мог быть перезагружен.
    CATEGORIES.with(|c| c.borrow_mut().clear());
    match res {
        Err(e) => {
            combo.append_text(&format!("(ошибка: {e})"));
        }
        Ok(cats) if cats.is_empty() => {
            combo.append_text("(нет категорий)");
        }
        Ok(cats) => {
            CATEGORIES.with(|c| {
                *c.borrow_mut() = cats
                    .iter()
                    .map(|cat| (cat.label.clone(), cat.value.clone()))
                    .collect();
            });
            for cat in cats {
                combo.append_text(&cat.label);
            }
        }
    }
    combo.set_active(Some(0));
}

/// Хук: отчёты загружены (все уже принадлежат запрошенному провайдеру).
pub fn on_reports_loaded(reports: &[ReportInfo]) {
    // Защита от гонки: если пользователь уже сменил магазин (=> провайдера),
    // устаревший результат игнорируем — иначе он затрёт актуальный список.
    let active_pid = active_provider_id();
    let result_pid = reports.first().map(|r| r.provider_id.clone());
    if let (Some(active), Some(got)) = (active_pid.as_deref(), result_pid.as_deref()) {
        if active != got {
            tracing::debug!(
                "on_reports_loaded: игнорируем устаревший результат \
                 (провайдер {got:?}, сейчас активен {active:?})"
            );
            return;
        }
    }

    REPORTS.with(|r| *r.borrow_mut() = reports.to_vec());
    let combo = W_REPORT.with(|w| w.borrow().clone());
    let Some(combo) = combo else { return };

    // Блокируем connect_changed на время программной перестройки combo,
    // чтобы не вызвать каскад лишних maybe_request_categories.
    REPORT_CHANGED_HANDLER.with(|h| {
        if let Some(id) = h.borrow().as_ref() {
            combo.block_signal(id);
        }
    });

    combo.remove_all();
    if reports.is_empty() {
        combo.append_text("(нет отчётов)");
    } else {
        for r in reports {
            // Только человекочитаемое имя; type_id хранится в REPORTS (по индексу).
            combo.append_text(&r.display_name);
        }
    }

    // Восстанавливаем выбранный отчёт из сохранённого состояния/предвыбора.
    // Провайдер-guard: pending от ДРУГОГО провайдера (напр., состояние было
    // сохранено за test/ozon, а загружаются отчёты WB) не восстанавливаем —
    // иначе выберется несуществующий индекс (stale state, §11-5).
    let pending_provider = PENDING_PROVIDER.with(|p| p.borrow_mut().take());
    let stale = pending_provider.is_some_and(|pv| {
        reports.first().is_some_and(|r| r.provider_id != pv)
    });
    let pending = PENDING_REPORT.with(|p| p.borrow_mut().take());
    if stale {
        tracing::debug!(
            "on_reports_loaded: pending отчёта другого провайдера — отброшен ({:?})",
            pending
        );
        combo.set_active(Some(0));
    } else if let Some(rtype) = pending {
        // Combo хранит только display_name — по тексту type_id не найти. Индексы
        // combo и REPORTS совпадают (заполняются одним циклом), ищем по REPORTS.
        let idx = REPORTS.with(|r| r.borrow().iter().position(|rep| rep.type_id == rtype));
        combo.set_active(Some(idx.unwrap_or(0) as u32));
    } else {
        combo.set_active(Some(0));
    }

    REPORT_CHANGED_HANDLER.with(|h| {
        if let Some(id) = h.borrow().as_ref() {
            combo.unblock_signal(id);
        }
    });

    update_mode_hint();
    // Явно запрашиваем категории, т.к. set_active при заблокированном сигнале
    // не вызовет connect_changed.
    maybe_request_categories();
}

/// Запоминает отчёт (`type_id`), который нужно выбрать в combo «Загрузки», и
/// выбирает его сразу, если combo уже заполнен. Вызывается из вкладки «Отчёты»
/// при клике по отчёту (предвыбор + переход на «Загрузка»).
pub fn set_pending_report(type_id: &str) {
    PENDING_REPORT.with(|p| *p.borrow_mut() = Some(type_id.to_string()));
    // Предвыбор всегда для текущего активного магазина.
    PENDING_PROVIDER.with(|p| *p.borrow_mut() = active_provider_id());
    // Если combo уже заполнен — выберем немедленно; иначе выберется при
    // следующей загрузке списка отчётов (on_reports_loaded возьмёт PENDING_REPORT).
    select_report_by_type(type_id);
}

/// Выбирает в combo отчёт с заданным `type_id` (по индексу в REPORTS). Сигнал
/// connect_changed НЕ блокируется — штатно срабатывают обновление подсказки и
/// загрузка категорий выбранного отчёта. `true`, если отчёт найден и выбран.
fn select_report_by_type(type_id: &str) -> bool {
    let Some(combo) = W_REPORT.with(|w| w.borrow().clone()) else {
        return false;
    };
    let idx = REPORTS.with(|r| r.borrow().iter().position(|rep| rep.type_id == type_id));
    if let Some(i) = idx {
        combo.set_active(Some(i as u32));
        true
    } else {
        false
    }
}

pub fn build(cs: &CommandSender) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.set_margin_top(16);
    root.set_margin_bottom(16);

    root.append(&crate::widgets::tab_help::title_row_with_help(
        "Загрузка документов",
        "title-2",
        DOWNLOAD_HELP,
    ));

    root.append(&Label::builder()
        .label("Магазин выбирается во вкладке «Магазин». Здесь задайте отчёт и фильтры, затем нажмите «Список документов» (для отчётов-списков) или «Скачать по периоду» (для отчётов по периоду).")
        .css_classes(["dim-label"])
        .halign(gtk4::Align::Start)
        .wrap(true)
        .build());

    // Read-only индикатор активного магазина (обновляется из ActiveShopChanged).
    let shop_label = Label::builder()
        .label("Магазин: не выбран — выберите во вкладке «Магазин».")
        .css_classes(["heading"])
        .halign(gtk4::Align::Start)
        .wrap(true)
        .build();
    root.append(&shop_label.clone());

    // --- Строка 1: отчёт + обновить (магазин берётся из вкладки «Магазин») ---
    let row1 = GtkBox::new(Orientation::Horizontal, 8);

    let report_combo = ComboBoxText::new();
    report_combo.set_tooltip_text(Some("Тип отчёта"));
    row1.append(&Label::new(Some("Отчёт:")));
    row1.append(&report_combo);

    let load_reports_btn = super::icon_button("Обновить", "view-refresh-symbolic");
    load_reports_btn.set_tooltip_text(Some("Перезагрузить список отчётов провайдера"));
    row1.append(&load_reports_btn);
    root.append(&row1);

    // Подсказка о режиме выбранного отчёта.
    let mode_hint = Label::builder()
        .label("")
        .css_classes(["dim-label"])
        .halign(gtk4::Align::Start)
        .hexpand(true)
        .wrap(true)
        .build();
    // Кнопка «Открыть в ЛК» — рядом с инфо-панелью, видна когда у текущего
    // отчёта известен URL раздела кабинета (обновляется в update_mode_hint).
    let lk_btn = super::icon_button("Открыть в ЛК", "insert-link-symbolic");
    lk_btn.set_tooltip_text(Some("Открыть раздел этого отчёта в личном кабинете"));
    lk_btn.set_visible(false);
    lk_btn.set_valign(gtk4::Align::Center);
    lk_btn.connect_clicked(|_| {
        let url = LK_URL.with(|u| u.borrow().clone());
        if !url.is_empty() {
            if let Err(e) = super::open_url(&url) {
                eprintln!("open_url: {e}");
                super::show_url_error(&url, &e);
            }
        }
    });
    let info_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    info_row.append(&mode_hint);
    info_row.append(&lk_btn);
    root.append(&info_row);

    // --- Строка 2: период (диапазон + интервал) ---
    // Фильтры разложены на ДВЕ строки: одна длинная задавала минимальную
    // ширину окна ~710px и мешала уменьшению (жалоба на размер окна).
    // Период по умолчанию: последний год (диапазон) + прошлый месяц (для period-отчётов).
    let today = chrono::Local::now().date_naive();
    let year_ago = today - chrono::Duration::days(365);
    let default_from = year_ago.format("%d.%m.%Y").to_string();
    let default_to = today.format("%d.%m.%Y").to_string();
    let row2 = GtkBox::new(Orientation::Horizontal, 8);
    let category_combo = ComboBoxText::new();
    category_combo.append_text("(все)");
    category_combo.set_active(Some(0));
    category_combo.set_tooltip_text(Some("Категория документа (загружается автоматически из WB)"));
    let date_from = Entry::builder().placeholder_text("с ДД.ММ.ГГГГ").width_chars(12).text(&default_from).build();
    let date_to = Entry::builder().placeholder_text("по ДД.ММ.ГГГГ").width_chars(12).text(&default_to).build();
    let limit_entry = Entry::builder().placeholder_text("лимит").width_chars(6).build();

    // Кнопка «📅 Интервал» — выбор стандартного интервала (неделя/месяц/квартал/год)
    // через виджет widgets::interval_picker. Проставляет date_from/date_to.
    let interval_btn = gtk4::MenuButton::new();
    interval_btn.set_child(Some(&super::icon_label_child("Интервал", "x-office-calendar-symbolic")));
    interval_btn.set_tooltip_text(Some("Выбрать стандартный интервал: месяц / квартал / полугодие / год"));
    let interval_popover = gtk4::Popover::new();
    {
        let df = date_from.clone();
        let dt = date_to.clone();
        let pop = interval_popover.clone();
        let picker = crate::widgets::interval_picker::make_interval_picker(move |f: &str, t: &str| {
            df.set_text(f);
            dt.set_text(t);
            pop.popdown();
            update_mode_hint();
            schedule_save();
        });
        interval_popover.set_child(Some(&picker.widget));
        // При открытии — позиционируем виджет на ТЕКУЩИЙ период полей дат
        // (вкладка месяц/квартал/полугодие/год + год), а не на прошлый выбор.
        {
            let sync = picker.sync.clone();
            let df = date_from.clone();
            let dt = date_to.clone();
            interval_popover.connect_notify_local(Some("visible"), move |popw, _| {
                if popw.is_visible() {
                    if let (Some(f), Some(t)) = (
                        super::parse_date_flex(&df.text()),
                        super::parse_date_flex(&dt.text()),
                    ) {
                        sync(f, t);
                    }
                }
            });
        }
    }
    interval_btn.set_popover(Some(&interval_popover));

    row2.append(&Label::new(Some("Период:")));
    row2.append(&date_from);
    // Кнопка-календарь для date_from
    row2.append(&super::make_date_picker(&date_from, "%d.%m.%Y"));
    row2.append(&Label::new(Some("..")));
    row2.append(&date_to);
    // Кнопка-календарь для date_to
    row2.append(&super::make_date_picker(&date_to, "%d.%m.%Y"));
    // Кнопка выбора стандартного интервала
    row2.append(&interval_btn);
    root.append(&row2);

    // --- Строка 2b: категория + лимит ---
    let row2b = GtkBox::new(Orientation::Horizontal, 8);
    let category_label = Label::new(Some("Категория:"));
    row2b.append(&category_label);
    row2b.append(&category_combo);
    row2b.append(&Label::new(Some("Лимит:")));
    row2b.append(&limit_entry);
    root.append(&row2b);

    // Описание выбранного периода («январь 2025» / «3 квартал 2025» /
    // «второе полугодие 2024» / «23 января 2026» / «с 04.03.2025 по…») —
    // вычисляется ИЗ ПОЛЕЙ ДАТ, как бы они ни были заданы: виджет интервала,
    // ручной ввод, календарь, восстановление состояния.
    let range_desc = Label::builder()
        .label("")
        .halign(gtk4::Align::Start)
        .wrap(true)
        .css_classes(["dim-label"])
        .build();
    root.append(&range_desc);
    W_RANGE_DESC.with(|w| *w.borrow_mut() = Some(range_desc.clone()));
    date_from.connect_changed(|_| refresh_range_desc());
    date_to.connect_changed(|_| refresh_range_desc());
    refresh_range_desc();

    // --- Кнопки действий (два ряда: один длинный ряд держал min-ширину
    // окна ~680px — жалоба на размер окна) ---
    let row3 = GtkBox::new(Orientation::Horizontal, 8);
    let list_btn = super::icon_button("Список документов", "view-list-symbolic");
    list_btn.set_tooltip_text(Some("Для отчётов-списков (Browsable)"));
    let download_btn = super::icon_button("Скачать выбранные", "document-save-symbolic");
    download_btn.add_css_class("suggested-action");
    download_btn.set_tooltip_text(Some("Скачать отмеченные документы"));
    row3.append(&list_btn);
    row3.append(&download_btn);
    root.append(&row3);

    let row3b = GtkBox::new(Orientation::Horizontal, 8);
    let period_btn = super::icon_button("Скачать по периоду", "x-office-calendar-symbolic");
    period_btn.set_tooltip_text(Some("Сгенерировать отчёт за период"));
    let cancel_btn = super::icon_button("Отмена", "process-stop-symbolic");
    cancel_btn.set_tooltip_text(Some("Остановить текущее скачивание (если оно есть)"));
    {
        let cs_c = cs.clone();
        cancel_btn.connect_clicked(move |_| {
            cs_c.cancel_current();
        });
    }
    row3b.append(&period_btn);
    row3b.append(&cancel_btn);
    root.append(&row3b);

    // --- Список документов (с чекбоксами) ---
    let list_box = ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::None);
    list_box.set_vexpand(true);
    let scroll = ScrolledWindow::builder()
        .child(&list_box)
        .hscrollbar_policy(PolicyType::Never)
        .build();
    root.append(&scroll);

    // --- Результат (контейнер: label + кнопка "Открыть папку") ---
    let result_box = GtkBox::new(Orientation::Horizontal, 8);
    let result_label = Label::builder()
        .label("Готов к работе. Выберите магазин во вкладке «Магазин».")
        .halign(gtk4::Align::Start)
        .css_classes(["dim-label"])
        .wrap(true)
        .hexpand(true)
        .build();
    result_box.append(&result_label);
    root.append(&result_box);

    // Сохраняем виджеты.
    W_SHOP_LABEL.with(|w| *w.borrow_mut() = Some(shop_label.clone()));
    W_REPORT.with(|w| *w.borrow_mut() = Some(report_combo.clone()));
    CMD.with(|c| *c.borrow_mut() = Some(cs.clone()));
    W_LIST.with(|w| *w.borrow_mut() = Some(list_box.clone()));
    W_RESULT.with(|w| *w.borrow_mut() = Some(result_label.clone()));
    W_RESULT_BOX.with(|w| *w.borrow_mut() = Some(result_box.clone()));
    W_MODE_HINT.with(|w| *w.borrow_mut() = Some(mode_hint.clone()));
    W_LK_BTN.with(|w| *w.borrow_mut() = Some(lk_btn.clone()));
    W_LIST_BTN.with(|w| *w.borrow_mut() = Some(list_btn.clone()));
    W_PERIOD_BTN.with(|w| *w.borrow_mut() = Some(period_btn.clone()));
    W_DOWNLOAD_BTN.with(|w| *w.borrow_mut() = Some(download_btn.clone()));
    W_CATEGORY_COMBO.with(|w| *w.borrow_mut() = Some(category_combo.clone()));
    W_CATEGORY_LABEL.with(|w| *w.borrow_mut() = Some(category_label.clone()));
    W_DATE_FROM.with(|w| *w.borrow_mut() = Some(date_from.clone()));
    W_DATE_TO.with(|w| *w.borrow_mut() = Some(date_to.clone()));
    W_LIMIT.with(|w| *w.borrow_mut() = Some(limit_entry.clone()));

    // Смена отчёта → обновить подсказку режима + доступность кнопок + автосохранение.
    {
        let handler_id = report_combo.connect_changed(move |_| {
            update_mode_hint();
            // Запрашиваем категории WB только при выборе отчёта wb.documents.
            maybe_request_categories();
            schedule_save();
        });
        REPORT_CHANGED_HANDLER.with(|h| *h.borrow_mut() = Some(handler_id));
    }
    update_mode_hint();

    // Автосохранение при изменении полей ввода + обновление инфо-панели (число
    // месяцев в интервале зависит от date_from/date_to).
    for entry in [&date_from, &date_to, &limit_entry] {
        let e = entry.clone();
        entry.connect_changed(move |_| {
            let _ = &e;
            update_mode_hint();
            schedule_save();
        });
    }
    // Автосохранение для category_combo.
    category_combo.connect_changed(move |_| {
        schedule_save();
    });

    // «Обновить» — запросить отчёты активного провайдера (из вкладки «Магазин»).
    let cs_rep = cs.clone();
    load_reports_btn.connect_clicked(move |_| {
        if let Some(pid) = active_provider_id() {
            cs_rep.send(crate::channels::UiCommand::LoadReports(pid));
        }
    });

    // Клоны полей для period-обработчика (list-обработчик замувит оригиналы).
    let df_per = date_from.clone();
    let dt_per = date_to.clone();

    // «Список документов».
    let cs_list = cs.clone();
    let cat_combo_list = category_combo.clone();
    list_btn.connect_clicked(move |_| {
        let Some((pid, pname, rtype)) = current_target() else {
            notify("Выберите профиль и отчёт.");
            return;
        };
        let filter = build_filter(&cat_combo_list, &date_from, &date_to, &limit_entry);
        // Категория опциональна: если не выбрана, вернутся документы всех категорий.
        // Оставляем подсказку только для удобства.
        if rtype == "wb.documents" && filter.category.is_none() {
            notify("Получаю документы всех категорий. Для фильтра выберите категорию из списка.");
        }
        let token = mdwf_core::CancelToken::new();
        cs_list.set_cancel_token(token.clone());
        cs_list.send(crate::channels::UiCommand::ListDocuments {
            provider_id: pid,
            profile_name: pname,
            report_type: rtype,
            filter,
            cancel: token,
        });
        notify("Запрос списка документов…");
    });

    // «Скачать выбранные».
    let cs_dl = cs.clone();
    download_btn.connect_clicked(move |_| {
        let Some((pid, pname, rtype)) = current_target() else {
            notify("Выберите профиль и отчёт.");
            return;
        };
        let docs: Vec<DocumentSel> = CHECKS.with(|c| {
            c.borrow()
                .iter()
                .filter(|(_, cb)| cb.is_active())
                .map(|(sel, _)| sel.clone())
                .collect()
        });
        if docs.is_empty() {
            notify("Отметьте документы в списке выше.");
            return;
        }
        let n = docs.len();
        let token = mdwf_core::CancelToken::new();
        cs_dl.set_cancel_token(token.clone());
        cs_dl.send(crate::channels::UiCommand::Download {
            provider_id: pid,
            profile_name: pname,
            report_type: rtype,
            documents: docs,
            params: ReportParams::new(),
            cancel: token,
        });
        notify(&format!("Скачивание {n} документов…"));
    });

    // «Скачать по периоду». Поведение зависит от PeriodKind отчёта:
    //   Month  → цикл по всем месяцам [date_from..date_to], по выгрузке на каждый
    //            (чтобы date_to не терялась; квартал=3, год=12);
    //   иначе  → один запрос за весь [date_from..date_to].
    let cs_per = cs.clone();
    period_btn.connect_clicked(move |_| {
        let Some((pid, pname, rtype)) = current_target() else {
            notify("Выберите профиль и отчёт.");
            return;
        };
        // Поля показывают ДД.ММ.ГГГГ → в API уходит ISO.
        let df = super::to_iso(&df_per.text());
        let dt = super::to_iso(&dt_per.text());
        if current_period_kind() == mdwf_core::PeriodKind::Month {
            let months = months_in_current_range();
            if months.is_empty() {
                notify("Задайте корректный диапазон дат (начало ≤ конец).");
                return;
            }
            let n = months.len();
            // Один токен на всю последовательность — «Отмена» прервёт все месяцы.
            let token = mdwf_core::CancelToken::new();
            cs_per.set_cancel_token(token.clone());
            for period in &months {
                if token.is_cancelled() {
                    break;
                }
                let params = ReportParams {
                    period: Some(period.clone()),
                    ..Default::default()
                }
                .with("date_from", df.clone())
                .with("date_to", dt.clone());
                cs_per.send(crate::channels::UiCommand::Download {
                    provider_id: pid.clone(),
                    profile_name: pname.clone(),
                    report_type: rtype.clone(),
                    documents: Vec::new(),
                    params,
                    cancel: token.clone(),
                });
            }
            let msg = if n == 1 {
                format!("Генерация отчёта за {}…", months[0])
            } else {
                format!(
                    "Генерация за {n} мес. ({}…{})…",
                    months.first().map_or("?", String::as_str),
                    months.last().map_or("?", String::as_str)
                )
            };
            notify(&msg);
        } else if let Some(cap) = current_max_range_days() {
            // Диапазонный отчёт с жёстким капом дат (balance ≤30, buyout/placement
            // ≤31): длинный интервал API отвергнет 4xx → режем на окна ≤ капа.
            let windows = windows_in_current_range(cap);
            if windows.is_empty() {
                notify("Задайте корректный диапазон дат (начало ≤ конец).");
                return;
            }
            let period = current_month_value();
            let token = mdwf_core::CancelToken::new();
            cs_per.set_cancel_token(token.clone());
            for (wf, wt) in &windows {
                if token.is_cancelled() {
                    break;
                }
                let params = ReportParams {
                    period: period.clone(),
                    ..Default::default()
                }
                .with("date_from", wf.clone())
                .with("date_to", wt.clone());
                cs_per.send(crate::channels::UiCommand::Download {
                    provider_id: pid.clone(),
                    profile_name: pname.clone(),
                    report_type: rtype.clone(),
                    documents: Vec::new(),
                    params,
                    cancel: token.clone(),
                });
            }
            let msg = if windows.len() == 1 {
                format!("Генерация за период (окно {}…{})…", windows[0].0, windows[0].1)
            } else {
                format!(
                    "Генерация за {} окон по ≤{cap} дн. ({}…{})…",
                    windows.len(),
                    windows.first().map_or("?", |w| w.0.as_str()),
                    windows.last().map_or("?", |w| w.1.as_str())
                )
            };
            notify(&msg);
        } else {
            // Range/Day/None — один запрос за весь диапазон (период = стартовый
            // месяц для отчётов, которым он нужен).
            let period = current_month_value();
            let params = ReportParams {
                period,
                ..Default::default()
            }
            .with("date_from", df)
            .with("date_to", dt);
            let token = mdwf_core::CancelToken::new();
            cs_per.set_cancel_token(token.clone());
            cs_per.send(crate::channels::UiCommand::Download {
                provider_id: pid,
                profile_name: pname,
                report_type: rtype,
                documents: Vec::new(),
                params,
                cancel: token,
            });
            notify("Генерация отчёта за период…");
        }
    });

    root
}

// ===== Хелперы =====

/// Возвращает (provider_id, profile_name, report_type) для активного магазина
/// и выбранного отчёта. provider/profile — из вкладки «Магазин» (ACTIVE_SHOP).
fn current_target() -> Option<(String, String, String)> {
    let (pid, pname) = active_target()?;
    let rtype = current_report_type()?;
    Some((pid, pname, rtype))
}

/// Возвращает выбранный report_type (без display_name).
/// Combo показывает ТОЛЬКО display_name (type_id спрятан, QW1) — сам текст
/// парсить нельзя: индекс combo синхронен списку REPORTS (заполняются одним
/// циклом в on_reports_loaded), берём type_id оттуда.
fn current_report_type() -> Option<String> {
    let combo = W_REPORT.with(|w| w.borrow().clone())?;
    let idx = combo.active()?;
    REPORTS.with(|r| r.borrow().get(idx as usize).map(|ri| ri.type_id.clone()))
}

/// `PeriodKind` выбранного отчёта (по метаданным). По умолчанию `Range`.
fn current_period_kind() -> mdwf_core::PeriodKind {
    let rtype = current_report_type();
    rtype
        .and_then(|t| REPORTS.with(|r| r.borrow().iter().find(|x| x.type_id == t).cloned()))
        .map_or(mdwf_core::PeriodKind::Range, |ri| ri.period_kind)
}

/// Жёсткий кап диапазона дат API выбранного отчёта (None = без ограничения).
fn current_max_range_days() -> Option<u32> {
    let rtype = current_report_type()?;
    REPORTS.with(|r| {
        r.borrow()
            .iter()
            .find(|x| x.type_id == rtype)
            .and_then(|ri| ri.max_range_days)
    })
}

/// Разбивает текущий диапазон полей дат на окна ≤ `cap_days` (включительно),
/// каждое окно — (from, to) в ISO. Пусто, если даты некорректны или from > to.
fn windows_in_current_range(cap_days: u32) -> Vec<(String, String)> {
    let from = W_DATE_FROM
        .with(|w| w.borrow().as_ref().and_then(|e| super::parse_date_flex(&e.text())));
    let to = W_DATE_TO
        .with(|w| w.borrow().as_ref().and_then(|e| super::parse_date_flex(&e.text())));
    let (Some(from), Some(to)) = (from, to) else {
        return Vec::new();
    };
    if from > to {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = from;
    let step = chrono::Duration::days(i64::from(cap_days.saturating_sub(1)));
    while start <= to {
        let end = (start + step).min(to);
        out.push((
            start.format("%Y-%m-%d").to_string(),
            end.format("%Y-%m-%d").to_string(),
        ));
        if end == to {
            break;
        }
        // succ_opt None (край календаря) — не зацикливаемся, завершаем.
        match end.succ_opt() {
            Some(next) => start = next,
            None => break,
        }
    }
    out
}

/// Период `YYYY-MM` из текущего `date_from` (месяц начала диапазона). Источник
/// правды для period-отчётов после удаления month/year combos. `None`, если
/// `date_from` не парсится как дата.
fn current_month_value() -> Option<String> {
    let s = W_DATE_FROM.with(|w| w.borrow().as_ref().map(|e| e.text().to_string()))?;
    let d = super::parse_date_flex(&s)?;
    Some(d.format("%Y-%m").to_string())
}

/// Список периодов `YYYY-MM` всех месяцев, попадающих в `[date_from, date_to]`
/// (включительно). Пусто, если даты некорректны или `date_from > date_to`.
/// Для месячных отчётов «Скачать по периоду» идёт циклом по каждому из этих
/// месяцев отдельной выгрузкой (чтобы `date_to` не терялась). Также число
/// элементов используется в инфо-панели («соберём по месяцам: N мес.»).
fn months_in_current_range() -> Vec<String> {
    let from = W_DATE_FROM.with(|w| w.borrow().as_ref().and_then(|e| super::parse_date_flex(&e.text())));
    let to = W_DATE_TO.with(|w| w.borrow().as_ref().and_then(|e| super::parse_date_flex(&e.text())));
    let (Some(from), Some(to)) = (from, to) else {
        return Vec::new();
    };
    if from > to {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cur = NaiveDate::from_ymd_opt(from.year(), from.month(), 1).unwrap_or(from);
    let end_month = NaiveDate::from_ymd_opt(to.year(), to.month(), 1).unwrap_or(to);
    while cur <= end_month {
        out.push(cur.format("%Y-%m").to_string());
        cur = match cur.checked_add_months(chrono::Months::new(1)) {
            Some(d) => d,
            None => break,
        };
    }
    out
}

/// Запрашивает категории WB только если активный магазин = wildberries и выбран
/// отчёт wb.documents. provider/profile берёт из активного магазина.
fn maybe_request_categories() {
    let rtype = current_report_type();
    let Some((pid, pname)) = active_target() else {
        return;
    };

    if rtype.as_deref() != Some("wb.documents") || pid != "wildberries" {
        return;
    }
    let Some(cs) = CMD.with(|c| c.borrow().clone()) else {
        return;
    };

    // Показываем «загрузка…» и отправляем запрос.
    if let Some(combo) = W_CATEGORY_COMBO.with(|w| w.borrow().clone()) {
        combo.remove_all();
        combo.append_text("(загрузка…)");
        combo.set_active(Some(0));
    }
    cs.send(crate::channels::UiCommand::LoadDocumentCategories {
        provider_id: "wildberries".into(),
        profile_name: pname,
    });
}

/// Обновить подсказку режима и доступность кнопок для выбранного отчёта.
/// Обновляет метку описания периода по текущим значениям полей дат.
/// Вызывается при любом изменении date_from/date_to (ввод, виджет интервала,
/// календарь, restore — все они меняют текст полей).
fn refresh_range_desc() {
    let from = W_DATE_FROM.with(|w| {
        w.borrow()
            .as_ref()
            .and_then(|e| super::parse_date_flex(&e.text()))
    });
    let to = W_DATE_TO.with(|w| {
        w.borrow()
            .as_ref()
            .and_then(|e| super::parse_date_flex(&e.text()))
    });
    let text = super::describe_range(from, to).unwrap_or_default();
    W_RANGE_DESC.with(|w| {
        if let Some(l) = w.borrow().as_ref() {
            l.set_text(&text);
        }
    });
}

fn update_mode_hint() {
    let rtype = current_report_type();
    let info = rtype
        .as_ref()
        .and_then(|t| REPORTS.with(|r| r.borrow().iter().find(|x| x.type_id == *t).cloned()));
    let (is_browsable, name) = info
        .as_ref()
        .map_or((false, String::new()), |r| (r.is_browsable, r.display_name.clone()));
    // URL раздела ЛК текущего отчёта → кнопка «Открыть в ЛК».
    LK_URL.with(|u| {
        *u.borrow_mut() = info
            .as_ref()
            .and_then(|r| r.cabinet_url.clone())
            .unwrap_or_default();
    });
    W_LK_BTN.with(|w| {
        if let Some(b) = w.borrow().as_ref() {
            b.set_visible(info.as_ref().and_then(|r| r.cabinet_url.as_deref()).is_some());
        }
    });

    W_MODE_HINT.with(|w| {
        if let Some(l) = w.borrow().as_ref() {
            let text = if name.is_empty() {
                String::new()
            } else {
                // Описание отчёта (из метаданных провайдера).
                let head = match info.as_ref().and_then(|r| r.description.as_deref()) {
                    Some(d) => format!("«{name}». {d}"),
                    None => format!("«{name}»."),
                };
                // Период-нота: как «Скачать по периоду» трактовать интервал.
                let kind = info
                    .as_ref()
                    .map_or(mdwf_core::PeriodKind::Range, |r| r.period_kind);
                let note = match kind {
                    mdwf_core::PeriodKind::Month => {
                        let months = months_in_current_range();
                        match months.len() {
                            0 => "📅 Задайте корректный диапазон дат (начало ≤ конец).".to_string(),
                            // Интервал ровно один месяц → обычная выгрузка за этот месяц.
                            1 => format!(
                                "📅 Месячный отчёт за {} — обычная выгрузка за месяц (кнопка «Скачать по периоду»).",
                                months[0]
                            ),
                            // Несколько месяцев → собираем по месяцам из интервала.
                            n => format!(
                                "📅 Месячный отчёт. За интервал соберём по месяцам: {n} мес. (кнопка «Скачать по периоду»)."
                            ),
                        }
                    }
                    mdwf_core::PeriodKind::Range if is_browsable => {
                        "📊 Задайте диапазон дат и категорию → «Список документов» → отметьте → «Скачать выбранные».".to_string()
                    }
                    mdwf_core::PeriodKind::Range => {
                        "📊 Диапазонный отчёт — выгрузка за весь выбранный период (кнопка «Скачать по периоду»).".to_string()
                    }
                    mdwf_core::PeriodKind::Day => {
                        "📅 Цикл по дням выбранного месяца.".to_string()
                    }
                    mdwf_core::PeriodKind::None => {
                        "🔹 Без привязки к периоду (срез/справочник).".to_string()
                    }
                };
                // Где отчёт находится в ЛК маркетплейса (путь из официальной
                // документации) — тем же приглушённым текстом в конец панели.
                let cab = info
                    .as_ref()
                    .and_then(|r| r.cabinet_path.as_deref())
                    .map(|c| format!(" В ЛК: {c}."))
                    .unwrap_or_default();
                format!("{head} {note}{cab}")
            };
            l.set_text(&text);
        }
    });

    // Доступность кнопок по режиму.
    let list_enabled = is_browsable;
    let dl_enabled = is_browsable;
    let period_enabled = !is_browsable;
    W_LIST_BTN.with(|w| { if let Some(b) = w.borrow().as_ref() { b.set_sensitive(list_enabled); } });
    W_DOWNLOAD_BTN.with(|w| { if let Some(b) = w.borrow().as_ref() { b.set_sensitive(dl_enabled); } });
    W_PERIOD_BTN.with(|w| { if let Some(b) = w.borrow().as_ref() { b.set_sensitive(period_enabled); } });

    // Категория нужна только для wb.documents — скрываем для остальных отчётов.
    let cat_visible = rtype.as_deref() == Some("wb.documents");
    if let Some(combo) = W_CATEGORY_COMBO.with(|w| w.borrow().clone()) {
        combo.set_visible(cat_visible);
    }
    if let Some(label) = W_CATEGORY_LABEL.with(|w| w.borrow().clone()) {
        label.set_visible(cat_visible);
    }
}

fn build_filter(category: &ComboBoxText, date_from: &Entry, date_to: &Entry, limit: &Entry) -> DocumentFilter {
    let mut f = DocumentFilter::default();
    if let Some(cat) = category.active_text() {
        let cat = cat.to_string();
        if cat != "(все)" && !cat.is_empty() {
            // Переводим выбранное отображаемое имя (label) в технический
            // идентификатор (value), который WB ожидает в параметре category.
            let resolved = CATEGORIES.with(|c| {
                c.borrow()
                    .iter()
                    .find(|(label, _)| label == &cat)
                    .map(|(_, value)| value.clone())
            });
            // Если перевод не найден (напр. служебные пункты combo),
            // категорию не передаём — WB вернёт документы всех категорий.
            f.category = resolved;
        }
    }
    if let Some(d) = super::parse_date_flex(&date_from.text()) {
        f.date_from = Some(d);
    }
    if let Some(d) = super::parse_date_flex(&date_to.text()) {
        f.date_to = Some(d);
    }
    if let Ok(n) = limit.text().parse::<u32>() {
        f.limit = Some(n);
    }
    f
}

// ===== Автосохранение состояния =====

macro_rules! entry_value {
    ($static_:ident) => {
        $static_.with(|c| c.borrow().as_ref().map(|e| e.text().to_string()))
    };
}

/// Собирает текущее состояние экрана из виджетов.
fn collect_state() -> DownloadState {
    // provider_id/profile_name берём из активного магазина (вкладка «Магазин»).
    // Эти поля в DownloadState дублируют ActiveShop (для обратной совместимости
    // сохранённого JSON), но источник правды выбора — ui_state/active_shop.
    let (provider_id, profile_name) = active_target()
        .map_or((None, None), |(pid, pname)| (Some(pid), Some(pname)));
    let report_type = current_report_type();
    DownloadState {
        provider_id,
        profile_name,
        report_type,
        category: W_CATEGORY_COMBO.with(|w| {
            w.borrow()
                .as_ref()
                .and_then(gtk4::ComboBoxText::active_text)
                .map(|s| s.to_string())
                .filter(|s| s != "(все)")
        }),
        date_from: entry_value!(W_DATE_FROM),
        date_to: entry_value!(W_DATE_TO),
        month: current_month_value(),
        limit: entry_value!(W_LIMIT),
    }
}

/// Отправляет команду сохранения состояния (вызывается из обработчиков).
fn schedule_save() {
    let Some(cs) = CMD.with(|c| c.borrow().clone()) else {
        return;
    };
    cs.send(crate::channels::UiCommand::SaveDownloadState(collect_state()));
}

/// Обработчик: сохранённое состояние загружено при старте → восстанавливаем выбор.
pub fn on_download_state_loaded(state: Option<&DownloadState>) {
    let Some(state) = state else {
        return;
    };

    // 1. Восстанавливаем категорию в combo (ищем совпадение).
    if let Some(v) = &state.category {
        W_CATEGORY_COMBO.with(|w| {
            if let Some(combo) = w.borrow().as_ref() {
                let n = combo.model().map_or(0, |m| m.iter_n_children(None));
                for i in 0..n {
                    combo.set_active(Some(i as u32));
                    if let Some(text) = combo.active_text() {
                        if text.as_str() == v {
                            break;
                        }
                    }
                }
            }
        });
    }
    if let Some(v) = &state.date_from {
        W_DATE_FROM.with(|w| { if let Some(e) = w.borrow().as_ref() { e.set_text(&super::disp_date(v)); } });
    }
    if let Some(v) = &state.date_to {
        W_DATE_TO.with(|w| { if let Some(e) = w.borrow().as_ref() { e.set_text(&super::disp_date(v)); } });
    }
    // Поле month НЕ восстанавливаем: combos месяца/года удалены, период теперь
    // выводится из date_from (восстановленного выше) — current_month_value().
    if let Some(v) = &state.limit {
        W_LIMIT.with(|w| { if let Some(e) = w.borrow().as_ref() { e.set_text(v); } });
    }

    // provider_id/profile_name НЕ восстанавливаем здесь — выбор магазина теперь
    // живет во вкладке «Магазин» и восстанавливается через ActiveShopLoaded.
    // Поля provider_id/profile_name в DownloadState сохраняются для обратной
    // совместимости сохранённого JSON, но источником правды не являются.

    // Восстанавливаем выбор отчёта (после загрузки списка отчётов активного магазина).
    if let Some(rtype) = &state.report_type {
        // Отложим восстановление: combo отчётов заполнится после LoadReports.
        // Запоминаем желаемый report_type для on_reports_loaded.
        PENDING_REPORT.with(|p| *p.borrow_mut() = Some(rtype.clone()));
        // Guard: состояние могло быть сохранено за другим провайдером.
        if let Some(pid) = &state.provider_id {
            PENDING_PROVIDER.with(|p| *p.borrow_mut() = Some(pid.clone()));
        }
        // Гонка порядка событий: список отчётов мог уже прийти (LoadReports
        // быстрее LoadDownloadState) — тогда PENDING_REPORT никто не consum-нет.
        // Выбираем сразу; если список ещё пуст, select_report_by_type — no-op,
        // выбор сделает on_reports_loaded по pending.
        select_report_by_type(rtype);
    }
}

thread_local! {
    /// Желаемый report_type, который нужно выбрать после загрузки списка отчётов.
    static PENDING_REPORT: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    /// Провайдер, для которого установлен PENDING_REPORT (guard от stale state:
    /// не восстанавливать отчёт другого провайдера — напр., сохранённое
    /// состояние за test/ozon при загрузке отчётов WB).
    static PENDING_PROVIDER: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    /// Handler id сигнала connect_changed у report_combo — чтобы блокировать
    /// его на время программной перестройки combo.
    static REPORT_CHANGED_HANDLER: Rc<RefCell<Option<glib::SignalHandlerId>>> =
        Rc::new(RefCell::new(None));
}

fn notify(msg: &str) {
    W_RESULT.with(|rw| {
        if let Some(l) = rw.borrow().as_ref() {
            l.set_text(msg);
        }
    });
}

// ===== События =====

/// Обработчик: список документов получен.
pub fn on_documents_listed(res: &Result<Vec<DocumentEntry>, String>) {
    match res {
        Err(e) => notify(&format!("Ошибка: {e}")),
        Ok(docs) => {
            DOCS.with(|d| *d.borrow_mut() = docs.clone());
            render_list(docs);
            notify(&format!("Получено документов: {}", docs.len()));
            // Запрашиваем статус «уже загружен» для активного магазина+отчёта.
            // (только для Browsable-отчётов со списком документов.)
            request_downloads_status();
        }
    }
}

/// Обработчик: список скачанных документов получен (для значка «уже загружен»).
/// Сохраняет в DOWNLOADED (если report_type совпадает с активным) и перерисовывает
/// список, чтобы показать/скрыть значки.
pub fn on_downloads_listed(report_type: &str, docs: Vec<DownloadedDocInfo>) {
    // Защита от гонки: применяем только если report_type совпадает с активным.
    let active_matches = current_report_type().is_some_and(|rt| rt == report_type);
    if !active_matches {
        return;
    }
    DOWNLOADED.with(|d| {
        let mut map = d.borrow_mut();
        map.clear();
        for info in &docs {
            map.insert(info.document_id.clone(), info.clone());
        }
    });
    // Перерисовываем список, чтобы отразить значки.
    let docs = DOCS.with(|d| d.borrow().clone());
    render_list(&docs);
}

/// Запрашивает у доменного слоя список уже скачанных документов для активного
/// магазина и выбранного отчёта. Ответ придёт в on_downloads_listed.
fn request_downloads_status() {
    let Some((_, profile_name)) = active_target() else {
        return;
    };
    let Some(report_type) = current_report_type() else {
        return;
    };
    let Some(cs) = CMD.with(|c| c.borrow().clone()) else {
        return;
    };
    cs.send(crate::channels::UiCommand::ListDownloads {
        profile_name,
        report_type,
    });
}

fn render_list(docs: &[DocumentEntry]) {
    W_LIST.with(|lw| {
        let list_box = match lw.borrow().as_ref() {
            Some(lb) => lb.clone(),
            None => return,
        };
        while let Some(child) = list_box.first_child() {
            list_box.remove(&child);
        }
        CHECKS.with(|c| c.borrow_mut().clear());

        if docs.is_empty() {
            list_box.append(&Label::new(Some("Документы не найдены.")));
            return;
        }

        let header = GtkBox::new(Orientation::Horizontal, 12);
        header.set_margin_start(8);
        header.set_margin_end(8);
        header.set_margin_top(4);
        header.set_margin_bottom(4);
        header.append(&Label::builder().label("").width_chars(3).build()); // чекбокс
        header.append(&Label::builder().label("").width_chars(3).build()); // значок статуса
        header.append(&Label::builder().label("Имя").width_chars(36).xalign(0.0).build());
        header.append(&Label::builder().label("Дата").width_chars(12).xalign(0.0).build());
        header.append(&Label::builder().label("Форматы").width_chars(16).xalign(0.0).build());
        header.append(&Label::builder().label("Размер").width_chars(10).xalign(0.0).build());
        header.append(&Label::builder().label("Действия").width_chars(16).xalign(0.0).build());
        list_box.append(&header);

        // Тулбар массового выделения (удобно для пакетной выгрузки многих документов).
        let toolbar = GtkBox::new(Orientation::Horizontal, 8);
        toolbar.set_margin_start(8);
        toolbar.set_margin_end(8);
        toolbar.set_margin_top(2);
        toolbar.set_margin_bottom(4);
        let all_btn = super::icon_button("Выбрать всё", "edit-select-all-symbolic");
        all_btn.connect_clicked(|_| {
            CHECKS.with(|c| {
                for (_, cb) in c.borrow().iter() {
                    cb.set_active(true);
                }
            });
        });
        let none_btn = super::icon_button("Снять выделение", "edit-clear-all-symbolic");
        none_btn.connect_clicked(|_| {
            CHECKS.with(|c| {
                for (_, cb) in c.borrow().iter() {
                    cb.set_active(false);
                }
            });
        });
        toolbar.append(&all_btn);
        toolbar.append(&none_btn);
        list_box.append(&toolbar);

        for doc in docs {
            let row = GtkBox::new(Orientation::Horizontal, 12);
            row.set_margin_start(8);
            row.set_margin_end(8);
            row.set_margin_top(2);
            row.set_margin_bottom(2);
            row.set_css_classes(&["doc-list-row"]);

            let cb = CheckButton::new();
            row.append(&cb);

            // Значок «уже загружен»: если document_id есть в DOWNLOADED — зелёный ✓.
            let downloaded_info = DOWNLOADED.with(|d| d.borrow().get(&doc.id).cloned());
            let status_label = if let Some(info) = &downloaded_info {
                let date = info.downloaded_at.format("%Y-%m-%d %H:%M").to_string();
                let lbl = Label::builder()
                    .label("✓")
                    .css_classes(["success"])
                    .tooltip_text(format!("Скачан {date}:\n{}", info.file_path).as_str())
                    .build();
                lbl
            } else {
                Label::builder().label("").width_chars(3).build()
            };
            row.append(&status_label);

            // Иконка типа файла (PNG из gresource) + название в одном Box,
            // чтобы не сбить выравнивание колонок с header.
            let ext = doc.extensions.first();
            let name_box = GtkBox::new(Orientation::Horizontal, 6);
            name_box.append(
                &Image::builder()
                    .resource(ext_icon_resource(ext))
                    .pixel_size(20)
                    .build(),
            );
            name_box.append(
                &Label::builder()
                    .label(&doc.display_name)
                    .width_chars(36)
                    .xalign(0.0)
                    .ellipsize(gtk4::pango::EllipsizeMode::End)
                    .build(),
            );
            row.append(&name_box);
            let date_str = doc
                .date
                .map(|d| super::disp_date(&d.to_string()))
                .unwrap_or_default();
            row.append(&Label::builder().label(&date_str).width_chars(12).xalign(0.0).build());
            let exts = doc
                .extensions
                .iter()
                .map(|e| super::ext_label(e))
                .collect::<Vec<_>>()
                .join(", ");
            row.append(&Label::builder().label(&exts).width_chars(16).xalign(0.0).build());
            let size = doc.size_hint.map(human_size).unwrap_or_default();
            row.append(&Label::builder().label(&size).width_chars(10).xalign(0.0).build());

            // Действия: «📂 Открыть» (если уже скачан) + «↻ Перекачать».
            let actions_box = GtkBox::new(Orientation::Horizontal, 4);
            if let Some(info) = &downloaded_info {
                let path = info.file_path.clone();
                let open_btn = super::icon_only_button("document-open-symbolic", "Открыть файл");
                open_btn.connect_clicked(move |_| {
                    let _ = open_file(&path);
                });
                actions_box.append(&open_btn);
            }
            // Перекачать — переотправить Download с одним документом.
            let sel = DocumentSel {
                id: doc.id.clone(),
                name: Some(doc.display_name.clone()),
                extension: doc.extensions.first().cloned(),
                date: doc.date.map(|d| d.to_string()),
            };
            let redownload_btn =
                super::icon_only_button("view-refresh-symbolic", "Перекачать (с заменой)");
            redownload_btn.connect_clicked(move |_| {
                let Some((pid, pname, rtype)) = current_target() else {
                    notify("Магазин или отчёт не выбраны.");
                    return;
                };
                let Some(cs) = CMD.with(|c| c.borrow().clone()) else {
                    return;
                };
                let token = mdwf_core::CancelToken::new();
                cs.set_cancel_token(token.clone());
                cs.send(crate::channels::UiCommand::Download {
                    provider_id: pid,
                    profile_name: pname,
                    report_type: rtype,
                    documents: vec![sel.clone()],
                    params: ReportParams::new(),
                    cancel: token,
                });
                notify("Перекачивание документа…");
            });
            actions_box.append(&redownload_btn);
            row.append(&actions_box);

            CHECKS.with(|c| {
                c.borrow_mut().push((
                    DocumentSel {
                        id: doc.id.clone(),
                        // display_name — человекочитаемое имя (поле name из WB);
                        // станет базовым именем файла на диске.
                        name: Some(doc.display_name.clone()),
                        // Первый доступный формат — предпочтительный для скачивания.
                        extension: doc.extensions.first().cloned(),
                        // Дата документа (creationTime) — для каталога/Архива/имени.
                        date: doc.date.map(|d| d.to_string()),
                    },
                    cb,
                ));
            });
            list_box.append(&row);
        }
    });
}

fn human_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Путь к иконке типа файла в gresource. Регистронезависимо: WB отдаёт
/// расширения как есть из ответа API, регистр явно не гарантирован.
/// None (нет расширения) → generic-иконка.
fn ext_icon_resource(ext: Option<&String>) -> &'static str {
    let lower = ext.map(|s| s.to_ascii_lowercase());
    match lower.as_deref() {
        Some("txt") => "/org/mdwf/icons/file-txt.png",
        Some("xlsx" | "xls" | "csv") => "/org/mdwf/icons/file-xlsx.png",
        Some("pdf") => "/org/mdwf/icons/file-pdf.png",
        Some("json") => "/org/mdwf/icons/file-json.png",
        Some("xml") => "/org/mdwf/icons/file-xml.png",
        Some("zip" | "rar" | "7z" | "gz" | "tar") => "/org/mdwf/icons/file-zip.png",
        _ => "/org/mdwf/icons/file-generic.png",
    }
}

/// Обработчик: скачивание завершено (с путями к файлам).
pub fn on_download_finished(result: &crate::channels::DownloadResult) {
    let n = result.files.len();
    if n == 0 {
        notify("Файлы не найдены.");
        return;
    }

    // Показываем пути к файлам.
    let paths_text = result
        .saved_paths
        .iter()
        .map(|p| format!("  • {p}"))
        .collect::<Vec<_>>()
        .join("\n");
    notify(&format!("✅ Скачано файлов: {n}.\n{paths_text}"));

    // Добавляем кнопку «Открыть папку» рядом с результатом.
    let result_box = W_RESULT_BOX.with(|w| w.borrow().clone());
    if let Some(rbox) = result_box {
        // Удаляем старую кнопку, если была. Кнопка — gtk4::Button (НЕ LinkButton:
        // у LinkButton открывалось 2 проводника — URI + clicked). Раньше тут
        // искали LinkButton, поэтому старые кнопки НЕ удалялись и размножались
        // после каждой загрузки, растягивая окно.
        let mut child = rbox.last_child();
        while let Some(c) = child {
            let next = c.prev_sibling();
            let is_btn = c.downcast_ref::<gtk4::Button>().is_some();
            if is_btn {
                rbox.remove(&c);
            }
            child = next;
        }

        // Кнопка «📄 Открыть файл» — открывает первый скачанный файл
        // ассоциированным приложением (Excel/PDF/…). При нескольких файлах —
        // tooltip перечисляет все пути, открывается первый.
        if let Some(first_path) = result.saved_paths.first() {
            let multi = result.saved_paths.len() > 1;
            let file = first_path.clone();
            let label = if multi {
                format!("Открыть файл (первый из {})", result.saved_paths.len())
            } else {
                "Открыть файл".to_string()
            };
            let tooltip = result.saved_paths.join("\n");
            let file_btn = super::icon_button(&label, "document-open-symbolic");
            file_btn.set_has_tooltip(true);
            file_btn.set_tooltip_text(Some(&tooltip));
            file_btn.connect_clicked(move |_| {
                let _ = open_file(&file);
            });
            rbox.append(&file_btn);
        }

        // Кнопка «📁 Открыть папку» — папка первого скачанного файла.
        if let Some(first_path) = result.saved_paths.first() {
            if let Some(parent) = std::path::Path::new(first_path).parent() {
                let folder = parent.display().to_string();
                // Обычный Button (НЕ LinkButton): у LinkButton срабатывает и
                // авто-открытие URI, и connect_clicked → открывалось 2 проводника.
                // folder-symbolic (не folder-open): рядом стоит
                // document-open-symbolic у «Открыть файл» — две «стрелки
                // открытия» неразличимы; чистая папка отличима с первого
                // взгляда и совпадает с иконкой папки в Архиве.
                let link = super::icon_button("Открыть папку", "folder-symbolic");
                link.set_has_tooltip(true);
                link.set_tooltip_text(Some(&folder));
                link.connect_clicked(move |_| {
                    let _ = open_folder(&folder);
                });
                rbox.append(&link);
            }
        }
    }
}

/// Открывает папку в проводнике (тонкая обёртка над общим хелпером views::open_folder).
fn open_folder(path: &str) -> std::io::Result<()> {
    crate::views::open_folder(path)
}

/// Открывает файл ассоциированным приложением (тонкая обёртка над views::open_file).
fn open_file(path: &str) -> std::io::Result<()> {
    crate::views::open_file(path)
}

/// Обработчик: ошибка скачивания.
pub fn on_download_error(err: &str) {
    notify(&format!("Ошибка скачивания: {err}"));
}

// Кнопка-календарь (make_date_picker + parse_date_for_calendar) вынесена в
// views/mod.rs как pub(crate) — переиспользуется вкладками «Загрузка» и «Архив».

/// Контекстная помощь вкладки «Загрузка» (кнопка «?» в заголовке).
const DOWNLOAD_HELP: &[crate::widgets::tab_help::HelpBlock] = &[
    crate::widgets::tab_help::HelpBlock::H("Порядок работы"),
    crate::widgets::tab_help::HelpBlock::B(&[
        "1) Выберите отчёт в строке «Отчёт».",
        "2) Задайте период — три способа ниже.",
        "3) Нажмите «📅 Скачать по периоду» или «📋 Список документов».",
    ]),
    crate::widgets::tab_help::HelpBlock::H("Как задать период"),
    crate::widgets::tab_help::HelpBlock::B(&[
        "Кнопка «📅 Интервал» — стандартный интервал: выберите год, вкладку (Месяц/Квартал/Полугодие/Год) и значение одним кликом.",
        "Поля «С:»/«По:» — произвольный интервал, даты в виде ГГГГ-ММ-ДД.",
        "Кнопка-календарь 📅 рядом с полем — выбор даты из календаря.",
    ]),
    crate::widgets::tab_help::HelpBlock::H("Подсказка об отчёте"),
    crate::widgets::tab_help::HelpBlock::T("Под строкой отчёта — описание и тип периода. Для месячного отчёта за квартал/год скачивание пойдёт по каждому месяцу интервала автоматически. Рядом — кнопка «🔗 Открыть в ЛК»: раздел этого отчёта в кабинете (у Wildberries ссылок нет — кнопка скрыта)."),
    crate::widgets::tab_help::HelpBlock::H("Полезно знать"),
    crate::widgets::tab_help::HelpBlock::B(&[
        "✓ у документа — уже скачан ранее; «Открыть» открывает файл, «Перекачать» скачивает заново.",
        "Повторное скачивание не создаёт дубликат — дедупликация по SHA-256.",
        "После загрузки внизу — кнопки «Открыть файл» (первый из скачанных) и «Открыть папку».",
    ]),
];
