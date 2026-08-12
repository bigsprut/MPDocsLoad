//! Конвертация JSON-ответов Ozon в Excel (.xlsx) с русскими заголовками колонок.
//!
//! Используется для отчётов, которые Ozon отдаёт JSON (а не готовым .xlsx):
//! buyout, balance, cash_flow, analytics_stocks/turnover, accrual_postings/by_day,
//! realization (сводный). Заголовки колонок — из документации docs.ozon.ru.
//!
//! `ozon.realization_posting` использует серверный Excel от Ozon
//! (/v1/report/realization/posting/create → /v1/report/info) и сюда НЕ попадает.

use rust_xlsxwriter::{Format, Workbook};

use mdwf_core::{CoreError, CoreResult};

use serde_json::Value;

/// Создаёт .xlsx из JSON-ответа для отчёта `type_id`. Возвращает байты файла.
///
/// Диспетчер: по type_id выбирает пер-отчётный шаблон (колонки + расположение
/// массива строк в JSON). Для неизвестного type_id — fallback: пробует найти
/// любой массив объектов в ответе и вывести его как лист.
pub fn workbook_bytes(type_id: &str, json: &Value) -> CoreResult<Vec<u8>> {
    let mut wb = Workbook::new();
    match type_id {
        "ozon.buyout" => sheet_from_array_report(&mut wb, "Выкупы", json, "products", headers_buyout()),
        "ozon.analytics_turnover" => sheet_from_array_report(&mut wb, "Оборачиваемость", json, "items", headers_analytics_turnover()),
        "ozon.analytics_stocks" => sheet_from_array_report(&mut wb, "Остатки", json, "items", headers_analytics_stocks()),
        "ozon.cash_flow" => sheet_from_array_report(&mut wb, "Движение средств", json, "items", headers_cash_flow()),
        "ozon.realization" => sheet_from_array_report(&mut wb, "Реализация", json, "result.rows", headers_realization()),
        "ozon.returns" => sheet_from_array_report(&mut wb, "Возвраты", json, "returns", headers_returns()),
        "ozon.accrual_postings" => sheet_accrual_postings(&mut wb, json),
        "ozon.accrual_by_day" => sheet_accrual_by_day(&mut wb, json),
        "ozon.balance" => workbook_balance(&mut wb, json),
        _ => {
            // Fallback: ищем первый массив объектов в JSON.
            if let Some((path, arr)) = find_first_array(json) {
                let headers = infer_headers_vec(arr);
                sheet_from_array_report(&mut wb, "Отчёт", json, &path, &headers)?;
            } else {
                // Нет массива — выводим как key/value.
                sheet_key_value(&mut wb, "Отчёт", json)?;
            }
            Ok(())
        }
    }?;
    wb.save_to_buffer()
        .map_err(|e| CoreError::Internal(format!("xlsx write: {e}")))
}

// ===== Заголовки колонок (русские, из docs.ozon.ru) =====
// Кортеж: (путь поля в JSON через точку, русский заголовок).

type Headers = &'static [(&'static str, &'static str)];

fn headers_buyout() -> Headers {
    &[
        ("name", "Название товара"),
        ("offer_id", "Артикул"),
        ("sku", "SKU"),
        ("posting_number", "Номер отправления"),
        ("seller_price_per_instance", "Цена продавца со скидкой"),
        ("deduction_by_category_percent", "Скидка по категории, %"),
        ("buyout_price", "Цена выкупа"),
        ("vat_percent", "Ставка НДС, %"),
        ("quantity", "Количество"),
        ("amount", "Сумма к начислению"),
    ]
}

/// Колонки для /v1/returns/list (возвраты FBO+FBS). Поля вложены (product.*,
/// visual.status.*, logistic.*, place.*, compensation_status.*, storage.*).
/// extract_path резолвит точечные пути. Источник: docs.ozon.ru.
fn headers_returns() -> Headers {
    &[
        ("posting_number", "Номер отправления"),
        ("order_number", "Номер заказа"),
        ("schema", "Схема"),
        ("type", "Тип возврата"),
        ("product.sku", "SKU"),
        ("product.offer_id", "Артикул"),
        ("product.name", "Наименование товара"),
        ("product.quantity", "Количество"),
        ("product.price.price", "Цена"),
        ("return_reason_name", "Причина возврата"),
        ("visual.status.display_name", "Статус возврата"),
        ("visual.status.sys_name", "Статус (код)"),
        ("logistic.return_date", "Дата возврата"),
        ("visual.change_moment", "Дата изменения статуса"),
        ("place.name", "Склад (место)"),
        ("compensation_status.status.display_name", "Компенсация"),
        ("storage.days", "Дней хранения"),
        ("storage.sum.price", "Сумма хранения"),
        ("logistic.barcode", "Штрихкод"),
        ("id", "ID возврата"),
    ]
}

