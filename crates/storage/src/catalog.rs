//! SQLite-каталог (спец. §2.7.2, гл. 06).
//!
//! Хранит профили, выгрузки, расписания, сохранённые фильтры.

use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use mdwf_core::{CoreError, CoreResult, Profile};

/// Версия схемы для совместимости (спец. `schema_version` в config.toml).
/// v3: добавлена колонка `downloads.document_id` (значок «уже загружен»).
/// v4: добавлена колонка `downloads.document_date` (дата документа WB для
/// фильтра периода Архива).
pub const SCHEMA_VERSION: u32 = 4;

/// Встроенная схема (создаётся при первом подключении).
const SCHEMA_SQL: &str = include_str!("../schema.sql");

/// Idempotent миграция v3: добавляет `downloads.document_id`, если колонки нет
/// (существующие БД до v3), и backfill-ит её из `params.values["ids"]`
/// (первый serviceName в CSV) для уже скачанных документов. Также создаёт
/// индекс по document_id (после гарантированного наличия колонки).
fn migrate_add_document_id(conn: &Connection) -> CoreResult<()> {
    // Проверяем наличие колонки через PRAGMA table_info.
    let has_col: bool = conn
        .prepare("PRAGMA table_info(downloads)")
        .and_then(|mut stmt| {
            let rows = stmt.query_map([], |row| {
                let name: String = row.get(1)?;
                Ok(name)
            })?;
            Ok(rows.filter_map(Result::ok).any(|name| name == "document_id"))
        })
        .map_err(map_sqlite_err)?;
    if !has_col {
        conn.execute("ALTER TABLE downloads ADD COLUMN document_id TEXT", [])
            .map_err(map_sqlite_err)?;
    }
    // Backfill: для строк с NULL document_id извлекаем serviceName из params JSON.
    let rows: Vec<(i64, Option<String>)> = conn
        .prepare("SELECT id, params FROM downloads WHERE document_id IS NULL")
        .and_then(|mut stmt| {
            let mapped = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            Ok(mapped.filter_map(Result::ok).collect())
        })
        .map_err(map_sqlite_err)?;
    for (id, params_json) in rows {
        if let Some(doc_id) = params_json
            .as_deref()
            .and_then(extract_first_id_from_params)
        {
            let _ = conn.execute(
                "UPDATE downloads SET document_id=?1 WHERE id=?2",
                params![doc_id, id],
            );
        }
    }
    // Индекс по document_id — после гарантированного наличия колонки.
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_downloads_doc \
         ON downloads(profile_id, report_type, document_id)",
        [],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

/// Idempotent миграция v4: добавляет `downloads.document_date`, если колонки нет
/// (существующие БД до v4). Backfill НЕ выполняется — у старых WB-записей даты
/// документа нет в `params` (не из чего восстановить); они остаются с NULL и
/// выпадают из фильтра периода Архива (видны при фильтре «все»).
fn migrate_add_document_date(conn: &Connection) -> CoreResult<()> {
    let has_col: bool = conn
        .prepare("PRAGMA table_info(downloads)")
        .and_then(|mut stmt| {
            let rows = stmt.query_map([], |row| {
                let name: String = row.get(1)?;
                Ok(name)
            })?;
            Ok(rows.filter_map(Result::ok).any(|name| name == "document_date"))
        })
        .map_err(map_sqlite_err)?;
    if !has_col {
        conn.execute("ALTER TABLE downloads ADD COLUMN document_date TEXT", [])
            .map_err(map_sqlite_err)?;
    }
    Ok(())
}

/// Извлекает первый идентификатор документа из JSON `ReportParams` (поле
/// `values["ids"]` — CSV serviceName, или `values["doc_meta"]` — массив).
/// Используется при backfill-миграции для старых строк без `document_id`.
fn extract_first_id_from_params(params_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(params_json).ok()?;
    let values = v.get("values")?;
    // values["ids"] = "id1,id2,..." → первый элемент.
    if let Some(ids) = values.get("ids").and_then(|i| i.as_str()) {
        if let Some(first) = ids.split(',').next() {
            let trimmed = first.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    // values["doc_meta"] = [{id,...}] → id первого элемента.
    if let Some(arr) = values.get("doc_meta").and_then(|d| d.as_array()) {
        if let Some(first) = arr.first() {
            if let Some(id) = first.get("id").and_then(|i| i.as_str()) {
                return Some(id.to_string());
            }
        }
    }
    None
}

/// Запись о скачанном файле в каталоге.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadRecord {
    pub id: i64,
    pub profile_id: i64,
    pub report_type: String,
    pub period: Option<String>,
    pub params: Option<String>,
    pub file_path: String,
    pub file_size: i64,
    pub file_hash: Option<String>,
    pub file_format: String,
    pub rows_count: Option<i64>,
    pub downloader_kind: String,
    pub source_url: Option<String>,
    /// Дата документа (WB creationTime → YYYY-MM-DD). None для Period-отчётов.
    pub document_date: Option<String>,
    pub downloaded_at: DateTime<Utc>,
}

/// Элемент архива скачанных документов (плоский DTO для офлайн-навигации в UI).
///
/// В отличие от `DownloadRecord`, здесь JOIN с `profiles` добавляет человекочитаемые
/// `profile_name` и `provider_id` (их нет в таблице `downloads`), а число полей
/// сокращено до необходимых для списка Архива.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveEntry {
    pub id: i64,
    pub profile_id: i64,
    pub profile_name: String,
    pub provider_id: String,
    pub report_type: String,
    /// Период отчёта (параметр запроса: YYYY-MM/YYYY-MM-DD) — для Ozon.
    /// Для WB-документов NULL (нет периода запроса); вместо него смотрим `document_date`.
    pub period: Option<String>,
    pub file_path: String,
    pub file_size: i64,
    pub file_format: String,
    /// Идентификатор документа (WB serviceName); None для Period-отчётов Ozon.
    pub document_id: Option<String>,
    /// Дата документа (WB creationTime → YYYY-MM-DD). Используется для фильтра
    /// периода Архива (как fallback периода) и отображается в колонке «Период»,
    /// если `period` пуст.
    pub document_date: Option<String>,
    pub downloaded_at: DateTime<Utc>,
}

/// Сохранённый фильтр журнала документов.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedFilter {
    pub id: i64,
    pub name: String,
    pub provider_id: String,
    pub report_type: String,
    pub filter_json: String,
}

