# План: Marketplace Downloader Framework (MDWF) v1.4

## Решения зафиксированы
- **Объём:** полный фреймворк по спеке (9 крейтов, 44 отчёта, GUI + CLI + scheduler + REST API).
- **GUI:** GTK4 + libadwaita (ADR-002), сборка через MSYS2/MinGW в `D:\msys64\mingw64`.
- **Модель документов:** оба режима — *Browsable* (список→фильтр→выбор→скачать: WB Documents API, Ozon transaction-list/accrual-postings/b2b-sales/mutual-settlement) и *Period* (тип+период→генерация→скачать: Ozon realization/compensation, WB sales-reports/detailed). Это расширяет трейт `Report` из спеки.

## Ключевое расширение против спеки (мотивировано требованием интерактивного UI)
В `core` добавляю понятие режима получения, поверх `DownloaderKind`:

```rust
pub enum AcquisitionMode { Period, Browsable }

pub struct DocumentEntry {
    pub id: String,                 // provider-native id
    pub display_name: String,
    pub category: String,
    pub date: Option<NaiveDate>,
    pub extensions: Vec<String>,    // xml/pdf/xlsx/zip
    pub size_hint: Option<u64>,
    pub metadata: serde_json::Value,
}

// В трейте Report добавляется:
fn acquisition_mode(&self) -> AcquisitionMode;
async fn list(auth, params, cancel) -> CoreResult<Vec<DocumentEntry>>; // только Browsable
// download() остаётся как в спеке: для Period генерит, для Browsable качает выбранные DocumentEntry.id
```

UI-цикл: `выбор провайдера/отчёта → ввод фильтров → List (Browsable) или предпросмотр периодов (Period) → выбор чекбоксами → Download → запись на диск + каталог SQLite`.

---

## ЭТАП 0 — Bootstrap среды (сначала, быстро)
1. В корне `D:\work\Learn\ZCode\MPDocsLoad` создаю `rust-toolchain.toml` → пин `stable-x86_64-pc-windows-gnu` (GTK-библиотеки MinGW-сборки, msvc с ними не линкуется).
2. Скрипт окружения `scripts/env.sh` / `scripts/env.ps1`: выставляет `PKG_CONFIG_PATH=D:/msys64/mingw64/lib/pkgconfig`, добавляет `D:/msys64/mingw64/bin` в PATH. Проверка: `pkg-config --modversion gtk4 libadwaita-1`.
3. `git init`, `.gitignore` (target/, *.exe, временные БД).
4. Критерий: `cargo +gnu new`/build пустого хеллоу с gtk4 компилируется.

## ЭТАП 1 — Скелет workspace (гл.03/17, этап 2)
- Корневой `Cargo.toml` (workspace resolver=2, members из 9 крейтов, `[workspace.dependencies]` из спеки).
- Версии gtk-rs выравниваю: `gtk4 = "0.9"`, `libadwaita = { version = "0.7", features=["v1_5"] }`, **`glib = "0.20"`, `gio = "0.20"`** (gtk4 0.9 требует glib/gio 0.20 — в спеке опечатка `0.19`).
- `rustfmt.toml`, `clippy.toml` (`clippy::pedantic`), `deny.toml`.
- Пустые крейты `mdwf-core|storage|secrets|scheduler|providers-ozon|providers-wildberries|cli|gui|api`, в каждом `lib.rs`/`main.rs` с минимальным заглушкой.
- Критерий: `cargo build --workspace` проходит.

## ЭТАП 2 — Core-трейты (гл.09, этап 3)
`mdwf-core/src/`: `provider.rs`, `auth.rs` (Authenticator+AuthType), `report.rs` (Report + ReportCategory + **AcquisitionMode + DocumentEntry**), `downloader.rs`, `capabilities.rs` (Capabilities, AuthField, AuthFieldKind), `registry.rs` (ProviderRegistry с RwLock<HashMap>), `profile.rs`, `params.rs`, `progress.rs`, `health.rs`, `pagination.rs`, `secret.rs` (SecretString-обёртка), `error.rs` (CoreError через thiserror). Без упоминаний маркетплейсов.
- Критерий: трейты определены, компилируются, минимальные unit-тесты.

## ЭТАП 3 — Storage + Secrets (гл.06/07, этап 4)
- `mdwf-storage`: `schema.sql` (profiles/downloads/schedules из §2.7.2), `catalog.rs` (rusqlite bundled), `migrations/`, `file_store.rs`, `naming.rs` (шаблон `{provider}_{profile}_{report}_{period}.{ext}`), `dedup.rs` (SHA-256).
- `mdwf-secrets`: trait `SecretStore`, реализация через `keyring` (Windows Credential Manager), `memory.rs` mock.
- Критерий: миграции применяются, секрет сохраняется/читается, дедупликация работает (тесты).

## ЭТАП 4 — TestProvider mock (этап 5)
В `mdwf-core` (или тестовом крейте) — `TestProvider`, возвращающий фейковые DocumentEntry/DownloadedFile. Нужен, чтобы GUI/CLI заработали до реальных провайдеров.
- Критерий: GUI показывает mock-документы.

## ЭТАП 5 — GUI: основа GTK4+libadwaita (гл.04, этап 7 — поднимаем раньше, т.к. нужно для интерактивного тестирования)
- `mdwf-gui`: `main.rs` (adw::Application `dev.mdwf.MDWF`), `app.rs` (MdwfApp), `theme.rs` (брендовый CSS + ColorScheme), навигация через GtkStack, каналы `mpsc` UI↔tokio.
- Связь GTK↔tokio: tokio-задачи шлют события через `glib::MainContext` (никакой бизнес-логики в UI).
- Критерий: окно открывается, переключаются вкладки.