fn headers_analytics_turnover() -> Headers {
    &[
        ("ads", "Среднедневные продажи (60 дн)"),
        ("current_stock", "Текущий остаток, шт"),
        ("idc", "IDC (дни остатка)"),
        ("idc_grade", "Уровень остатка"),
        ("name", "Название товара"),
        ("offer_id", "Артикул"),
        ("sku", "SKU"),
        ("turnover", "Оборачиваемость, дни"),
        ("turnover_grade", "Уровень оборачиваемости"),
    ]
}

fn headers_analytics_stocks() -> Headers {
    &[
        ("ads", "Среднедневные продажи (все кластеры)"),
        ("ads_cluster", "Среднедневные продажи (кластер)"),
        ("available_stock_count", "Доступно к продаже"),
        ("cluster_id", "ID кластера"),
        ("cluster_name", "Кластер"),
        ("days_without_sales", "Дней без продаж (все кластеры)"),
        ("days_without_sales_cluster", "Дней без продаж (кластер)"),
        ("excess_stock_count", "Излишки с поставки"),
        ("expiring_stock_count", "С истекающим сроком годности"),
        ("idc", "IDC (все кластеры)"),
        ("idc_cluster", "IDC (кластер)"),
        ("item_tags", "Теги товара"),
        ("macrolocal_cluster_id", "ID макролокального кластера"),
        ("name", "Название товара"),
        ("offer_id", "Артикул"),
        ("other_stock_count", "На проверке"),
        ("requested_stock_count", "В заявках на поставку"),
        ("return_from_customer_stock_count", "Возвраты от покупателей"),
        ("return_to_seller_stock_count", "Готовится к вывозу"),
        ("sku", "SKU"),
        ("stock_defect_stock_count", "Брак со стока"),
        ("transit_defect_stock_count", "Брак с поставки"),
        ("transit_stock_count", "В пути"),
        ("turnover_grade", "Ликвидность (все кластеры)"),
        ("turnover_grade_cluster", "Ликвидность (кластер)"),
        ("valid_stock_count", "Готовим к продаже"),
        ("waiting_docs_stock_count", "Ожидает действий (маркировка)"),
        ("warehouse_id", "ID склада"),
        ("warehouse_name", "Склад"),
    ]
}

fn headers_cash_flow() -> Headers {
    &[
        ("period.begin", "Начало периода"),
        ("period.end", "Конец периода"),
        ("period.id", "ID периода"),
        ("orders_amount", "Сумма заказов"),
        ("returns_amount", "Сумма возвратов"),
        ("item_delivery_and_return_amount", "Доставка и возврат товаров"),
        ("commission_amount", "Комиссия"),
        ("services_amount", "Услуги"),
        ("currency_code", "Валюта"),
    ]
}