/// Каталог на SQLite. Потокобезопасный через Mutex (rusqlite::Connection не Sync).
#[derive(Clone)]
pub struct Catalog {
    conn: Arc<Mutex<Connection>>,
}

impl Catalog {
    /// Открывает (или создаёт) каталог по пути.
    pub fn open(path: &Path) -> CoreResult<Self> {
        let conn = Connection::open(path).map_err(map_sqlite_err)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(map_sqlite_err)?;
        let cat = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        cat.apply_schema()?;
        Ok(cat)
    }

    /// In-memory каталог для тестов.
    #[cfg(test)]
    pub fn open_in_memory() -> CoreResult<Self> {
        let conn = Connection::open_in_memory().map_err(map_sqlite_err)?;
        let cat = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        cat.apply_schema()?;
        Ok(cat)
    }

    fn apply_schema(&self) -> CoreResult<()> {
        let conn = self.conn.lock();
        conn.execute_batch(SCHEMA_SQL).map_err(map_sqlite_err)?;
        // Миграция v3: колонка downloads.document_id для значка «уже загружен».
        // Для существующих БД (где таблица создана без колонки) — idempotent ALTER.
        migrate_add_document_id(&conn)?;
        // Миграция v4: колонка downloads.document_date (дата документа WB для
        // фильтра периода Архива). Без backfill (см. migrate_add_document_date).
        migrate_add_document_date(&conn)?;
        Ok(())
    }

    // ----- Профили -----

    /// Сохраняет/обновляет профиль. Возвращает id.
    pub fn upsert_profile(&self, p: &Profile) -> CoreResult<i64> {
        let metadata = serde_json::to_string(&p.auth_metadata).unwrap_or_else(|_| "{}".into());
        let conn = self.conn.lock();
        if let Some(id) = p.id {
            conn.execute(
                "UPDATE profiles SET name=?1, provider_id=?2, description=?3, auth_metadata=?4,
                 keychain_id=?5, updated_at=CURRENT_TIMESTAMP WHERE id=?6",
                params![p.name, p.provider_id, p.description, metadata, p.keychain_id, id],
            )
            .map_err(map_sqlite_err)?;
            Ok(id)
        } else {
            conn.execute(
                "INSERT INTO profiles (name, provider_id, description, auth_metadata, keychain_id)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![p.name, p.provider_id, p.description, metadata, p.keychain_id],
            )
            .map_err(map_sqlite_err)?;
            Ok(conn.last_insert_rowid())
        }
    }

