//! Конвертация WB-отчётов в Excel (.xlsx) с русскими заголовками колонок.
//!
//! Все WB-отчёты (кроме Documents API, который отдаёт настоящие файлы)
//! приходят JSON-массивами строк. Для бухгалтера JSON нечитаем — строим
//! xlsx по образцу ozon::xlsx.
//!
//! Отличие от Ozon-подхода (жёсткие списки колонок): курируемые упорядоченные
//! колонки задают ПРИОРИТЕТНЫЕ поля и их русские имена, а ВСЕ остальные поля
//! ответа (в т.ч. добавленные WB после релиза) автоматически добавляются в
//! конец с исходными именами — данные не теряются молча.
//!
//! Источники имён полей: OpenAPI-спека WB (eslazarev/wildberries-sdk) и
//! живые ответы (аудит 2026-08-14).

use rust_xlsxwriter::{Format, Workbook};

use mdwf_core::{CoreError, CoreResult};

use serde_json::Value;

/// Строит .xlsx из строк отчёта `type_id`. Возвращает байты файла.
pub(crate) fn rows_to_xlsx(type_id: &str, rows: &[Value]) -> CoreResult<Vec<u8>> {
    let mut wb = Workbook::new();
    let spec = sheet_spec(type_id);
    write_rows_sheet(&mut wb, spec.title, rows, spec.headers)?;
    wb.save_to_buffer()
        .map_err(|e| CoreError::Internal(format!("xlsx write: {e}")))
}

/// Строит .xlsx для баланса (плоский объект → ключ/значение с русскими
/// подписями). Возвращает байты файла.
pub(crate) fn balance_to_xlsx(balance: &Value) -> CoreResult<Vec<u8>> {
    let labels = &[
        ("currency", "Валюта"),
        ("current", "Текущий баланс"),
        ("for_withdraw", "Доступно к выводу"),
    ];
    let mut wb = Workbook::new();
    let sheet = wb.add_worksheet();
    sheet
        .set_name("Баланс")
        .map_err(|e| CoreError::Internal(format!("xlsx sheet name: {e}")))?;
    let bold = Format::new().set_bold();
    sheet
        .write_string_with_format(0, 0, "Показатель", &bold)
        .map_err(|e| CoreError::Internal(format!("xlsx header: {e}")))?;
    sheet
        .write_string_with_format(0, 1, "Значение", &bold)
        .map_err(|e| CoreError::Internal(format!("xlsx header: {e}")))?;
    let mut row = 1u32;
    for (key, label) in labels {
        sheet
            .write_string(row, 0, *label)
            .map_err(|e| CoreError::Internal(format!("xlsx cell: {e}")))?;
        write_cell(sheet, row, 1, value_to_cell(balance.get(*key)).as_deref());
        row += 1;
    }
    // Неизвестные поля баланса (WB может добавить) — дописываем с raw-именем.
    if let Some(obj) = balance.as_object() {
        for (k, v) in obj {
            if labels.iter().any(|(key, _)| *key == *k) {
                continue;
            }
            sheet
                .write_string(row, 0, k)
                .map_err(|e| CoreError::Internal(format!("xlsx cell: {e}")))?;
            write_cell(sheet, row, 1, value_to_cell(Some(v)).as_deref());
            row += 1;
        }
    }
    let _ = sheet.autofit();
    wb.save_to_buffer()
        .map_err(|e| CoreError::Internal(format!("xlsx write: {e}")))
}

// =========================================================================
// Спецификации листов (заголовок + курируемые колонки)
// =========================================================================

struct SheetSpec {
    title: &'static str,
    headers: &'static [(&'static str, &'static str)],
}