fn headers_realization() -> Headers {
    &[
        ("rowNumber", "№ строки"),
        ("item.barcode", "Штрихкод"),
        ("item.name", "Название товара"),
        ("item.offer_id", "Артикул"),
        ("item.sku", "SKU"),
        ("seller_price_per_instance", "Цена продавца за единицу"),
        ("commission_ratio", "Коэффициент комиссии"),
        // delivery_commission.* — префикс «Доставка / »
        ("delivery_commission.amount", "Доставка / Сумма"),
        ("delivery_commission.price_per_instance", "Доставка / Цена за единицу"),
        ("delivery_commission.quantity", "Доставка / Количество"),
        ("delivery_commission.commission", "Доставка / Комиссия"),
        ("delivery_commission.standard_fee", "Доставка / Стандартная комиссия"),
        ("delivery_commission.bonus", "Доставка / Скидки за баллы"),
        ("delivery_commission.compensation", "Доставка / Компенсации"),
        ("delivery_commission.bank_coinvestment", "Доставка / Софинансирование банка"),
        ("delivery_commission.pick_up_point_coinvestment", "Доставка / Софинансирование ПВЗ"),
        ("delivery_commission.stars", "Доставка / Кешбэк звёздами"),
        ("delivery_commission.total", "Доставка / Итого"),
        // return_commission.* — префикс «Возврат / »
        ("return_commission.amount", "Возврат / Сумма"),
        ("return_commission.price_per_instance", "Возврат / Цена за единицу"),
        ("return_commission.quantity", "Возврат / Количество"),
        ("return_commission.commission", "Возврат / Комиссия"),
        ("return_commission.standard_fee", "Возврат / Стандартная комиссия"),
        ("return_commission.bonus", "Возврат / Скидки за баллы"),
        ("return_commission.compensation", "Возврат / Компенсации"),
        ("return_commission.bank_coinvestment", "Возврат / Софинансирование банка"),
        ("return_commission.pick_up_point_coinvestment", "Возврат / Софинансирование ПВЗ"),
        ("return_commission.stars", "Возврат / Кешбэк звёздами"),
        ("return_commission.total", "Возврат / Итого"),
    ]
}

// ===== Построение листа из массива объектов =====

/// Находит массив по точечному пути (напр. "result.rows", "products", "items")
/// и пишет лист: строка 0 — русские заголовки (bold), далее — строки данных.
/// `headers` — любое время жизни (static словари или вычисляемые для fallback).
fn sheet_from_array_report(
    wb: &mut Workbook,
    sheet_title: &str,
    json: &Value,
    array_path: &str,
    headers: &[(&str, &str)],
) -> CoreResult<()> {
    let sheet = wb.add_worksheet();
    sheet
        .set_name(sheet_title)
        .map_err(|e| CoreError::Internal(format!("xlsx sheet name: {e}")))?;
    let bold = Format::new().set_bold();

    // Заголовки.
    for (col, (_, title)) in headers.iter().enumerate() {
        sheet
            .write_string_with_format(0, col as u16, *title, &bold)
            .map_err(|e| CoreError::Internal(format!("xlsx header: {e}")))?;
    }

    // Данные.
    let rows = array_at_path(json, array_path);
    let mut row_idx = 1u32;
    for row_val in rows {
        for (col, (path, _)) in headers.iter().enumerate() {
            let cell = extract_path(row_val, path);
            write_cell(sheet, row_idx, col as u16, cell.as_deref());
        }
        row_idx += 1;
    }
    sheet.set_freeze_panes(1, 0).ok();
    let _ = sheet.autofit();
    Ok(())
}

// ===== accrual_postings: денормализация (posting_accruals[].accruals[]) =====

fn sheet_accrual_postings(wb: &mut Workbook, json: &Value) -> CoreResult<()> {
    let sheet = wb.add_worksheet();
    sheet
        .set_name("Начисления по отправлениям")
        .map_err(|e| CoreError::Internal(format!("xlsx sheet name: {e}")))?;
    let bold = Format::new().set_bold();
    let headers: &[(&str, &str)] = &[
        ("posting_number", "Номер отправления"),
        ("accrual_date", "Дата начисления"),
        ("type_id", "Тип начисления"),
        ("sku", "SKU"),
        ("quantity", "Количество"),
        ("seller_price.amount", "Цена продавца"),
        ("seller_price.currency", "Валюта цены"),
        ("accrued.amount", "Начислено"),
        ("accrued.currency", "Валюта начисления"),
    ];
    for (col, (_, title)) in headers.iter().enumerate() {
        sheet
            .write_string_with_format(0, col as u16, *title, &bold)
            .map_err(|e| CoreError::Internal(format!("xlsx header: {e}")))?;
    }
    // Денормализация: один posting → несколько строк (по числу accruals).
    let postings = array_at_path(json, "posting_accruals");
    let mut row_idx = 1u32;
    for post in postings {
        let posting_number = extract_path(post, "posting_number").unwrap_or_default();
        let accruals = post.get("accruals").and_then(|v| v.as_array());
        if let Some(arr) = accruals {
            for acc in arr {
                // posting_number — из родителя.
                sheet
                    .write_string(row_idx, 0, &posting_number)
                    .map_err(|e| CoreError::Internal(format!("xlsx cell: {e}")))?;
                for (col, (path, _)) in headers.iter().enumerate().skip(1) {
                    let cell = extract_path(acc, path);
                    write_cell(sheet, row_idx, col as u16, cell.as_deref());
                }
                row_idx += 1;
            }
        }
    }
    sheet.set_freeze_panes(1, 0).ok();
    let _ = sheet.autofit();
    Ok(())
}