    /// Возвращает все профили.
    pub fn list_profiles(&self) -> CoreResult<Vec<Profile>> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, provider_id, description, auth_metadata, keychain_id FROM profiles ORDER BY name",
            )
            .map_err(map_sqlite_err)?;
        let rows = stmt
            .query_map([], row_to_profile)
            .map_err(map_sqlite_err)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(map_sqlite_err)?);
        }
        Ok(out)
    }

    /// Возвращает профиль по имени.
    pub fn get_profile_by_name(&self, name: &str) -> CoreResult<Option<Profile>> {
        let conn = self.conn.lock();
        let p = conn
            .query_row(
                "SELECT id, name, provider_id, description, auth_metadata, keychain_id
                 FROM profiles WHERE name=?1",
                params![name],
                row_to_profile,
            )
            .optional()
            .map_err(map_sqlite_err)?;
        Ok(p)
    }

    /// Удаляет профиль по имени.
    pub fn delete_profile(&self, name: &str) -> CoreResult<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM profiles WHERE name=?1", params![name])
            .map_err(map_sqlite_err)?;
        Ok(())
    }

    /// Удаляет **все** профили. Утилита для обслуживания (напр. полный сброс),
    /// регулярного использования нет — секреты хранятся в keyring, а профили
    /// переживают перезапуск приложения.
    #[allow(dead_code)]
    pub fn clear_profiles(&self) -> CoreResult<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM profiles", params![])
            .map_err(map_sqlite_err)?;
        Ok(())
    }

    // ----- Загрузки -----

    /// Записывает факт скачивания файла. Дедупликация через UNIQUE-индекс:
    /// повторная вставка того же хэша вернёт существующий id (no-op).
    pub fn record_download(&self, r: &NewDownload) -> CoreResult<i64> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO downloads
             (profile_id, report_type, period, params, file_path, file_size, file_hash,
              file_format, rows_count, downloader_kind, source_url, document_id, document_date)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(profile_id, report_type, period, file_hash) DO UPDATE SET
                 file_path=excluded.file_path, document_id=excluded.document_id,
                 document_date=excluded.document_date,
                 downloaded_at=CURRENT_TIMESTAMP",
            params![
                r.profile_id,
                r.report_type,
                r.period,
                r.params,
                r.file_path,
                r.file_size,
                r.file_hash,
                r.file_format,
                r.rows_count,
                r.downloader_kind,
                r.source_url,
                r.document_id,
                r.document_date,
            ],
        )
        .map_err(map_sqlite_err)?;
        Ok(conn.last_insert_rowid())
    }

    /// Список скачанных документов для профиля+отчёта (для значка «уже загружен»).
    /// Возвращает только строки с заполненным `document_id`. UI строит
    /// `HashMap<document_id, DownloadedDocInfo>` для O(1) lookup в списке.
    pub fn list_downloaded_docs(
        &self,
        profile_id: i64,
        report_type: &str,
    ) -> CoreResult<Vec<DownloadedDocInfo>> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT document_id, file_path, file_format, downloaded_at
                 FROM downloads
                 WHERE profile_id=?1 AND report_type=?2 AND document_id IS NOT NULL",
            )
            .map_err(map_sqlite_err)?;
        let rows = stmt
            .query_map(params![profile_id, report_type], |row| {
                let downloaded_at: String = row.get(3)?;
                let downloaded_at = match chrono::DateTime::parse_from_rfc3339(&downloaded_at) {
                    Ok(dt) => dt.with_timezone(&Utc),
                    Err(_) => Utc::now(),
                };
                Ok(DownloadedDocInfo {
                    document_id: row.get(0)?,
                    file_path: row.get(1)?,
                    file_format: row.get(2)?,
                    downloaded_at,
                })
            })
            .map_err(map_sqlite_err)?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    /// Список скачанных файлов с опциональными фильтрами (для вкладки «Архив»).
    ///
    /// В отличие от `list_downloaded_docs`, возвращает **все** строки (включая
    /// Period-отчёты Ozon без `document_id`), JOIN с `profiles` добавляет
    /// `profile_name`/`provider_id`. Фильтры опциональны: `None` = не фильтровать.
    ///
    /// `date_range` — фильтр периода как **пересечение диапазонов** (inclusion),
    /// НЕ точное совпадение. Кортеж `(from "YYYY-MM-DD", to "YYYY-MM-DD")`.
    /// Файл попадает, если его «интервал даты» пересекается с диапазоном фильтра.
    /// Интервал файла вычисляется из `period` (YYYY-MM → месяц целиком; YYYY-MM-DD
    /// → точка) или, при отсутствии `period`, из `document_date` (точка). Файлы
    /// без обеих дат (старые WB-записи) из фильтра по периоду выпадают (видны при
    /// `date_range=None`). Результат отсортирован по `downloaded_at DESC`.
    pub fn list_downloads_filtered(
        &self,
        profile_id: Option<i64>,
        report_type: Option<&str>,
        date_range: Option<(String, String)>,
    ) -> CoreResult<Vec<ArchiveEntry>> {
        let conn = self.conn.lock();
        // Динамическая сборка WHERE: только выбранные фильтры. Имена колонок
        // зашиты в строку (не от пользователя), параметры — через placeholders.
        let mut where_clauses: Vec<String> = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(pid) = profile_id {
            where_clauses.push("d.profile_id = ?".to_string());
            params_vec.push(Box::new(pid));
        }
        if let Some(rt) = report_type {
            where_clauses.push("d.report_type = ?".to_string());
            params_vec.push(Box::new(rt.to_string()));
        }
        if let Some((from, to)) = date_range {
            // Вычисляем границы интервала файла в SQL (CASE WHEN), затем проверяем
            // пересечение: file_start <= filter_to AND file_end >= filter_from.
            // period длины 7 ("YYYY-MM") → месяц целиком; длины 10 ("YYYY-MM-DD")
            // → точка; иначе fallback на document_date; иначе NULL (не попадает).
            where_clauses.push(
                "(CASE \
                    WHEN d.period IS NOT NULL AND length(d.period) = 7 \
                        THEN substr(d.period,1,7)||'-01' \
                    WHEN d.period IS NOT NULL AND length(d.period) = 10 \
                        THEN d.period \
                    WHEN d.document_date IS NOT NULL \
                        THEN d.document_date \
                    ELSE NULL \
                  END) IS NOT NULL \
                 AND (CASE \
                    WHEN d.period IS NOT NULL AND length(d.period) = 7 \
                        THEN substr(d.period,1,7)||'-01' \
                    WHEN d.period IS NOT NULL AND length(d.period) = 10 \
                        THEN d.period \
                    WHEN d.document_date IS NOT NULL \
                        THEN d.document_date \
                    ELSE NULL \
                  END) <= ? \
                 AND (CASE \
                    WHEN d.period IS NOT NULL AND length(d.period) = 7 \
                        THEN date(substr(d.period,1,7)||'-01','+1 month','-1 day') \
                    WHEN d.period IS NOT NULL AND length(d.period) = 10 \
                        THEN d.period \
                    WHEN d.document_date IS NOT NULL \
                        THEN d.document_date \
                    ELSE NULL \
                  END) >= ?"
                    .to_string(),
            );
            params_vec.push(Box::new(to));
            params_vec.push(Box::new(from));
        }
        let where_sql = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };
        let sql = format!(
            "SELECT d.id, d.profile_id, p.name, p.provider_id, d.report_type,
                    d.period, d.file_path, d.file_size, d.file_format,
                    d.document_id, d.document_date, d.downloaded_at
             FROM downloads d
             JOIN profiles p ON p.id = d.profile_id
             {where_sql}
             ORDER BY d.downloaded_at DESC"
        );
        let mut stmt = conn.prepare(&sql).map_err(map_sqlite_err)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(std::convert::AsRef::as_ref).collect();
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                let downloaded_at: String = row.get(11)?;
                let downloaded_at = match chrono::DateTime::parse_from_rfc3339(&downloaded_at) {
                    Ok(dt) => dt.with_timezone(&Utc),
                    Err(_) => Utc::now(),
                };
                let period: Option<String> = row.get(5)?;
                let document_date: Option<String> = row.get(10)?;
                Ok(ArchiveEntry {
                    id: row.get(0)?,
                    profile_id: row.get(1)?,
                    profile_name: row.get(2)?,
                    provider_id: row.get(3)?,
                    report_type: row.get(4)?,
                    // Колонка отображения: период запроса (Ozon) ИЛИ дата документа (WB).
                    period: period.or(document_date.clone()),
                    file_path: row.get(6)?,
                    file_size: row.get(7)?,
                    file_format: row.get(8)?,
                    document_id: row.get(9)?,
                    document_date,
                    downloaded_at,
                })
            })
            .map_err(map_sqlite_err)?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    /// Список уникальных `report_type` среди скачанных файлов (для combo «Отчёт»
    /// в Архиве — показывает только то, что реально есть). Отсортирован по алфавиту.
    pub fn distinct_report_types(&self) -> CoreResult<Vec<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT report_type FROM downloads
                 ORDER BY report_type ASC",
            )
            .map_err(map_sqlite_err)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(map_sqlite_err)?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    /// Проверяет, есть ли уже файл с таким хэшем для профиля+отчёта+периода.
    pub fn has_download(
        &self,
        profile_id: i64,
        report_type: &str,
        period: Option<&str>,
        file_hash: &str,
    ) -> CoreResult<bool> {
        let conn = self.conn.lock();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM downloads
                 WHERE profile_id=?1 AND report_type=?2 AND period IS ?3 AND file_hash=?4",
                params![profile_id, report_type, period, file_hash],
                |row| row.get(0),
            )
            .map_err(map_sqlite_err)?;
        Ok(n > 0)
    }

    // ----- Сохранённые фильтры -----

    /// Сохраняет фильтр (с дедупликацией по имени).
    pub fn save_filter(
        &self,
        name: &str,
        provider_id: &str,
        report_type: &str,
        filter_json: &str,
    ) -> CoreResult<i64> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO saved_filters (name, provider_id, report_type, filter_json)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(name) DO UPDATE SET
                provider_id=excluded.provider_id,
                report_type=excluded.report_type,
                filter_json=excluded.filter_json,
                updated_at=CURRENT_TIMESTAMP",
            params![name, provider_id, report_type, filter_json],
        )
        .map_err(map_sqlite_err)?;
        Ok(conn.last_insert_rowid())
    }

    /// Список сохранённых фильтров.
    pub fn list_filters(&self) -> CoreResult<Vec<SavedFilter>> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT id, name, provider_id, report_type, filter_json FROM saved_filters ORDER BY name")
            .map_err(map_sqlite_err)?;
        let rows = stmt.query_map([], |row| {
            Ok(SavedFilter {
                id: row.get(0)?,
                name: row.get(1)?,
                provider_id: row.get(2)?,
                report_type: row.get(3)?,
                filter_json: row.get(4)?,
            })
        }).map_err(map_sqlite_err)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(map_sqlite_err)?);
        }
        Ok(out)
    }

    /// Удаляет сохранённый фильтр по имени.
    pub fn delete_filter(&self, name: &str) -> CoreResult<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM saved_filters WHERE name=?1", params![name])
            .map_err(map_sqlite_err)?;
        Ok(())
    }

    // ----- Расписания (schedules) для планировщика -----

    /// Создаёт или обновляет расписание. Возвращает id.
    pub fn upsert_schedule(&self, s: &NewSchedule) -> CoreResult<i64> {
        let reports_json = serde_json::to_string(&s.reports).unwrap_or_else(|_| "[]".into());
        let conn = self.conn.lock();
        if let Some(id) = s.id {
            conn.execute(
                "UPDATE schedules SET name=?1, profile_id=?2, reports=?3, cron_expr=?4,
                 period_offset=?5, params=?6, enabled=?7, next_run_at=?8 WHERE id=?9",
                params![s.name, s.profile_id, reports_json, s.cron_expr, s.period_offset, s.params, s.enabled, s.next_run_at_ts, id],
            )
            .map_err(map_sqlite_err)?;
            Ok(id)
        } else {
            conn.execute(
                "INSERT INTO schedules (name, profile_id, reports, cron_expr, period_offset, params, enabled, next_run_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![s.name, s.profile_id, reports_json, s.cron_expr, s.period_offset, s.params, s.enabled, s.next_run_at_ts],
            )
            .map_err(map_sqlite_err)?;
            Ok(conn.last_insert_rowid())
        }
    }

    /// Список всех расписаний.
    pub fn list_schedules(&self) -> CoreResult<Vec<ScheduleRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT id, name, profile_id, reports, cron_expr, period_offset, params, enabled, next_run_at, last_run_at, last_run_status FROM schedules ORDER BY name")
            .map_err(map_sqlite_err)?;
        let rows = stmt
            .query_map([], row_to_schedule)
            .map_err(map_sqlite_err)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(map_sqlite_err)?);
        }
        Ok(out)
    }

    /// Расписание по имени.
    pub fn get_schedule(&self, name: &str) -> CoreResult<Option<ScheduleRecord>> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, name, profile_id, reports, cron_expr, period_offset, params, enabled, next_run_at, last_run_at, last_run_status FROM schedules WHERE name=?1",
            params![name],
            row_to_schedule,
        )
        .optional()
        .map_err(map_sqlite_err)
    }

    /// Удаляет расписание.
    pub fn delete_schedule(&self, name: &str) -> CoreResult<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM schedules WHERE name=?1", params![name])
            .map_err(map_sqlite_err)?;
        Ok(())
    }

    /// Обновляет статус последнего запуска и следующий запуск.
    pub fn update_schedule_run(
        &self,
        id: i64,
        last_run_ts: Option<String>,
        status: &str,
        next_run_ts: Option<String>,
    ) -> CoreResult<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE schedules SET last_run_at=?1, last_run_status=?2, next_run_at=?3 WHERE id=?4",
            params![last_run_ts, status, next_run_ts, id],
        )
        .map_err(map_sqlite_err)?;
        Ok(())
    }

    // ----- Состояние UI (автосохранение между запусками) -----

    /// Сохраняет значение состояния UI по ключу (upsert). Значение — JSON-строка.
    pub fn set_ui_state(&self, key: &str, value_json: &str) -> CoreResult<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO ui_state (key, value_json, updated_at)
             VALUES (?1, ?2, CURRENT_TIMESTAMP)
             ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json, updated_at=CURRENT_TIMESTAMP",
            params![key, value_json],
        )
        .map_err(map_sqlite_err)?;
        Ok(())
    }

    /// Читает значение состояния UI по ключу. Возвращает None, если ключа нет.
    pub fn get_ui_state(&self, key: &str) -> CoreResult<Option<String>> {
        let conn = self.conn.lock();
        let v: Option<String> = conn
            .query_row(
                "SELECT value_json FROM ui_state WHERE key=?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_sqlite_err)?;
        Ok(v)
    }
}