fn sheet_spec(type_id: &str) -> SheetSpec {
    let (title, headers): (&str, &[(&str, &str)]) = match type_id {
        "wb.sales_reports_detailed" => (
            "Детализация реализации",
            &[
                ("rrDate", "Дата начисления"),
                ("rrdId", "ID строки"),
                ("reportId", "ID отчёта"),
                ("reportType", "Тип отчёта"),
                ("dateFrom", "Период с"),
                ("dateTo", "Период по"),
                ("saleDt", "Дата продажи"),
                ("orderDt", "Дата заказа"),
                ("orderId", "Заказ"),
                ("srid", "srid"),
                ("vendorCode", "Артикул продавца"),
                ("brandName", "Бренд"),
                ("subjectName", "Предмет"),
                ("nmId", "nmId"),
                ("techSize", "Размер"),
                ("quantity", "Количество"),
                ("retailPrice", "Цена до скидки"),
                ("retailAmount", "Сумма до скидки"),
                ("salePercent", "Скидка, %"),
                ("commissionPercent", "Комиссия, %"),
                ("ppvzSalesCommission", "Комиссия за продажу"),
                ("ppvzReward", "Вознаграждение WB"),
                ("forPay", "К перечислению"),
                ("penalty", "Штраф"),
                ("deduction", "Удержание"),
                ("additionalPayment", "Доплаты"),
                ("returnAmount", "Возврат"),
                ("deliveryAmount", "Стоимость логистики"),
                ("deliveryService", "Служба доставки"),
                ("paidStorage", "Платное хранение"),
                ("paidAcceptance", "Платная приёмка"),
                ("acquiringBank", "Банк-эквайер"),
                ("officeName", "Офис/склад"),
                ("vw", "НДС"),
                ("currency", "Валюта"),
                ("createDate", "Дата создания"),
            ],
        ),
        "wb.sales_reports_list" => (
            "Реестр реализации",
            &[
                ("reportId", "ID отчёта"),
                ("reportType", "Тип отчёта"),
                ("createDate", "Дата создания"),
                ("dateFrom", "Период с"),
                ("dateTo", "Период по"),
                ("retailAmountSum", "Сумма розничная"),
                ("forPaySum", "К перечислению"),
                ("penaltySum", "Штрафы"),
                ("deductionSum", "Удержания"),
                ("additionalPaymentSum", "Доплаты"),
                ("paidAcceptanceSum", "Платная приёмка"),
                ("paidStorageSum", "Платное хранение"),
                ("deliveryServiceSum", "Стоимость доставки"),
                ("cashbackAmountSum", "Кэшбэк"),
                ("currency", "Валюта"),
                ("sellerFinanceName", "Кабинет"),
            ],
        ),
        "wb.acquiring_list" => (
            "Эквайринг",
            &[
                ("dateFrom", "Период с"),
                ("dateTo", "Период по"),
                ("createDate", "Дата создания"),
                ("docNumber", "Номер документа"),
                ("acquiringSum", "Сумма"),
                ("currency", "Валюта"),
            ],
        ),
        "wb.acquiring_detailed" => (
            "Эквайринг (детализация)",
            &[
                ("rrdId", "ID строки"),
                ("docNumber", "Номер документа"),
                ("date", "Дата"),
                ("orderNumber", "Заказ"),
                ("cardMask", "Карта"),
                ("acquiringPercent", "Эквайринг, %"),
                ("acquiringFee", "Комиссия эквайринга"),
                ("amount", "Сумма"),
                ("currency", "Валюта"),
            ],
        ),
        "wb.orders" => (
            "Заказы",
            &[
                ("date", "Дата заказа"),
                ("lastChangeDate", "Дата изменения"),
                ("status", "Статус"),
                ("warehouseName", "Склад"),
                ("supplierArticle", "Артикул"),
                ("brand", "Бренд"),
                ("subject", "Предмет"),
                ("techSize", "Размер"),
                ("nmId", "nmId"),
                ("barcode", "Штрихкод"),
                ("incomeID", "Номер поставки"),
                ("totalPrice", "Цена до скидки"),
                ("discountPercent", "Скидка, %"),
                ("finishedPrice", "Цена итоговая"),
                ("priceWithDisc", "Цена со скидкой"),
                ("isCancel", "Отменён"),
                ("gNumber", "Номер заказа"),
                ("srid", "srid"),
            ],
        ),
        "wb.sales" => (
            "Продажи",
            &[
                ("saleID", "Номер продажи"),
                ("date", "Дата продажи"),
                ("lastChangeDate", "Дата изменения"),
                ("warehouseName", "Склад"),
                ("supplierArticle", "Артикул"),
                ("brand", "Бренд"),
                ("subject", "Предмет"),
                ("techSize", "Размер"),
                ("nmId", "nmId"),
                ("barcode", "Штрихкод"),
                ("incomeID", "Номер поставки"),
                ("totalPrice", "Цена до скидки"),
                ("discountPercent", "Скидка, %"),
                ("finishedPrice", "Цена итоговая"),
                ("priceWithDisc", "Цена со скидкой"),
                ("isCancel", "Отменён"),
                ("gNumber", "Номер заказа"),
                ("srid", "srid"),
            ],
        ),
        "wb.deductions" => (
            "Удержания",
            &[
                ("nmId", "nmId"),
                ("subjectName", "Предмет"),
                ("dtBonus", "Дата"),
                ("bonusSumm", "Сумма"),
                ("reason", "Причина"),
            ],
        ),
        "wb.measurement_penalties" => (
            "Штрафы за габариты",
            &[
                ("nmId", "nmId"),
                ("subjectName", "Предмет"),
                ("volume", "Объём"),
                ("width", "Ширина"),
                ("length", "Длина"),
                ("height", "Высота"),
                ("prcOver", "Превышение, %"),
                ("fineAmount", "Сумма штрафа"),
            ],
        ),
        "wb.antifraud" => (
            "Антифрод",
            &[
                ("dateFrom", "Неделя с"),
                ("dateTo", "по"),
                ("nmID", "nmId"),
                ("sum", "Сумма"),
                ("currency", "Валюта"),
            ],
        ),
        "wb.claims" => (
            "Возвраты",
            &[
                ("id", "ID заявки"),
                ("claim_type", "Тип"),
                ("status", "Статус"),
                ("status_ex", "Статус (подробно)"),
                ("imt_name", "Товар"),
                ("nm_id", "nmId"),
                ("user_comment", "Комментарий продавца"),
                ("wb_comment", "Комментарий WB"),
                ("dt", "Дата заявки"),
                ("order_dt", "Дата заказа"),
                ("srid", "srid"),
                ("price", "Цена"),
                ("currency_code", "Валюта"),
                ("delivery_dt", "Дата доставки"),
            ],
        ),
        "wb.acceptance_report" => (
            "Приёмка",
            &[
                ("incomeId", "Номер поставки"),
                ("giCreateDate", "Дата приёмки"),
                ("nmID", "nmId"),
                ("subjectName", "Предмет"),
                ("shkCreateDate", "Дата ШК"),
                ("count", "Количество"),
                ("total", "Сумма"),
            ],
        ),
        _ => ("Отчёт", &[]),
    };
    SheetSpec { title, headers }
}

