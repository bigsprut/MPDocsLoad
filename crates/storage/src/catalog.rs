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

/// Версия схемы для совместимости (спец. `schema_version = 2` в config.toml).
pub const SCHEMA_VERSION: u32 = 2;

/// Встроенная схема (создаётся при первом подключении).
const SCHEMA_SQL: &str = include_str!("../schema.sql");

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
        conn.execute_batch(SCHEMA_SQL).map_err(map_sqlite_err)
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

    /// Удаляет **все** профили. Используется при переходе на keyring-only
    /// хранение секретов: старые профили (с секретами в `auth_metadata`)
    /// становятся нерабочими, поэтому сбрасываются — пользователь создаёт
    /// их заново, секреты уходят в keyring.
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
              file_format, rows_count, downloader_kind, source_url)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(profile_id, report_type, period, file_hash) DO UPDATE SET
                 file_path=excluded.file_path, downloaded_at=CURRENT_TIMESTAMP",
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
            ],
        )
        .map_err(map_sqlite_err)?;
        Ok(conn.last_insert_rowid())
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
}