/// Новое/обновляемое расписание.
#[derive(Debug, Clone)]
pub struct NewSchedule {
    pub id: Option<i64>,
    pub name: String,
    pub profile_id: i64,
    pub reports: Vec<String>,
    pub cron_expr: String,
    pub period_offset: i32,
    pub params: Option<String>,
    pub enabled: bool,
    pub next_run_at_ts: Option<String>,
}

/// Запись расписания из БД.
#[derive(Debug, Clone)]
pub struct ScheduleRecord {
    pub id: i64,
    pub name: String,
    pub profile_id: i64,
    pub reports: Vec<String>,
    pub cron_expr: String,
    pub period_offset: i32,
    pub params: Option<String>,
    pub enabled: bool,
    pub next_run_at: Option<String>,
    pub last_run_at: Option<String>,
    pub last_run_status: Option<String>,
}

fn row_to_schedule(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScheduleRecord> {
    let reports_json: String = row.get(3)?;
    let reports: Vec<String> = serde_json::from_str(&reports_json).unwrap_or_default();
    Ok(ScheduleRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        profile_id: row.get(2)?,
        reports,
        cron_expr: row.get(4)?,
        period_offset: row.get(5)?,
        params: row.get(6)?,
        enabled: row.get(7)?,
        next_run_at: row.get(8)?,
        last_run_at: row.get(9)?,
        last_run_status: row.get(10)?,
    })
}

