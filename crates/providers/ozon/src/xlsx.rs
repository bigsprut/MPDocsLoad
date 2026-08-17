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
        "ozon.analytics_turnover" => sheet_from_array_report(&mut wb, "Оборотная ведомость", json, "items", headers_analytics_turnover()),
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
    for (i, row_val) in rows.into_iter().enumerate() {
        let row_idx = (i + 1) as u32;
        for (col, (path, _)) in headers.iter().enumerate() {
            let cell = extract_path(row_val, path);
            write_cell(sheet, row_idx, col as u16, cell.as_deref());
        }
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
    for (i, acc) in accruals.iter().enumerate() {
        let row_idx = (i + 1) as u32;
        for (col, (path, _)) in h1.iter().enumerate() {
            let cell = extract_path(acc, path);
            write_cell(s1, row_idx, col as u16, cell.as_deref());
        }
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

// ===== Фоллбэк «Отчёта об отправлениях» (данные /v3/posting/fbo/list) =====

/// Лист «Отправления» из полных данных /v3/posting/fbo/list: денормализация
/// posting × products — по строке на товар внутри отправления (как CSV Ozon).
/// Финансы (старая цена) подтягиваются из financial_data.products по product_id.
pub fn sheet_postings_from_fbo_list(wb: &mut Workbook, postings: &[Value]) -> CoreResult<()> {
    let sheet = wb.add_worksheet();
    sheet
        .set_name("Отправления")
        .map_err(|e| CoreError::Internal(format!("xlsx sheet name: {e}")))?;
    let bold = Format::new().set_bold();
    let headers: &[(&str, &str)] = &[
        ("posting_number", "Номер отправления"),
        ("order_number", "Заказ"),
        ("status", "Статус"),
        ("substatus", "Подстатус"),
        ("warehouse_name", "Склад"),
        ("city", "Город"),
        ("delivery_type", "Тип доставки"),
        ("offer_id", "Артикул"),
        ("sku", "SKU"),
        ("name", "Наименование"),
        ("quantity", "Количество"),
        ("price", "Цена"),
        ("old_price", "Цена до скидки"),
        ("currency", "Валюта"),
    ];
    for (col, (_, title)) in headers.iter().enumerate() {
        sheet
            .write_string_with_format(0, col as u16, *title, &bold)
            .map_err(|e| CoreError::Internal(format!("xlsx header: {e}")))?;
    }
    let mut row_idx = 1u32;
    for p in postings {
        // Финансы по товару: financial_data.products[].{product_id, old_price}.
        let fin_by_sku: std::collections::HashMap<String, &Value> = p
            .pointer("/financial_data/products")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|f| {
                        let id = f.get("product_id")?;
                        Some((id.to_string(), f))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let products = p
            .get("products")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for prod in &products {
            let sku_key = prod
                .get("sku")
                .map(std::string::ToString::to_string)
                .unwrap_or_default();
            let fin = fin_by_sku.get(&sku_key).copied();
            let mut row_obj = serde_json::Map::new();
            for field in ["posting_number", "order_number", "status", "substatus"] {
                if let Some(v) = p.get(field) {
                    row_obj.insert(field.to_string(), v.clone());
                }
            }
            for (field, ptr) in [
                ("warehouse_name", "/analytics_data/warehouse_name"),
                ("city", "/analytics_data/city"),
                ("delivery_type", "/analytics_data/delivery_type"),
            ] {
                if let Some(v) = p.pointer(ptr) {
                    row_obj.insert(field.to_string(), v.clone());
                }
            }
            for field in ["offer_id", "sku", "name", "quantity"] {
                if let Some(v) = prod.get(field) {
                    row_obj.insert(field.to_string(), v.clone());
                }
            }
            // Цена: products[].price.amount; старая цена — из financial_data.
            if let Some(amount) = prod.pointer("/price/amount") {
                row_obj.insert("price".into(), amount.clone());
            }
            if let Some(v) = prod.pointer("/price/currency") {
                row_obj.insert("currency".into(), v.clone());
            }
            if let Some(old) = fin.and_then(|f| f.get("old_price")) {
                row_obj.insert("old_price".into(), old.clone());
            }
            let row_val = Value::Object(row_obj);
            for (col, (path, _)) in headers.iter().enumerate() {
                let cell = extract_path(&row_val, path);
                write_cell(sheet, row_idx, col as u16, cell.as_deref());
            }
            row_idx += 1;
        }
    }
    sheet.set_freeze_panes(1, 0).ok();
    let _ = sheet.autofit();
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
            // Числа — числами (Excel суммирует/сортирует), остальное — строкой.
            // Дробные парсим напрямую; целые через i128→f64 (mantissa f64 = 52 бита,
            // потери для финансовых величин нет; крейт пишет f64).
            #[allow(clippy::cast_precision_loss)]
            if let Ok(f) = t.parse::<f64>() {
                if t.parse::<i64>().is_ok() && !t.contains('.') && !t.contains('e') {
                    let _ = sheet.write_number(row, col, f.trunc());
                } else {
                    let _ = sheet.write_number(row, col, f);
                }
            } else {
                let _ = sheet.write_string(row, col, t);
            }
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

/// Интернер ключей: &'static без безлимитного Box::leak на каждый вызов —
/// утечка ограничена числом УНИКАЛЬНЫХ ключей за сессию (обчно десятки).
static INTERNED_KEYS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<&'static str>>> =
    std::sync::OnceLock::new();

fn intern(s: &str) -> &'static str {
    let lock = INTERNED_KEYS.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()));
    let mut set = lock.lock().expect("interner poisoned");
    if let Some(&owned) = set.get(s) {
        return owned;
    }
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    set.insert(leaked);
    leaked
}

/// Fallback: выводит заголовки из union ключей массива объектов.
fn infer_headers(arr: &[&Value]) -> Vec<(&'static str, &'static str)> {
    // Собираем union ключей; path = имя поля, заголовок = оно же.
    let mut keys: Vec<&'static str> = Vec::new();
    for row in arr {
        if let Some(obj) = row.as_object() {
            for k in obj.keys() {
                let owned = intern(k);
                if !keys.contains(&owned) {
                    keys.push(owned);
                }
            }
        }
    }
    keys.into_iter().map(|k| (k, k)).collect()
}

#[allow(clippy::ptr_arg)]
fn infer_headers_vec(arr: &Vec<Value>) -> Vec<(&'static str, &'static str)> {
    let refs: Vec<&Value> = arr.iter().collect();
    infer_headers(&refs)
}

// ===== realization_posting: серверный CSV → Excel (как в ЛК) =====
//
// API /v1/report/realization/posting отдаёт CSV, но в кабинете этот же отчёт
// скачивается Excel-файлом — конвертируем. Мэппинг колонок СВЕРЕН с живым
// ЛК-файлом (июль 2026, итоги сошлись копейка в копейку):
//   «Реализовано на сумму»      = delivery_commission_amount (= price × qty),
//   «выплаты по механикам»      = delivery_commission_bank_coinvestment,
//   «Возвращено на сумму»       = return_commission_amount,
//   «выплаты (возврат)»         = return_commission_bank_coinvestment,
//   «цена реализации»           = *_price_per_instance (НЕ цена продавца!).

/// Тип значения колонки листа «Отчёт».
#[derive(Clone, Copy, PartialEq)]
enum RpKind {
    /// Числом, без суммы в «Итого» (№ п/п, SKU, цены за шт.).
    Num,
    /// Числом + суммируется в «Итого» (количества, суммы, выплаты).
    Sum,
    /// Строкой (артикул/штрих-код с ведущим нулём/номер отправления).
    Str,
    /// Дата «2026-7-7» → «07.07.2026».
    Date,
}

/// Конвертирует CSV позаказного отчёта о реализации в .xlsx:
/// лист 1 «Отчёт о реализации» (колонки как в ЛК + строка «Итого»),
/// лист 2 «Детали (API)» (все исходные поля, русские заголовки).
pub fn realization_posting_csv_to_xlsx(csv: &[u8], period: &str) -> CoreResult<Vec<u8>> {
    const COLS: &[(&str, &str, RpKind)] = &[
        ("row_number", "№ п/п", RpKind::Num),
        ("item_name", "Название товара", RpKind::Str),
        ("item_offer_id", "Артикул", RpKind::Str),
        ("item_sku", "SKU", RpKind::Num),
        ("item_barcode", "Штрих-код товара", RpKind::Str),
        ("delivery_commission_quantity", "Реализовано: кол-во", RpKind::Sum),
        ("delivery_commission_price_per_instance", "Реализовано: цена реализации, руб.", RpKind::Num),
        ("delivery_commission_amount", "Реализовано: на сумму, руб.", RpKind::Sum),
        ("delivery_commission_bank_coinvestment", "Реализовано: выплаты по механикам, руб.", RpKind::Sum),
        ("return_commission_quantity", "Возвращено: кол-во", RpKind::Sum),
        ("return_commission_price_per_instance", "Возвращено: цена реализации, руб.", RpKind::Num),
        ("return_commission_amount", "Возвращено: на сумму, руб.", RpKind::Sum),
        ("return_commission_bank_coinvestment", "Возвращено: выплаты по механикам, руб.", RpKind::Sum),
        ("order_posting_number", "Отправление: номер", RpKind::Str),
        ("order_created_date", "Отправление: дата", RpKind::Date),
        ("legal_entity_document_number", "Продажа юрлицу: № счёт-фактуры", RpKind::Str),
        ("legal_entity_document_sale_date", "Продажа юрлицу: дата", RpKind::Date),
    ];

    let text = String::from_utf8_lossy(csv.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(csv));
    let rows = parse_csv(text.as_ref());
    let Some(header) = rows.first() else {
        return Err(CoreError::Internal(
            "realization_posting: пустой CSV от сервера".into(),
        ));
    };
    let col = |name: &str| header.iter().position(|h| h == name);

    let bold = Format::new().set_bold();
    let mut wb = Workbook::new();

    // --- Лист 1: «Отчёт о реализации» (как в ЛК) ---
    let s1 = wb.add_worksheet();
    s1.set_name("Отчёт о реализации")
        .map_err(|e| CoreError::Internal(format!("xlsx sheet name: {e}")))?;
    s1.write_string_with_format(0, 0, format!("Отчёт о реализации (позаказный). Период: {period}"), &bold)
        .map_err(|e| CoreError::Internal(format!("xlsx: {e}")))?;
    for (c, (_, title, _)) in COLS.iter().enumerate() {
        s1.write_string_with_format(1, c as u16, *title, &bold)
            .map_err(|e| CoreError::Internal(format!("xlsx header: {e}")))?;
    }
    let mut totals = vec![0f64; COLS.len()];
    for (ri, row) in rows.iter().enumerate().skip(1) {
        let out_row = (ri + 1) as u32;
        for (c, (field, _, kind)) in COLS.iter().enumerate() {
            let raw = col(field).and_then(|i| row.get(i)).map(String::as_str);
            match kind {
                RpKind::Str => {
                    if let Some(t) = raw {
                        let _ = s1.write_string(out_row, c as u16, t);
                    }
                }
                RpKind::Date => {
                    if let Some(t) = raw {
                        let _ = s1.write_string(out_row, c as u16, normalize_ymd(t));
                    }
                }
                RpKind::Num | RpKind::Sum => {
                    if let Some(t) = raw {
                        write_cell(s1, out_row, c as u16, Some(t));
                        if *kind == RpKind::Sum {
                            if let Ok(v) = t.parse::<f64>() {
                                totals[c] += v;
                            }
                        }
                    }
                }
            }
        }
    }
    // «Итого»: только осмысленные суммы (количества/суммы/выплаты); SKU,
    // № п/п и цены за штуку не суммируются.
    let total_row = (rows.len() + 1) as u32;
    let _ = s1.write_string_with_format(total_row, 0, "Итого", &bold);
    for (c, (_, _, kind)) in COLS.iter().enumerate() {
        if *kind == RpKind::Sum {
            let _ = s1.write_number_with_format(total_row, c as u16, totals[c], &bold);
        }
    }
    s1.set_freeze_panes(2, 0).ok();
    let _ = s1.autofit();

    // --- Лист 2: «Детали (API)» — все исходные поля, ничего не теряем ---
    let s2 = wb.add_worksheet();
    s2.set_name("Детали (API)")
        .map_err(|e| CoreError::Internal(format!("xlsx sheet name: {e}")))?;
    for (c, h) in header.iter().enumerate() {
        s2.write_string_with_format(0, c as u16, rp_field_ru(h), &bold)
            .map_err(|e| CoreError::Internal(format!("xlsx header: {e}")))?;
    }
    for (ri, row) in rows.iter().enumerate().skip(1) {
        for (c, v) in row.iter().enumerate() {
            // Штрих-коды/артикулы/отправления — строкой (ведущие нули),
            // остальное через write_cell (числа числами).
            if matches!(
                header.get(c).map(String::as_str),
                Some("item_barcode" | "item_offer_id" | "order_posting_number")
            ) {
                let _ = s2.write_string(ri as u32, c as u16, v);
            } else {
                write_cell(s2, ri as u32, c as u16, Some(v));
            }
        }
    }
    s2.set_freeze_panes(1, 0).ok();
    let _ = s2.autofit();

    wb.save_to_buffer()
        .map_err(|e| CoreError::Internal(format!("xlsx write: {e}")))
}

/// Русское имя поля CSV позаказной реализации (для листа «Детали»).
/// Незнакомые поля (Ozon добавит новые) остаются с исходным именем.
fn rp_field_ru(field: &str) -> &str {
    match field {
        "row_number" => "№ п/п",
        "commission_ratio" => "Коэффициент комиссии",
        "seller_price_per_instance" => "Цена продавца за шт., руб.",
        "order_posting_number" => "Номер отправления",
        "order_created_date" => "Дата создания заказа",
        "item_sku" => "SKU",
        "item_barcode" => "Штрих-код товара",
        "item_name" => "Название товара",
        "item_offer_id" => "Артикул",
        "legal_entity_document_number" => "№ счёт-фактуры (юрлицо)",
        "legal_entity_document_sale_date" => "Дата счёт-фактуры (юрлицо)",
        "delivery_commission_amount" => "Доставка: сумма, руб.",
        "delivery_commission_bank_coinvestment" => "Доставка: соинвестирование банка, руб.",
        "delivery_commission_bonus" => "Доставка: бонус, руб.",
        "delivery_commission_commission" => "Доставка: комиссия, руб.",
        "delivery_commission_compensation" => "Доставка: компенсация, руб.",
        "delivery_commission_pick_up_point_coinvestment" => "Доставка: соинвестирование ПВЗ, руб.",
        "delivery_commission_price_per_instance" => "Доставка: цена за шт., руб.",
        "delivery_commission_quantity" => "Доставка: количество",
        "delivery_commission_standard_fee" => "Доставка: стандартный тариф, руб.",
        "delivery_commission_stars" => "Доставка: звёзды",
        "delivery_commission_total" => "Доставка: итого, руб.",
        "return_commission_amount" => "Возврат: сумма, руб.",
        "return_commission_bank_coinvestment" => "Возврат: соинвестирование банка, руб.",
        "return_commission_bonus" => "Возврат: бонус, руб.",
        "return_commission_commission" => "Возврат: комиссия, руб.",
        "return_commission_compensation" => "Возврат: компенсация, руб.",
        "return_commission_pick_up_point_coinvestment" => "Возврат: соинвестирование ПВЗ, руб.",
        "return_commission_price_per_instance" => "Возврат: цена за шт., руб.",
        "return_commission_quantity" => "Возврат: количество",
        "return_commission_standard_fee" => "Возврат: стандартный тариф, руб.",
        "return_commission_stars" => "Возврат: звёзды",
        "return_commission_total" => "Возврат: итого, руб.",
        other => intern(other),
    }
}

/// Мини-парсер CSV (RFC 4180): кавычки, запятые и переводы строк внутри
/// кавычек, пустые поля. Ozon realization_posting — разделитель «,».
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
        } else {
            match ch {
                '"' => in_quotes = true,
                ',' => {
                    row.push(std::mem::take(&mut field));
                }
                '\r' => {}
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    let blank_line = row.len() == 1 && row[0].is_empty();
                    if blank_line {
                        row.clear();
                    } else {
                        rows.push(std::mem::take(&mut row));
                    }
                }
                _ => field.push(ch),
            }
        }
    }
    row.push(field);
    if !(row.len() == 1 && row[0].is_empty()) {
        rows.push(row);
    }
    rows
}