## ЭТАП 6 — GUI: представления
- `views/profiles.rs` + `profile_edit.rs` (динамическая форма из `AuthField[]` — код из §2.5.3).
- `views/reports.rs` — выбор провайдера/отчёта, параметры (фильтры: категория, дата с/по, расширения).
- `views/download.rs` — **ядро интерактивного цикла**: кнопка «Список» → GtkListView/GtkColumnView с чекбоксами DocumentEntry → «Скачать выбранные» → прогресс (GtkProgressBar) → результат.
- `views/settings.rs` — редактор config.toml с сохранением (хуки к ЭТАПУ 7).
- `views/scheduler.rs`, `views/logs.rs`, `views/about.rs`.
- `widgets/dynamic_form.rs`, `widgets/file_tree.rs`, `widgets/progress_bar.rs`.
- Критерий: на mock-провайдере полный цикл список→выбор→загрузка в папку работает.

## ЭТАП 7 — Settings/Config Store (гл.06, частично нужен с ЭТАПОМ 6)
- TOML-конфиг `config.toml` (§2.7.1) + персистентные **сохранённые фильтры журнала документов** (отдельная SQLite-таблица `saved_filters` или секция в TOML — выберу SQLite для надёжности). Загрузка/сохранение между запусками.
- Критерий: настройки и фильтры переживают рестарт.

## ЭТАП 8 — OzonProvider (гл.11, этап 6)
- `providers/ozon`: `auth.rs` (OzonAuthenticator, Client-Id+Api-Key, TTL 180дн.), `client.rs` (rate limit 50 RPS, retry policy §2.8.2, circuit breaker), `date_format.rs` (3 форматтера), `pagination.rs` (Pages/Cursor/Offset), `capabilities.rs`.
- `reports/` — 20 отчётов, каждый файл. Browsable: transaction-list(⚠️deprecated→флаг), accrual-postings, accrual/by-day, b2b-sales, mutual-settlement, compensation, decompensation, cash-flow-statement. Period: realization(v2/by-day/posting), buyout, balance, act-discrepancy, analytics. Health-check через /v1/finance/balance.
- Feature-флаг `use_deprecated_transaction_list` (отключение 6 июля 2026 уже учтено — метод выдаёт DEPRECATED_METHOD).
- Критерий: на тестовых ключах (или mock-сервере) все 20 отчётов отдают данные.

## ЭТАП 9 — WildberriesProvider (гл.12, этап 11)
- `providers/wildberries`: `auth.rs` (WbAuthenticator, Authorization БЕЗ «Bearer», 4 типа токенов), 5 subclients (finance/documents/statistics/analytics/returns), `date_format.rs` (RFC3339 MSK UTC+3), `pagination.rs` (RrdidCursor/DateCursor/OffsetLimit/TaskId).
- `reports/`: balance, sales-reports(list+detailed), acquiring, **documents (УПД/УКД/акты — 3-шаговый паттерн §2.10.3, batch по 50)**, acceptance_report (async poll), orders, sales, deductions, measurement-penalties, antifraud, claims. Динамическое обнаружение категорий через GET /documents/categories.
- Строгое требование: `git diff crates/core` пуст после добавления WB (Framework First).
- Критерий: 24 отчёта работают, ядро не изменено.

## ЭТАП 10 — CLI (гл.05, этап 8)
- `mdwf-cli`: clap-подкоманды providers/profiles/reports/download/schedule/out-of-scope/doctor, exit-коды из §2.6.2. Общий доменный код с GUI.

## ЭТАП 11 — Scheduler (гл.08, этап 9)
- `mdwf-scheduler`: cron (1-го числа 02:00 и т.д.), очередь, retry, персистентность в SQLite `schedules`, автозапуск в Windows (реестр/планировщик задач), `max_parallel_jobs`.

## ЭТАП 12 — Out-of-scope + REST API (этапы 10 + опционально api-крейт)
- `out-of-scope` выводит 8 документов (5 Ozon + 3 WB) с инструкциями.
- `mdwf-api`: axum-роуты за feature-флагом.

## ЭТАП 13 — Тестирование (гл.15, этап 12)
- Unit (≥ целевых по core), integration с mock-сервером (wiremock/httpmock), E2E через CLI, gtk_test для виджетов (§2.12.2). Покрытие ≥80% core.

## ЭТАП 14 — Релиз v1.4 (этап 14)
- Бандл GTK-рантайма (DLL из `D:/msys64/mingw64/bin` рядом с .exe, ~70MB), smoke-тесты на Win11.

---

## Принципы выполнения (как будем идти)
- **Поэтапное утверждение (гл.17):** ни один этап не начинается без вашего «да». После каждого — показываю результат и критерий выполнения.
- **Стандарты кода (гл.16):** никаких `unwrap/expect/panic` вне тестов, `?`+`context`, `tracing` вместо `println`, `Result` везде, секреты через SecretString и не в логах. `cargo fmt` + `clippy::pedantic -D warnings`.
- **Архитектурные ограничения:** ядро не упоминает маркетплейсы; добавление WB = пустой `git diff crates/core`.

## Что нужно от вас для запуска реальных запросов (не блокирует разработку — пойдём на mock/httpmock)
- Ozon: Client-Id + Api-Key (Admin-роль для b2b-sales/mutual-settlement).
- WB: токен (тип Personal).
- Без них этапы 8/9 покрываются mock-сервером; реальные ключи подключим на ваших данных.

## Старт
Начну с **ЭТАПА 0 (bootstrap среды) + ЭТАПА 1 (скелет workspace)** как одной связки, чтобы быстро получить собирающийся проект, и остановлюсь на подтверждение перед трейтами core.

(Просьба по разрешениям Bash: сборка/запуск cargo, установка зависимостей, запуск тестов, git-операции в проекте.)