/// Новая запись о скачивании (для `record_download`).
#[derive(Debug, Clone)]
pub struct NewDownload {
    pub profile_id: i64,
    pub report_type: String,
    pub period: Option<String>,
    pub params: Option<String>,
    pub file_path: String,
    pub file_size: i64,
    pub file_hash: Option<String>,
    pub file_format: String,
    pub rows_count: Option<i64>,
    pub downloader_kind: String,
    pub source_url: Option<String>,
    /// Идентификатор документа (WB serviceName). None для Period-отчётов.
    /// Используется для значка «уже загружен» в списке документов.
    pub document_id: Option<String>,
    /// Дата документа (WB creationTime → YYYY-MM-DD). None для Period-отчётов.
    /// Используется для фильтра периода в Архиве.
    pub document_date: Option<String>,
}

/// Краткая информация о скачанном документе (для значка «уже загружен»).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadedDocInfo {
    pub document_id: String,
    pub file_path: String,
    pub file_format: String,
    pub downloaded_at: DateTime<Utc>,
}

fn row_to_profile(row: &rusqlite::Row<'_>) -> rusqlite::Result<Profile> {
    let id: i64 = row.get(0)?;
    let name: String = row.get(1)?;
    let provider_id: String = row.get(2)?;
    let description: Option<String> = row.get(3)?;
    let auth_metadata_json: Option<String> = row.get(4)?;
    let keychain_id: Option<String> = row.get(5)?;

    let auth_metadata: std::collections::BTreeMap<String, String> = auth_metadata_json
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    Ok(Profile {
        id: Some(id),
        name,
        provider_id,
        description,
        auth_metadata,
        keychain_id,
    })
}