// ===== accrual_by_day: multi-sheet (Начисления + Сборы) =====

fn sheet_accrual_by_day(wb: &mut Workbook, json: &Value) -> CoreResult<()> {
    let accruals = array_at_path(json, "accruals");

    // Лист 1: «Начисления» — основные поля.
    let s1 = wb.add_worksheet();
    s1.set_name("Начисления")
        .map_err(|e| CoreError::Internal(format!("xlsx: {e}")))?;
    let bold = Format::new().set_bold();
    let h1: &[(&str, &str)] = &[
        ("accrual_id", "ID начисления"),
        ("date", "Дата"),
        ("unit_number", "Номер единицы"),
        ("accrued_category", "Категория начисления"),
        ("total_amount.amount", "Общая сумма"),
        ("total_amount.currency", "Валюта"),
    ];
    for (col, (_, t)) in h1.iter().enumerate() {
        s1.write_string_with_format(0, col as u16, *t, &bold)
            .map_err(|e| CoreError::Internal(format!("xlsx: {e}")))?;
    }
    let mut row_idx = 1u32;
    for acc in &accruals {
        for (col, (path, _)) in h1.iter().enumerate() {
            let cell = extract_path(acc, path);
            write_cell(s1, row_idx, col as u16, cell.as_deref());
        }
        row_idx += 1;
    }
    s1.set_freeze_panes(1, 0).ok();
    let _ = s1.autofit();

    // Лист 2: «Сборы» — денормализованные item_fees.fees[].fees[].
    let s2 = wb.add_worksheet();
    s2.set_name("Сборы")
        .map_err(|e| CoreError::Internal(format!("xlsx: {e}")))?;
    let h2: &[(&str, &str)] = &[
        ("_accrual_id", "ID начисления"),
        ("sku", "SKU"),
        ("type_id", "Тип сбора"),
        ("accrued.amount", "Начислено"),
        ("accrued.currency", "Валюта"),
    ];
    for (col, (_, t)) in h2.iter().enumerate() {
        s2.write_string_with_format(0, col as u16, *t, &bold)
            .map_err(|e| CoreError::Internal(format!("xlsx: {e}")))?;
    }
    let mut row_idx = 1u32;
    for acc in &accruals {
        let accrual_id = extract_path(acc, "accrual_id").unwrap_or_default();
        // item_fees.fees[] — каждый {sku, fees:[{type_id, accrued}]}
        let fees_outer = acc.get("item_fees").and_then(|f| f.get("fees")).and_then(|v| v.as_array());
        if let Some(outer) = fees_outer {
            for group in outer {
                let sku = extract_path(group, "sku").unwrap_or_default();
                let inner = group.get("fees").and_then(|v| v.as_array());
                if let Some(inner_arr) = inner {
                    for fee in inner_arr {
                        s2.write_string(row_idx, 0, &accrual_id)
                            .map_err(|e| CoreError::Internal(format!("xlsx: {e}")))?;
                        s2.write_string(row_idx, 1, &sku)
                            .map_err(|e| CoreError::Internal(format!("xlsx: {e}")))?;
                        let type_id = extract_path(fee, "type_id");
                        write_cell(s2, row_idx, 2, type_id.as_deref());
                        let amt = extract_path(fee, "accrued.amount");
                        write_cell(s2, row_idx, 3, amt.as_deref());
                        let cur = extract_path(fee, "accrued.currency");
                        write_cell(s2, row_idx, 4, cur.as_deref());
                        row_idx += 1;
                    }
                }
            }
        }
        // non_item_fee — тоже в «Сборы».
        if let Some(nif) = acc.get("non_item_fee") {
            s2.write_string(row_idx, 0, &accrual_id)
                .map_err(|e| CoreError::Internal(format!("xlsx: {e}")))?;
            s2.write_string(row_idx, 1, "").ok();
            let type_id = extract_path(nif, "type_id");
            write_cell(s2, row_idx, 2, type_id.as_deref());
            let amt = extract_path(nif, "accrued.amount");
            write_cell(s2, row_idx, 3, amt.as_deref());
            let cur = extract_path(nif, "accrued.currency");
            write_cell(s2, row_idx, 4, cur.as_deref());
            row_idx += 1;
        }
    }
    s2.set_freeze_panes(1, 0).ok();
    let _ = s2.autofit();
    Ok(())
}