// =========================================================================
// Построение листа
// =========================================================================

/// Пишет лист: строка 0 — заголовки (bold), далее строки данных.
/// Колонки = курируемые (присутствующие в данных, в порядке словаря) +
/// все прочие поля строк (первое вхождение) с исходными именами.
fn write_rows_sheet(
    wb: &mut Workbook,
    sheet_title: &str,
    rows: &[Value],
    curated: &[(&str, &str)],
) -> CoreResult<()> {
    let sheet = wb.add_worksheet();
    sheet
        .set_name(sheet_title)
        .map_err(|e| CoreError::Internal(format!("xlsx sheet name: {e}")))?;
    let bold = Format::new().set_bold();

    // Итоговый список колонок: (поле, заголовок).
    let columns = build_columns(rows, curated);
    for (col, (_, title)) in columns.iter().enumerate() {
        sheet
            .write_string_with_format(0, col as u16, title, &bold)
            .map_err(|e| CoreError::Internal(format!("xlsx header: {e}")))?;
    }

    for (i, row_val) in rows.iter().enumerate() {
        let row_idx = (i + 1) as u32;
        for (col, (field, _)) in columns.iter().enumerate() {
            let cell = value_to_cell(row_val.get(*field));
            write_cell(sheet, row_idx, col as u16, cell.as_deref());
        }
    }
    sheet.set_freeze_panes(1, 0).ok();
    let _ = sheet.autofit();
    Ok(())
}

/// Курируемые поля, присутствующие хотя бы в одной строке (в порядке
/// словаря), затем все прочие поля строк в порядке первого появления.
fn build_columns(
    rows: &[Value],
    curated: &[(&str, &str)],
) -> Vec<(&'static str, String)> {
    // Сырые ключи строк интернируем: заголовки живут до конца сборки листа,
    // а HashSet<String> на каждую строку — лишние аллокации.
    let mut columns: Vec<(&'static str, String)> = Vec::new();
    let mut seen: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
    for (field, title) in curated {
        let present = rows
            .iter()
            .any(|r| r.as_object().is_some_and(|o| o.contains_key(*field)));
        if present {
            let f: &'static str = intern(field);
            seen.insert(f);
            columns.push((f, (*title).to_string()));
        }
    }
    for row in rows {
        if let Some(obj) = row.as_object() {
            for key in obj.keys() {
                let f: &'static str = intern(key);
                if seen.insert(f) {
                    columns.push((f, f.to_string()));
                }
            }
        }
    }
    columns
}

/// Интернер ключей: &'static без Box::leak на каждую строку — утечка
/// ограничена числом УНИКАЛЬНЫХ ключей за сессию (обычно десятки).
static INTERNED_KEYS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<&'static str>>> =
    std::sync::OnceLock::new();

fn intern(s: &str) -> &'static str {
    let lock = INTERNED_KEYS.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()));
    let mut set = lock.lock().expect("interner poisoned");
    if let Some(existing) = set.get(s) {
        existing
    } else {
        let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
        set.insert(leaked);
        leaked
    }
}

