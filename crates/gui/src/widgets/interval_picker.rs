//! Виджет выбора стандартного интервала: неделя / месяц / квартал / год.
//!
//! Layout: сверху выбор года (SpinButton — текст + стрелки ±1), ниже StackSwitcher
//! с ярлычками «Неделя / Месяц / Квартал / Год» и Stack; каждая вкладка — сетка
//! (FlowBox) кнопок выбираемых значений. Один клик по значению → расчёт диапазона
//! `[date_from, date_to]` и вызов `on_select`.
//!
//! Кнопка «📅 Интервал» в download.rs открывает этот виджет в popover; on_select
//! проставляет date_from/date_to.

use std::rc::Rc;

use chrono::{Datelike, NaiveDate, Weekday};

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Align, Button, FlowBox, Label, Orientation, SelectionMode, SpinButton, Stack, StackSwitcher,
};

/// Доступные размеры сетки года для быстрого выбора (текущий ±5).
const YEAR_RANGE: i32 = 5;
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

/// Колбэк выбора интервала: (from, to) строками `"ДД.ММ.ГГГГ"` (формат полей дат).
type SelectFn = Rc<dyn Fn(&str, &str)>;

/// Строит виджет выбора стандартного интервала.
/// `on_select(from, to)` вызывается со строками `"ДД.ММ.ГГГГ"` при клике на конкретный
/// интервал (неделя/месяц/квартал/год) в активной вкладке.
#[must_use]
pub fn make_interval_picker<F: Fn(&str, &str) + 'static>(on_select: F) -> gtk4::Widget {
    let on_select: SelectFn = Rc::new(on_select);
    let cur_year = chrono::Local::now().date_naive().year();

    let root = gtk4::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();

    // --- Год (SpinButton: текст + стрелки ±1) ---
    let year_row = gtk4::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .halign(Align::Center)
        .build();
    year_row.append(&Label::new(Some("Год:")));
    let spin = SpinButton::with_range(
        f64::from(cur_year - YEAR_RANGE),
        f64::from(cur_year + YEAR_RANGE),
        1.0,
    );
    spin.set_value(f64::from(cur_year));
    spin.set_digits(0);
    spin.set_numeric(true);
    spin.set_snap_to_ticks(true);
    spin.set_width_chars(6);
    year_row.append(&spin);
    root.append(&year_row);

    // --- Stack + Switcher (вкладки) ---
    let stack = Stack::builder().build();
    let month_grid = grid_of_months(&on_select, &spin);
    let quarter_grid = grid_of_quarters(&on_select, &spin);
    let year_grid = grid_of_years(&on_select, &spin, cur_year);
    // Неделя — пересобирается при смене года (число недель 52 или 53).
    let week_grid = FlowBox::builder()
        .selection_mode(SelectionMode::None)
        .max_children_per_line(7)
        .build();
    rebuild_weeks(&week_grid, cur_year, &on_select, &spin);
    {
        let week_grid_w = week_grid.clone();
        let spin_w = spin.clone();
        let on_select_w = Rc::clone(&on_select);
        spin.connect_value_changed(move |_| {
            let y = spin_w.value() as i32;
            rebuild_weeks(&week_grid_w, y, &on_select_w, &spin_w);
        });
    }

    stack.add_titled(&week_grid, Some("week"), "Неделя");
    stack.add_titled(&month_grid, Some("month"), "Месяц");
    stack.add_titled(&quarter_grid, Some("quarter"), "Квартал");
    stack.add_titled(&year_grid, Some("year"), "Год");

    let switcher = StackSwitcher::builder().stack(&stack).build();
    root.append(&switcher);
    root.append(&stack);

    root.upcast::<gtk4::Widget>()
}

// ============ Сетки значений ============

/// Сетка из кнопок; `items` — (надпись, индекс), `on_idx` — реакция на клик.
fn grid_of(items: &[(String, u32)], on_idx: Rc<dyn Fn(u32)>, max_per_line: u32) -> FlowBox {
    let fb = FlowBox::builder()
        .selection_mode(SelectionMode::None)
        .max_children_per_line(max_per_line)
        .homogeneous(true)
        .build();
    for (label, idx) in items {
        let idx = *idx; // owned u32 (иначе замыкание захватило бы &u32 из items)
        let btn = Button::builder().label(label.clone()).build();
        let on_idx = Rc::clone(&on_idx);
        btn.connect_clicked(move |_| on_idx(idx));
        fb.insert(&btn, -1);
    }
    fb
}

fn grid_of_months(on_select: &SelectFn, spin: &SpinButton) -> FlowBox {
    let items: Vec<(String, u32)> = MONTH_NAMES
        .iter()
        .enumerate()
        .map(|(i, n)| ((*n).to_string(), i as u32 + 1))
        .collect();
    let spin = spin.clone();
    let on_select = Rc::clone(on_select);
    grid_of(
        &items,
        Rc::new(move |m| {
            let y = spin.value() as i32;
            let (f, t) = month_range(y, m);
            on_select(&fmt(f), &fmt(t));
        }),
        4,
    )
}