// ===== balance: 3 листа (Доходы/расходы, Услуги, Итоги) =====

fn workbook_balance(wb: &mut Workbook, json: &Value) -> CoreResult<()> {
    let bold = Format::new().set_bold();
    let cashflows = json.get("cashflows").cloned().unwrap_or(Value::Null);
    let total = json.get("total").cloned().unwrap_or(Value::Null);

    // Лист 1: «Доходы и расходы» — returns/sales.
    let s1 = wb.add_worksheet();
    s1.set_name("Доходы и расходы")
        .map_err(|e| CoreError::Internal(format!("xlsx: {e}")))?;
    let h1: &[(&str, &str)] = &[
        ("_section", "Раздел"),
        ("amount.value", "Сумма"),
        ("amount.currency_code", "Валюта"),
        ("fee.value", "Комиссия"),
        ("fee.currency_code", "Валюта комиссии"),
        ("amount_details.revenue.value", "Выручка"),
        ("amount_details.revenue.currency_code", "Валюта выручки"),
        ("amount_details.partner_programs.value", "Партнёрские программы"),
        ("amount_details.partner_programs.currency_code", "Валюта"),
        ("amount_details.points_for_discounts", "Баллы для скидок"),
    ];
    for (col, (_, t)) in h1.iter().enumerate() {
        s1.write_string_with_format(0, col as u16, *t, &bold)
            .map_err(|e| CoreError::Internal(format!("xlsx: {e}")))?;
    }
    let mut row_idx = 1u32;
    for (key, label) in [("returns", "Возвраты"), ("sales", "Продажи")] {
        if let Some(sec) = cashflows.get(key) {
            s1.write_string(row_idx, 0, label).ok();
            for (col, (path, _)) in h1.iter().enumerate().skip(1) {
                let cell = extract_path(sec, path);
                write_cell(s1, row_idx, col as u16, cell.as_deref());
            }
            row_idx += 1;
        }
    }
    s1.set_freeze_panes(1, 0).ok();
    let _ = s1.autofit();

    // Лист 2: «Услуги» — services[].
    let s2 = wb.add_worksheet();
    s2.set_name("Услуги")
        .map_err(|e| CoreError::Internal(format!("xlsx: {e}")))?;
    let h2: &[(&str, &str)] = &[
        ("name", "Услуга"),
        ("amount.value", "Сумма"),
        ("amount.currency_code", "Валюта"),
    ];
    for (col, (_, t)) in h2.iter().enumerate() {
        s2.write_string_with_format(0, col as u16, *t, &bold)
            .map_err(|e| CoreError::Internal(format!("xlsx: {e}")))?;
    }
    let mut row_idx = 1u32;
    if let Some(services) = cashflows.get("services").and_then(|v| v.as_array()) {
        for svc in services {
            for (col, (path, _)) in h2.iter().enumerate() {
                let cell = extract_path(svc, path);
                write_cell(s2, row_idx, col as u16, cell.as_deref());
            }
            row_idx += 1;
        }
    }
    s2.set_freeze_panes(1, 0).ok();
    let _ = s2.autofit();

    // Лист 3: «Итоги» — total.
    let s3 = wb.add_worksheet();
    s3.set_name("Итоги")
        .map_err(|e| CoreError::Internal(format!("xlsx: {e}")))?;
    let h3: &[(&str, &str)] = &[
        ("_field", "Показатель"),
        ("_value", "Сумма"),
        ("_currency", "Валюта"),
    ];
    for (col, (_, t)) in h3.iter().enumerate() {
        s3.write_string_with_format(0, col as u16, *t, &bold)
            .map_err(|e| CoreError::Internal(format!("xlsx: {e}")))?;
    }
    let mut row_idx = 1u32;
    for (key, label) in [
        ("opening_balance", "Входящий остаток"),
        ("accrued", "Начислено"),
        ("closing_balance", "Исходящий остаток"),
    ] {
        if let Some(sec) = total.get(key) {
            s3.write_string(row_idx, 0, label).ok();
            let v = extract_path(sec, "value");
            let c = extract_path(sec, "currency_code");
            write_cell(s3, row_idx, 1, v.as_deref());
            write_cell(s3, row_idx, 2, c.as_deref());
            row_idx += 1;
        }
    }
    // payments[] — построчно.
    if let Some(payments) = total.get("payments").and_then(|v| v.as_array()) {
        for p in payments {
            s3.write_string(row_idx, 0, "Платёж").ok();
            let v = extract_path(p, "value");
            let c = extract_path(p, "currency_code");
            write_cell(s3, row_idx, 1, v.as_deref());
            write_cell(s3, row_idx, 2, c.as_deref());
            row_idx += 1;
        }
    }
    s3.set_freeze_panes(1, 0).ok();
    let _ = s3.autofit();
    Ok(())
}

