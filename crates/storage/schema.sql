-- Схема SQLite-каталога MDWF (спец. §2.7.2).
-- Применяется при первом запуске; миграции — в migrations/.
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;

-- Профили учётных данных.
CREATE TABLE IF NOT EXISTS profiles (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL UNIQUE,
    provider_id     TEXT NOT NULL,
    description     TEXT,
    auth_metadata   TEXT,                       -- JSON: не-секретные поля
    keychain_id     TEXT,                       -- ключ в OS keychain
    created_at      TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at      TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_check_at   TIMESTAMP,
    last_check_ok   BOOLEAN
);

-- Выгруженные файлы (каталог + дедупликация).
CREATE TABLE IF NOT EXISTS downloads (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id      INTEGER NOT NULL,
    report_type     TEXT NOT NULL,
    period          TEXT,
    params          TEXT,                       -- JSON: параметры выгрузки
    file_path       TEXT NOT NULL,
    file_size       INTEGER NOT NULL,
    file_hash       TEXT,                       -- SHA-256
    file_format     TEXT NOT NULL,
    rows_count      INTEGER,
    downloader_kind TEXT NOT NULL,
    source_url      TEXT,
    downloaded_at   TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (profile_id) REFERENCES profiles(id) ON DELETE CASCADE,
    UNIQUE(profile_id, report_type, period, file_hash)
);

-- Расписания планировщика.
CREATE TABLE IF NOT EXISTS schedules (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL UNIQUE,
    profile_id      INTEGER NOT NULL,
    reports         TEXT NOT NULL,              -- JSON: список типов отчётов
    cron_expr       TEXT NOT NULL,
    period_offset   INTEGER NOT NULL DEFAULT 0,
    params          TEXT,                       -- JSON
    enabled         BOOLEAN NOT NULL DEFAULT 1,
    next_run_at     TIMESTAMP,
    last_run_at     TIMESTAMP,
    last_run_status TEXT,
    FOREIGN KEY (profile_id) REFERENCES profiles(id) ON DELETE CASCADE
);

-- Сохранённые фильтры журнала документов (требование пользователя:
-- сохранение настроек фильтров между запусками).
CREATE TABLE IF NOT EXISTS saved_filters (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL UNIQUE,
    provider_id     TEXT NOT NULL,
    report_type     TEXT NOT NULL,
    filter_json     TEXT NOT NULL,              -- JSON: DocumentFilter
    created_at      TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at      TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- История запусков расписаний.
CREATE TABLE IF NOT EXISTS schedule_runs (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    schedule_id     INTEGER NOT NULL,
    started_at      TIMESTAMP NOT NULL,
    finished_at     TIMESTAMP,
    status          TEXT NOT NULL,              -- ok/failed/partial/running
    error           TEXT,
    files_count     INTEGER,
    FOREIGN KEY (schedule_id) REFERENCES schedules(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_downloads_profile ON downloads(profile_id);
CREATE INDEX IF NOT EXISTS idx_downloads_period  ON downloads(period);
CREATE INDEX IF NOT EXISTS idx_schedules_next   ON schedules(next_run_at) WHERE enabled = 1;
CREATE INDEX IF NOT EXISTS idx_filters_provider ON saved_filters(provider_id);