/// «2026-7-7»/«2026-07-07» → «07.07.2026»; не-дата — как есть.
fn normalize_ymd(src: &str) -> String {
    let trimmed = src.trim();
    let parts: Vec<&str> = trimmed.split('-').collect();
    if parts.len() != 3 {
        return trimmed.to_string();
    }
    match (
        parts[0].parse::<i32>(),
        parts[1].parse::<u32>(),
        parts[2].parse::<u32>(),
    ) {
        (Ok(year), Ok(month), Ok(day)) if (1..=12).contains(&month) && (1..=31).contains(&day) => {
            format!("{day:02}.{month:02}.{year}")
        }
        _ => trimmed.to_string(),
    }
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

    #[test]
    fn rp_csv_parser_quotes_and_newlines() {
        let rows = parse_csv("a,b,c\r\n1,\"x,y\",3\n\"много\nстрок\",,\r\n");
        assert_eq!(rows.len(), 3, "строки");
        assert_eq!(rows[0], vec!["a", "b", "c"]);
        assert_eq!(rows[1], vec!["1", "x,y", "3"]);
        assert_eq!(rows[2], vec!["много\nстрок", "", ""]);
    }

    #[test]
    fn rp_normalize_date() {
        assert_eq!(normalize_ymd("2026-7-7"), "07.07.2026");
        assert_eq!(normalize_ymd("2026-07-31"), "31.07.2026");
        // Не-даты не трогаем.
        assert_eq!(normalize_ymd("abc"), "abc");
        assert_eq!(normalize_ymd("2026-13-01"), "2026-13-01");
    }

    #[test]
    fn rp_xlsx_is_zip_and_built() {
        // Мини-CSV с основными полями (включая штрих-код с ведущим нулём).
        let csv = "row_number,item_name,item_offer_id,item_sku,item_barcode,\
delivery_commission_quantity,delivery_commission_price_per_instance,\
delivery_commission_amount,delivery_commission_bank_coinvestment,\
return_commission_quantity,return_commission_price_per_instance,\
return_commission_amount,return_commission_bank_coinvestment,\
order_posting_number,order_created_date,legal_entity_document_number,\
legal_entity_document_sale_date\n\
1,Товар,\"A-1\",1528621656,04610279152162,1,3084.360000,3084.360000,30.840000,0,0.000000,0.000000,0.000000,0148735686-0201-1,2026-7-7,,\n\
2,\"Товар, второй\",\"A-2\",1528634794,04610279152131,1,3318.440000,3318.440000,33.180000,1,2211.630000,2211.630000,22.120000,73841641-0021-1,2026-07-15,,\n";
        let bytes = realization_posting_csv_to_xlsx(csv.as_bytes(), "2026-07").unwrap();
        // Настоящий xlsx — ZIP (урок #29).
        assert!(bytes.starts_with(b"PK"), "должен быть ZIP");
        assert!(bytes.len() > 2000);
    }

    #[test]
    fn rp_empty_csv_errors() {
        assert!(realization_posting_csv_to_xlsx(b"", "2026-07").is_err());
    }
}