// ===== Хелперы =====

/// Извлекает значение по точечному пути ("a.b.c") из JSON-объекта.
/// Возвращает строку (числа/булевы → строковое представление). Для массивов
/// объектов возвращает пусто; для массива скаляров — CSV.
fn extract_path(value: &Value, path: &str) -> Option<String> {
    let mut cur = value;
    for part in path.split('.') {
        cur = cur.get(part)?;
    }
    Some(value_to_cell(cur))
}

/// Находит массив по точечному пути. Возвращает срез элементов.
fn array_at_path<'a>(json: &'a Value, path: &str) -> Vec<&'a Value> {
    let mut cur = json;
    for part in path.split('.') {
        cur = match cur.get(part) {
            Some(v) => v,
            None => return Vec::new(),
        };
    }
    cur.as_array().map(|a| a.iter().collect()).unwrap_or_default()
}

/// Преобразует значение JSON в строку ячейки. null/объект → пусто.
fn value_to_cell(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(arr) => arr
            .iter()
            .map(value_to_cell)
            .collect::<Vec<_>>()
            .join(", "),
        // null и вложенные объекты — пустая ячейка (объекты разворачивает extract_path).
        Value::Null | Value::Object(_) => String::new(),
    }
}

/// Пишет строку в ячейку. Пустое значение → пустая ячейка.
fn write_cell(sheet: &mut rust_xlsxwriter::Worksheet, row: u32, col: u16, text: Option<&str>) {
    if let Some(t) = text {
        if !t.is_empty() {
            // Пишем как строку (даже числа) — надёжно, без угадывания типа.
            let _ = sheet.write_string(row, col, t);
        }
    }
}

/// Fallback: рекурсивно ищет первый массив объектов в JSON. Возвращает (путь, массив).
fn find_first_array(json: &Value) -> Option<(String, &Vec<Value>)> {
    fn walk(v: &Value, path: String) -> Option<(String, &Vec<Value>)> {
        if let Some(arr) = v.as_array() {
            if arr.iter().all(Value::is_object) {
                return Some((path, arr));
            }
        }
        if let Some(obj) = v.as_object() {
            for (k, val) in obj {
                let p = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                if let Some(found) = walk(val, p) {
                    return Some(found);
                }
            }
        }
        None
    }
    walk(json, String::new())
}

/// Fallback: выводит любой объект как два столбца (ключ — значение).
fn sheet_key_value(wb: &mut Workbook, title: &str, json: &Value) -> CoreResult<()> {
    let sheet = wb.add_worksheet();
    sheet
        .set_name(title)
        .map_err(|e| CoreError::Internal(format!("xlsx: {e}")))?;
    let bold = Format::new().set_bold();
    let _ = sheet.write_string_with_format(0, 0, "Поле", &bold);
    let _ = sheet.write_string_with_format(0, 1, "Значение", &bold);
    let mut row = 1u32;
    if let Some(obj) = json.as_object() {
        for (k, v) in obj {
            let _ = sheet.write_string(row, 0, k);
            let _ = sheet.write_string(row, 1, value_to_cell(v));
            row += 1;
        }
    }
    sheet.set_freeze_panes(1, 0).ok();
    let _ = sheet.autofit();
    Ok(())
}