fn grid_of_quarters(on_select: &SelectFn, spin: &SpinButton) -> FlowBox {
    let items: Vec<(String, u32)> = (1..=4).map(|q| (format!("{q} кв."), q)).collect();
    let spin = spin.clone();
    let on_select = Rc::clone(on_select);
    grid_of(
        &items,
        Rc::new(move |q| {
            let y = spin.value() as i32;
            let (f, t) = quarter_range(y, q);
            on_select(&fmt(f), &fmt(t));
        }),
        4,
    )
}

fn grid_of_years(on_select: &SelectFn, spin: &SpinButton, cur_year: i32) -> FlowBox {
    let items: Vec<(String, u32)> = ((cur_year - YEAR_RANGE)..=(cur_year + YEAR_RANGE))
        .map(|y| (y.to_string(), y as u32))
        .collect();
    let spin = spin.clone();
    let on_select = Rc::clone(on_select);
    grid_of(
        &items,
        Rc::new(move |yu| {
            let y = yu as i32;
            // Клик по году в списке — выставить спиннер и применить весь год.
            spin.set_value(f64::from(y));
            let (f, t) = year_range(y);
            on_select(&fmt(f), &fmt(t));
        }),
        4,
    )
}

/// Пересобирает сетку недель для указанного года (52 или 53 ISO-недели).
fn rebuild_weeks(
    fb: &FlowBox,
    year: i32,
    on_select: &SelectFn,
    _spin: &SpinButton,
) {
    // FlowBox не имеет remove_all — чистим итеративно через first_child()/remove().
    while let Some(child) = fb.first_child() {
        fb.remove(&child);
    }
    let n = iso_weeks_in_year(year);
    for w in 1..=n {
        // Диапазон дат недели — в tooltip, чтобы «Н37» было понятно без гадания.
        let (wf, wt) = week_range(year, w);
        let btn = Button::builder()
            .label(format!("Н{w}"))
            .tooltip_text(format!(
                "Неделя {w} ({}–{})",
                wf.format("%d.%m"),
                wt.format("%d.%m")
            ))
            .build();
        let on_select = Rc::clone(on_select);
        btn.connect_clicked(move |_| {
            on_select(&fmt(wf), &fmt(wt));
        });
        fb.insert(&btn, -1);
    }
}

// ============ Математика дат (chrono) ============

fn fmt(d: NaiveDate) -> String {
    // Поля дат показывают ДД.ММ.ГГГГ; читатели конвертируют в ISO для API.
    d.format("%d.%m.%Y").to_string()
}

/// Первый и последний день месяца `month` в году `year`.
fn month_range(year: i32, month: u32) -> (NaiveDate, NaiveDate) {
    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap_or_else(|| {
        NaiveDate::from_ymd_opt(year, 1, 1).expect("valid fallback date")
    });
    let last = first
        .checked_add_months(chrono::Months::new(1))
        .map_or(first, |d| d.pred_opt().unwrap_or(d));
    (first, last)
}

/// Диапазон квартала `q` (1..=4) в году `year`.
fn quarter_range(year: i32, q: u32) -> (NaiveDate, NaiveDate) {
    let start_m = (q - 1) * 3 + 1;
    let end_m = start_m + 2;
    let (f, _) = month_range(year, start_m);
    let (_, t) = month_range(year, end_m);
    (f, t)
}

/// Весь год: 1 января .. 31 декабря.
fn year_range(year: i32) -> (NaiveDate, NaiveDate) {
    let f = NaiveDate::from_ymd_opt(year, 1, 1).expect("valid year");
    let t = NaiveDate::from_ymd_opt(year, 12, 31).expect("valid year");
    (f, t)
}

/// Понедельник .. воскресенье ISO-недели `week` в году `year`.
fn week_range(year: i32, week: u32) -> (NaiveDate, NaiveDate) {
    // from_isoywd_opt требует i32-неделю; недействительную неделю «прижимаем» к 1.
    let mon = NaiveDate::from_isoywd_opt(year, week, Weekday::Mon)
        .or_else(|| NaiveDate::from_isoywd_opt(year, 1, Weekday::Mon))
        .expect("valid ISO week");
    let sun = mon + chrono::Duration::days(6);
    (mon, sun)
}

/// Число ISO-недель в году (52 или 53). 28 декабря всегда в последней ISO-недели.
fn iso_weeks_in_year(year: i32) -> u32 {
    NaiveDate::from_ymd_opt(year, 12, 28)
        .map_or(52, |d| d.iso_week().week())
}

// Подавление неиспользуемого импорта glib (нужен для trait-ов виджетов косвенно).
#[allow(unused_imports)]
use glib as _glib;