#[allow(clippy::needless_pass_by_value)]
fn map_sqlite_err(e: rusqlite::Error) -> CoreError {
    CoreError::Internal(format!("sqlite: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cat() -> Catalog {
        Catalog::open_in_memory().expect("open in-memory catalog")
    }

    #[test]
    fn profile_crud() {
        let cat = make_cat();
        let mut p = Profile::new("Ozon-1", "ozon").with_metadata("client_id", "123");
        let id = cat.upsert_profile(&p).unwrap();
        p.id = Some(id);
        assert!(id > 0);

        let list = cat.list_profiles().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Ozon-1");
        assert_eq!(list[0].metadata("client_id"), Some("123"));

        let got = cat.get_profile_by_name("Ozon-1").unwrap().unwrap();
        assert_eq!(got.id, Some(id));

        cat.delete_profile("Ozon-1").unwrap();
        assert!(cat.get_profile_by_name("Ozon-1").unwrap().is_none());
    }

    #[test]
    fn download_dedup() {
        let cat = make_cat();
        let id = cat.upsert_profile(&Profile::new("p", "ozon")).unwrap();
        let rec = NewDownload {
            profile_id: id,
            report_type: "ozon.realization".into(),
            period: Some("2026-06".into()),
            params: None,
            file_path: "/tmp/x.csv".into(),
            file_size: 100,
            file_hash: Some("abc".into()),
            file_format: "csv".into(),
            rows_count: None,
            downloader_kind: "Api".into(),
            source_url: None,
            document_id: None,
            document_date: None,
        };
        cat.record_download(&rec).unwrap();
        assert!(cat
            .has_download(id, "ozon.realization", Some("2026-06"), "abc")
            .unwrap());
        assert!(!cat
            .has_download(id, "ozon.realization", Some("2026-06"), "other")
            .unwrap());
    }

    #[test]
    fn list_downloaded_docs_by_document_id() {
        // Значок «уже загружен»: list_downloaded_docs возвращает документы с
        // заполненным document_id (serviceName). Без document_id — не возвращаются.
        let cat = make_cat();
        let id = cat.upsert_profile(&Profile::new("wb1", "wildberries")).unwrap();
        // Документ с document_id.
        let with_doc = NewDownload {
            profile_id: id,
            report_type: "wb.documents".into(),
            period: None,
            params: None,
            file_path: "/tmp/УПД №123.xml".into(),
            file_size: 500,
            file_hash: Some("h1".into()),
            file_format: "xml".into(),
            rows_count: None,
            downloader_kind: "Api".into(),
            source_url: None,
            document_id: Some("УПД-123-service".into()),
            document_date: None,
        };
        cat.record_download(&with_doc).unwrap();
        // Документ без document_id (Period-отчёт) — не должен попасть в список.
        let no_doc = NewDownload {
            profile_id: id,
            report_type: "ozon.balance".into(),
            period: Some("2026-07".into()),
            params: None,
            file_path: "/tmp/balance.xlsx".into(),
            file_size: 200,
            file_hash: Some("h2".into()),
            file_format: "xlsx".into(),
            rows_count: None,
            downloader_kind: "Api".into(),
            source_url: None,
            document_id: None,
            document_date: None,
        };
        cat.record_download(&no_doc).unwrap();

        let docs = cat.list_downloaded_docs(id, "wb.documents").unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].document_id, "УПД-123-service");
        assert_eq!(docs[0].file_path, "/tmp/УПД №123.xml");
        assert_eq!(docs[0].file_format, "xml");

        // По другому report_type — пусто.
        assert!(cat.list_downloaded_docs(id, "wb.orders").unwrap().is_empty());
    }

    #[test]
    fn migration_backfills_document_id_from_params() {
        // Симулируем старую БД: создаём downloads БЕЗ document_id (как до v3),
        // затем открываем каталог заново — миграция должна backfill-нуть
        // document_id из params.values["ids"].
        let cat = make_cat();
        let id = cat.upsert_profile(&Profile::new("wb2", "wildberries")).unwrap();
        // Вставляем строку напрямую без document_id, params содержит ids CSV.
        {
            let conn = cat.conn.lock();
            conn.execute(
                "INSERT INTO downloads (profile_id, report_type, period, params, file_path,
                 file_size, file_hash, file_format, rows_count, downloader_kind, source_url)
                 VALUES (?1, ?2, NULL, ?3, ?4, 100, 'h3', 'xml', NULL, 'Api', NULL)",
                params![
                    id,
                    "wb.documents",
                    r#"{"values":{"ids":"sid-1,sid-2"}}"#,
                    "/tmp/doc1.xml"
                ],
            )
            .unwrap();
        }
        // Переоткрываем — apply_schema запускает миграцию (но колонка уже есть из
        // schema.sql в in-memory). Эмулируем backfill напрямую: вызываем миграцию.
        let conn = cat.conn.lock();
        migrate_add_document_id(&conn).unwrap();
        drop(conn);
        // Проверяем, что document_id заполнен первым id из CSV.
        let docs = cat.list_downloaded_docs(id, "wb.documents").unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].document_id, "sid-1");
    }

    #[test]
    fn extract_first_id_from_params_variants() {
        // CSV ids.
        assert_eq!(
            extract_first_id_from_params(r#"{"values":{"ids":"a,b,c"}}"#),
            Some("a".into())
        );
        // doc_meta массив.
        assert_eq!(
            extract_first_id_from_params(r#"{"values":{"doc_meta":[{"id":"x","name":"n"}]}}"#),
            Some("x".into())
        );
        // Нет ни ids, ни doc_meta.
        assert_eq!(extract_first_id_from_params(r#"{"values":{}}"#), None);
        // Не JSON.
        assert_eq!(extract_first_id_from_params("not json"), None);
    }

    #[test]
    fn saved_filters_crud() {
        let cat = make_cat();
        cat.save_filter("monthly-upd", "wildberries", "wb.documents", r#"{"category":"upd"}"#)
            .unwrap();
        let filters = cat.list_filters().unwrap();
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].name, "monthly-upd");
        cat.delete_filter("monthly-upd").unwrap();
        assert!(cat.list_filters().unwrap().is_empty());
    }

    #[test]
    fn list_downloads_filtered_all_and_combinations() {
        // Архив: list_downloads_filtered возвращает ВСЕ строки (включая Period-отчёты
        // без document_id), JOIN с profiles даёт profile_name/provider_id.
        // Проверяем фильтры: все / по профилю / по отчёту / по диапазону (пересечение).
        let cat = make_cat();
        let oz = cat.upsert_profile(&Profile::new("OzonProd", "ozon")).unwrap();
        let wb = cat.upsert_profile(&Profile::new("WbProd", "wildberries")).unwrap();

        // Helper для вставки записи скачивания. document_date — для WB-документов.
        let mk = |profile_id: i64,
                  report_type: &str,
                  period: Option<&str>,
                  format: &str,
                  hash: &str,
                  document_id: Option<&str>,
                  document_date: Option<&str>| NewDownload {
            profile_id,
            report_type: report_type.into(),
            period: period.map(str::to_string),
            params: None,
            file_path: format!("/tmp/{hash}.file"),
            file_size: 100,
            file_hash: Some(hash.into()),
            file_format: format.into(),
            rows_count: None,
            downloader_kind: "Api".into(),
            source_url: None,
            document_id: document_id.map(str::to_string),
            document_date: document_date.map(str::to_string),
        };
        // Строки:
        //  h1: ozon.realization period=2026-07            (июль, месяц целиком)
        //  h2: ozon.balance      period=2026-06            (июнь, месяц целиком)
        //  h3: wb.documents      document_date=2026-07-15  (точка, 15 июля)
        //  h4: ozon.realization  period=2026-06            (июнь)
        //  h5: wb.documents      без даты (старая запись)  — выпадает из фильтра по периоду
        cat.record_download(&mk(oz, "ozon.realization", Some("2026-07"), "xlsx", "h1", None, None))
            .unwrap();
        cat.record_download(&mk(oz, "ozon.balance", Some("2026-06"), "xlsx", "h2", None, None))
            .unwrap();
        cat.record_download(&mk(wb, "wb.documents", None, "xml", "h3", Some("svc-1"), Some("2026-07-15")))
            .unwrap();
        cat.record_download(&mk(oz, "ozon.realization", Some("2026-06"), "xlsx", "h4", None, None))
            .unwrap();
        cat.record_download(&mk(wb, "wb.documents", None, "xml", "h5", Some("svc-2"), None))
            .unwrap();

        // Все — 5 строк.
        let all = cat.list_downloads_filtered(None, None, None).unwrap();
        assert_eq!(all.len(), 5);
        let e0 = all.iter().find(|e| e.id > 0).unwrap();
        // Поля JOIN: profile_name и provider_id заполнены.
        let oz_e = all
            .iter()
            .find(|e| e.report_type == "ozon.realization" && e.period.as_deref() == Some("2026-07"))
            .unwrap();
        assert_eq!(oz_e.profile_name, "OzonProd");
        assert_eq!(oz_e.provider_id, "ozon");
        let wb_e = all
            .iter()
            .find(|e| e.document_id.as_deref() == Some("svc-1"))
            .unwrap();
        assert_eq!(wb_e.profile_name, "WbProd");
        assert_eq!(wb_e.provider_id, "wildberries");
        // WB-документ: period None в БД, но document_date есть → колонка отображения
        // (entry.period) показывает document_date как fallback.
        assert_eq!(wb_e.document_date.as_deref(), Some("2026-07-15"));
        assert_eq!(wb_e.period.as_deref(), Some("2026-07-15")); // COALESCE в DTO
        // Элемент без document_id (Period-отчёт) тоже попадает в архив.
        assert!(oz_e.document_id.is_none());

        // Фильтр по профилю OzonProd → 3 строки (h1, h2, h4).
        let by_prof = cat.list_downloads_filtered(Some(oz), None, None).unwrap();
        assert_eq!(by_prof.len(), 3);
        assert!(by_prof.iter().all(|e| e.provider_id == "ozon"));

        // Фильтр по отчёту wb.documents → 2 строки (h3, h5).
        let by_rt = cat
            .list_downloads_filtered(None, Some("wb.documents"), None)
            .unwrap();
        assert_eq!(by_rt.len(), 2);
        assert!(by_rt.iter().all(|e| e.report_type == "wb.documents"));

        // Фильтр по диапазону: Июль 2026 → [2026-07-01, 2026-07-31].
        // Должны попасть: h1 (period 2026-07, месяц целиком) и h3 (doc_date 15 июля).
        // НЕ попадают: h2/h4 (июнь), h5 (без даты).
        let july = cat
            .list_downloads_filtered(None, None, Some(("2026-07-01".into(), "2026-07-31".into())))
            .unwrap();
        assert_eq!(july.len(), 2, "июль должен дать h1 + h3");
        // h1 — Period-отчёт ozon.realization (без document_id).
        assert!(july.iter().any(|e| e.report_type == "ozon.realization" && e.document_id.is_none()));
        // h3 — WB-документ svc-1 (document_date 15 июля).
        assert!(july.iter().any(|e| e.document_id.as_deref() == Some("svc-1")));

        // Фильтр по диапазону: Июнь 2026 → h2, h4.
        let june = cat
            .list_downloads_filtered(None, None, Some(("2026-06-01".into(), "2026-06-30".into())))
            .unwrap();
        assert_eq!(june.len(), 2);
        assert!(june.iter().all(|e| e.report_type != "wb.documents"));

        // Граничный случай: 2026-07-31 (последний день июля) — h1 (месяц целиком до 31) попадает.
        let last_day = cat
            .list_downloads_filtered(None, None, Some(("2026-07-31".into(), "2026-07-31".into())))
            .unwrap();
        assert_eq!(last_day.len(), 1);
        assert_eq!(last_day[0].report_type, "ozon.realization");

        // Комбинация: OzonProd + ozon.realization + июль → 1 строка (h1).
        let combo = cat
            .list_downloads_filtered(Some(oz), Some("ozon.realization"), Some(("2026-07-01".into(), "2026-07-31".into())))
            .unwrap();
        assert_eq!(combo.len(), 1);
        assert_eq!(combo[0].profile_name, "OzonProd");

        // WB-документ БЕЗ даты (h5) НЕ попадает ни в какой фильтр по диапазону.
        let h5_in_july = cat
            .list_downloads_filtered(None, Some("wb.documents"), Some(("2026-07-01".into(), "2026-07-31".into())))
            .unwrap();
        assert!(h5_in_july.iter().all(|e| e.document_id.as_deref() != Some("svc-2")));

        // Отсутствующий профиль → пусто.
        assert!(cat.list_downloads_filtered(Some(9999), None, None).unwrap().is_empty());

        // Утилизация e0 (избегаем dead_code в тесте): валидная запись с id.
        assert!(e0.id > 0);
    }

    #[test]
    fn distinct_report_types_sorted() {
        let cat = make_cat();
        let id = cat.upsert_profile(&Profile::new("p", "ozon")).unwrap();
        let mk = |rt: &str, hash: &str| NewDownload {
            profile_id: id,
            report_type: rt.into(),
            period: None,
            params: None,
            file_path: format!("/tmp/{hash}"),
            file_size: 1,
            file_hash: Some(hash.into()),
            file_format: "csv".into(),
            rows_count: None,
            downloader_kind: "Api".into(),
            source_url: None,
            document_id: None,
            document_date: None,
        };
        cat.record_download(&mk("wb.orders", "h1")).unwrap();
        cat.record_download(&mk("ozon.balance", "h2")).unwrap();
        cat.record_download(&mk("ozon.balance", "h3")).unwrap(); // дубликат отчёта
        let rts = cat.distinct_report_types().unwrap();
        // Уникальные + отсортированы по алфавиту.
        assert_eq!(rts, vec!["ozon.balance", "wb.orders"]);

        // Пустая БД — пустой список.
        let empty = make_cat();
        assert!(empty.distinct_report_types().unwrap().is_empty());
    }
}