/// Fallback: выводит заголовки из union ключей массива объектов.
fn infer_headers(arr: &[&Value]) -> Vec<(&'static str, &'static str)> {
    // Собираем union ключей; path = имя поля, заголовок = оно же.
    // Используем leak, чтобы получить 'static (модуль живёт всё время работы).
    let mut keys: Vec<String> = Vec::new();
    for row in arr {
        if let Some(obj) = row.as_object() {
            for k in obj.keys() {
                if !keys.contains(k) {
                    keys.push(k.clone());
                }
            }
        }
    }
    keys.into_iter()
        .map(|k| {
            let leaked: &'static str = Box::leak(k.into_boxed_str());
            (leaked, leaked)
        })
        .collect()
}

#[allow(clippy::ptr_arg)]
fn infer_headers_vec(arr: &Vec<Value>) -> Vec<(&'static str, &'static str)> {
    let refs: Vec<&Value> = arr.iter().collect();
    infer_headers(&refs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn buyout_produces_xlsx() {
        let j = json!({
            "products": [
                {"name":"Футболка","offer_id":"A1","sku":"100","posting_number":"P1",
                 "seller_price_per_instance":100.0,"deduction_by_category_percent":"3.0",
                 "buyout_price":97.0,"vat_percent":20,"quantity":1,"amount":97.0}
            ]
        });
        let bytes = workbook_bytes("ozon.buyout", &j).unwrap();
        assert!(bytes.len() > 1000, "xlsx too small");
        // ZIP-сигнатура.
        assert_eq!(&bytes[0..2], &[0x50, 0x4B], "not a zip/xlsx");
    }

    #[test]
    fn balance_produces_3_sheets() {
        let j = json!({
            "cashflows": {
                "returns": {"amount":{"value":10.0,"currency_code":"RUB"},"fee":{"value":1.0,"currency_code":"RUB"},
                            "amount_details":{"revenue":{"value":9.0,"currency_code":"RUB"},
                                              "partner_programs":{"value":0.0,"currency_code":"RUB"},
                                              "points_for_discounts":"0"}},
                "sales": {"amount":{"value":100.0,"currency_code":"RUB"},"fee":{"value":5.0,"currency_code":"RUB"},
                          "amount_details":{"revenue":{"value":95.0,"currency_code":"RUB"},
                                            "partner_programs":{"value":0.0,"currency_code":"RUB"},
                                            "points_for_discounts":"0"}},
                "services":[{"name":"Логистика","amount":{"value":2.0,"currency_code":"RUB"}}]
            },
            "total": {"opening_balance":{"value":1000.0,"currency_code":"RUB"},
                      "accrued":{"value":50.0,"currency_code":"RUB"},
                      "closing_balance":{"value":1050.0,"currency_code":"RUB"},
                      "payments":[{"value":50.0,"currency_code":"RUB"}]}
        });
        let bytes = workbook_bytes("ozon.balance", &j).unwrap();
        // workbook с 3 листами должен быть заметно больше минимального.
        assert!(bytes.len() > 2000, "xlsx balance too small");
    }

    #[test]
    fn accrual_postings_denormalization() {
        let j = json!({
            "posting_accruals": [
                {"posting_number":"P1","accruals":[
                    {"accrual_date":"2026-07-01","type_id":1,"sku":100,"quantity":1,
                     "seller_price":{"amount":"100","currency":"RUB"},
                     "accrued":{"amount":"97","currency":"RUB"}},
                    {"accrual_date":"2026-07-02","type_id":2,"sku":100,"quantity":1,
                     "seller_price":{"amount":"100","currency":"RUB"},
                     "accrued":{"amount":"97","currency":"RUB"}}
                ]}
            ]
        });
        let bytes = workbook_bytes("ozon.accrual_postings", &j).unwrap();
        assert!(bytes.len() > 1000);
    }

    #[test]
    fn extract_path_nested() {
        let v = json!({"a":{"b":{"c":42}}});
        assert_eq!(extract_path(&v, "a.b.c"), Some("42".into()));
        assert_eq!(extract_path(&v, "a.x"), None);
    }
}