/// Преобразует значение JSON в строку ячейки: None для null/пропуска
/// (пустая ячейка); bool → да/нет; вложенные объекты — компактный JSON
/// (у claims есть полезные вложения); массив скаляров — через запятую.
fn value_to_cell(v: Option<&Value>) -> Option<String> {
    match v {
        None | Some(Value::Null) => None,
        Some(other) => Some(cell_str(other)),
    }
}

fn cell_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => {
            if *b {
                "да".into()
            } else {
                "нет".into()
            }
        }
        Value::Array(arr) => arr
            .iter()
            .map(cell_str)
            .collect::<Vec<_>>()
            .join(", "),
        Value::Null => String::new(),
        Value::Object(_) => serde_json::to_string(v).unwrap_or_default(),
    }
}

/// Пишет строку в ячейку. Пустое значение → пустая ячейка. Числа — числами
/// (Excel суммирует/сортирует); целые через i128→f64 (mantissa f64 = 52 бита,
/// ID WB ≤ 16 цифр — потерь нет; крейт пишет f64).
fn write_cell(sheet: &mut rust_xlsxwriter::Worksheet, row: u32, col: u16, text: Option<&str>) {
    if let Some(t) = text {
        if !t.is_empty() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(pairs: &[(&str, Value)]) -> Value {
        let mut map = serde_json::Map::new();
        for (k, v) in pairs {
            map.insert((*k).to_string(), v.clone());
        }
        Value::Object(map)
    }

    #[test]
    fn columns_curated_first_extras_appended() {
        // Курируемые (orders): nmId/barcode; поле date отсутствует в данных —
        // колонки для него быть НЕ должно; прочие поля (zzzExtra, aaaExtra)
        // добавляются после курируемых в порядке первого появления.
        let rows = vec![
            row(&[("zzzExtra", json!(1)), ("nmId", json!(7)), ("barcode", json!("203"))]),
            row(&[("aaaExtra", json!(2)), ("nmId", json!(8))]),
        ];
        let curated = sheet_spec("wb.orders").headers;
        let cols = build_columns(&rows, curated);
        let names: Vec<&str> = cols.iter().map(|(f, _)| *f).collect();
        // Курируемые date/status/warehouseName/supplierArticle/brand/subject/
        // techSize/incomeID/totalPrice/... отсутствуют в данных — не входят.
        // Из курируемых присутствуют nmId и barcode; затем extras.
        assert_eq!(names, vec!["nmId", "barcode", "zzzExtra", "aaaExtra"]);
        // Заголовок курируемой колонки — русский.
        assert_eq!(cols[0].1, "nmId");
        assert_eq!(cols[1].1, "Штрихкод");
        // Extra-колонки: заголовок = имя поля.
        assert_eq!(cols[2].1, "zzzExtra");
    }

    #[test]
    fn bools_become_russian() {
        assert_eq!(value_to_cell(Some(&Value::Bool(true))).as_deref(), Some("да"));
        assert_eq!(value_to_cell(Some(&Value::Bool(false))).as_deref(), Some("нет"));
    }

    #[test]
    fn nested_object_becomes_compact_json() {
        let v = json!({"origin_id_info": {"order": "A-1"}});
        let s = value_to_cell(Some(&v)).unwrap_or_default();
        assert_eq!(s, r#"{"origin_id_info":{"order":"A-1"}}"#);
    }

    #[test]
    fn null_and_missing_are_empty() {
        assert_eq!(value_to_cell(None), None);
        assert_eq!(value_to_cell(Some(&Value::Null)), None);
    }

    #[test]
    fn xlsx_bytes_are_zip() {
        // Реальный файл xlsx — это ZIP (magic PK).
        let rows = vec![row(&[("nmId", json!(123)), ("sum", json!(10.5))])];
        let bytes = rows_to_xlsx("wb.antifraud", &rows).unwrap();
        assert_eq!(&bytes[..2], b"PK");
    }

    #[test]
    fn balance_sheet_bytes_are_zip() {
        let bytes = balance_to_xlsx(&json!({"currency": "RUB", "current": 1.5, "for_withdraw": 0}))
            .unwrap();
        assert_eq!(&bytes[..2], b"PK");
    }

    #[test]
    fn unknown_report_falls_back_to_all_fields() {
        // Нет курируемых — все поля. Внутри строки serde_json (BTreeMap)
        // отдаёт ключи алфавитно; между строками — порядок первого появления.
        let rows = vec![
            row(&[("b", json!(1)), ("a", json!(2))]),
            row(&[("c", json!(3)), ("a", json!(4))]),
        ];
        let cols = build_columns(&rows, &[]);
        let names: Vec<&str> = cols.iter().map(|(f, _)| *f).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }
}
