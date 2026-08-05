# MDWF — Статус проекта и контекст для продолжения работы

> **Этот файл — для передачи контекста в новый диалог.**
> Прочитайте его целиком перед продолжением работы над MDWF.

---

## 1. Что это за проект

Marketplace Downloader Framework (MDWF) v1.4 — desktop-приложение на Rust
для выгрузки финансовых документов с маркетплейсов **Ozon** и **Wildberries**
через их официальные API. GTK4 + libadwaita GUI, CLI, cron-планировщик.

**Спецификация:** `MarketplaceDownloaderFramework_TechnicalDoc_v1.4_2026-07-10.md`
в корне проекта.

**Рабочая директория:** `D:\work\Learn\ZCode\MPDocsLoad`

---

## 2. Среда разработки

- **Rust:** stable-x86_64-pc-windows-gnu (GTK-библиотеки MinGW-сборки из MSYS2)
- **GTK4 4.20.3 + libadwaita 1.8.3** в `D:\msys64\mingw64`
- **Запуск:** `source scripts/env.sh && cargo run -p mdwf-gui`
- **CLI:** `cargo run -p mdwf-cli -- providers list`
- **PKP_CONFIG_PATH** установлен в `.cargo/config.toml`
- **rust-toolchain.toml** пинит gnu-тулчейн

---

## 3. Структура проекта (12 крейтов)

```
crates/
├── core/              — трейты, типы, реестр (провайдер-агностик)
├── storage/           — SQLite (Catalog) + FileStore + дедуп SHA-256
├── secrets/           — OS keychain (keyring) + in-memory mock
├── scheduler/         — cron + retry + автозапуск Windows (HKCU Run)
├── config/            — config.toml + пути (%APPDATA%\mdwf)
├── test-provider/     — TestProvider mock
├── providers/ozon/    — Ozon Seller API (16 отчётов; accrual_types и b2b_sales_json удалены)
├── providers/wildberries/ — WB OpenAPI (14 отчётов)
├── cli/               — mdwf CLI (clap)
├── gui/               — mdwf-gui (GTK4 + libadwaita)
└── api/               — REST API (feature 'server', axum)
```

**Данные приложения:**
- Конфиг/БД: `C:\Users\MAN-MADE\AppData\Roaming\mdwf\` (config.toml, mdwf.db)
- Файлы: `C:\Users\MAN-MADE\Documents\MDWF\downloads\{provider}\{period}\`

---

## 4. Текущее состояние (что работает)

### ✅ Полностью работает
- GUI: главное окно, 7 вкладок, ToolbarView с кнопками управления окном
- CLI: все подкоманды (providers/profiles/reports/download/schedule/out-of-scope/doctor)
- Управление профилями: динамическая форма из capabilities (правильные ключи token/client_id/api_key)
- Список профилей: Gio.ListStore, живая перерисовка
- Вкладка «Загрузка»: самодостаточный поток Магазин→Профиль→Отчёт→Фильтры→Скачивание
- Календарь: MenuButton + gtk4::Calendar в Popover для выбора дат
- Автосохранение: состояние экрана «Загрузка» в SQLite (ui_state таблица)
- Период по умолчанию: последний год (диапазон) + прошлый месяц
- Запись файлов на диск + показ полных путей + кнопка «Открыть папку»
- Дедупликация: SHA-256 + SQLite UNIQUE
- Настройки: редактирование config.toml в UI
- Scheduler: cron + retry + автозапуск Windows
- REST API: axum за feature flag
- 105+ тестов, clippy --workspace --tests чист

### ✅ Недавно исправлено (подтверждено работающим)
- **Категории WB загружаются при выборе wb.documents** — РЕШЕНО.
  Корневая причина была **гонкой (race condition)**: устаревший ответ
  `LoadReports("ozon")` приходил позже, чем пользователь переключался на
  `wildberries`, и затирал список чужими `ozon.*` отчётами → `wb.documents`
  не появлялся → `maybe_request_categories()` никогда не срабатывал.
  **Фикс** (`download.rs::on_reports_loaded`): сверка `provider_id` результата
  с текущим выбранным провайдером; если не совпадает — устаревший результат
  игнорируется. Дополнительно: `REPORT_CHANGED_HANDLER` блокирует
  `connect_changed` на время программной перестройки combo (убрал каскадный шум).
  Подтверждено логами: `LoadDocumentCategories result: Ok(61)` и категории отрисованы.
- **Категории WB:** combo виден только для wb.documents (сравнение с type_id, не display_name)
- **Категории WB:** загружаются автоматически при выборе wb.documents (maybe_request_categories)
- **wb.documents:** category опциональна (дока: без category = все категории)
- **«Категории документов» убран из списка отчётов** (это служебный метод, не отчёт)

### ✅ Русские названия категорий в выпадающем списке (сделано)
В combo категорий показывается человекочитаемый `label` (русский `title`,
напр. «УПД») вместо технического `name` (напр. `upd`). Разделение отображаемого
имени и значения для API через `DocumentCategoryInfo { label, value }`:
- `channels.rs` — тип `DocumentCategoryInfo`, событие `DocumentCategoriesLoaded(Result<Vec<DocumentCategoryInfo>, String>)`.
- `app.rs::load_document_categories` — `label = title.unwrap_or(name)`, `value = name`.
- `reports.rs::WbCategoriesReport::list` — `display_name = c.title.unwrap_or(c.name)`.
- `download.rs::on_document_categories_loaded` — combo наполняется `label`,
  карта `label → value` хранится в thread-local `CATEGORIES`.
- `download.rs::build_filter` — выбранный `label` переводится в `value` через `CATEGORIES`
  перед отправкой в WB API; если не найден — категория не передаётся (все категории).

### ✅ Человекочитаемые имена документов WB (список + файлы на диске) (сделано)
Раньше в списке документов и в именах файлов на диске фигурировал технический
`serviceName` (напр. `upd-44841941`). Сверились с официальной OpenAPI-спецификацией
WB (зеркало `github.com/eslazarev/wildberries-sdk`, т.к. `dev.wildberries.ru` за антиботом):

**Схема `/api/v1/documents/list`** (`GetListDataDocumentsInner`):
`serviceName`, `name` (←человекочитаемое), `category`, `extensions`, `creationTime`, `viewed`.
Поля `amount`/`date` в реальной схеме **отсутствуют** — были написаны наугад и удалены.

**Схема `/api/v1/documents/download`** (`GetDocData`): `{fileName, extension, document(base64)}`.

Изменения по слоям:
- **`documents.rs::WbDocument`** — приведён к схеме: `serviceName/name/category/extensions/creationTime/viewed`
  (Option); убраны несуществующие `amount`/`date`.
- **`documents.rs::wb_document_to_entry`** — `display_name` из `name` (fallback: category → id),
  `extensions` реальные, дата из `creationTime`; убран некорректный `size_hint = amount`.
- **`documents.rs::download_one`** — возвращает `WbDownloadedDoc { bytes, extension, file_name }`,
  парся реальный `extension`/`fileName` из ответа (раньше выбрасывались). Батч `download_batch` удалён.
- **`reports.rs::WbDocumentsReport::download`** — **поштучное** скачивание (1 док = 1 файл),
  читает `doc_meta` из `params`, ставит `source_id = name` (человекочитаемое),
  реальное `extension` из ответа WB. Батч ≤50 → `wb_documents_batch.zip` убран.
- **`channels.rs`** — новый тип `DocumentSel { id, name, extension }`;
  `UiCommand::Download.document_ids: Vec<String>` → `documents: Vec<DocumentSel>`.
- **`app.rs::do_download`** — маршалит name+extension в `params["doc_meta"]` (JSON),
  сохраняя `ids` CSV для CLI-совместимости.
- **`download.rs`** — `CHECKS` хранит `DocumentSel` (id + display_name + первый формат);
  обработчик «Скачать выбранные» собирает `Vec<DocumentSel>`.
- **`naming.rs`** — `document_id` None → `"unknown"` (вместо пустоты); добавлена
  **нормализация**: unknown-сегменты вырезаются, повторы `_` схлопываются, `..` → `.`.
  Защищает Period-отчёты Ozon от мусора в имени при добавлении `{doc_id}`.
- **`settings.rs` + `file_store.rs`** (2 места правды) — дефолтный шаблон:
  `{provider}_{profile}_{report}_{doc_id}_{period}.{ext}`.

Примеры имён файлов после правки:
- Документ WB: `wildberries_Профиль_wb.documents_УПД №123.xml`
- Period-отчёт Ozon (doc_id нет, вырезан нормализацией): `ozon_Профиль_realization_2026-06.json`

**Проверки:** `cargo test --workspace` — 110+ тестов, 0 failed;
`cargo clippy --workspace --tests -- -D warnings` — чист. Ручная проверка вживую
(WB → wb.documents → список/скачивание) — не автоматизируется.

---

## 5. API WB — сверен с официальной документацией

**Источник:** dev.wildberries.ru (разделы: Баланс, Финансы, Документы, Отчёты, Аналитика)

### Домены (ВСЕ подтверждены nslookup + докой)
| Домен | Назначение |
|-------|-----------|
| `finance-api.wildberries.ru` | Баланс, отчёты реализации, эквайринг |
| `documents-api.wildberries.ru` | Категории, список, скачивание документов |
| `statistics-api.wildberries.ru` | Заказы, продажи |
| `seller-analytics-api.wildberries.ru` | Штрафы, антифрод, возвраты, приёмка |
| `returns-api.wildberries.ru` | Возвраты (claims) — по спеке, не проверено в доках |

**api.wildberries.ru НЕ существует (NXDOMAIN) — не использовать!**

### Эндпоинты WB (14 отчётов по спеке §2.2.2)

| type_id | Метод | Домен | Путь | Формат ответа |
|---------|-------|-------|------|---------------|
| wb.balance | GET | finance-api | /api/v1/account/balance | {currency,current,for_withdraw} |
| wb.sales_reports_list | POST | finance-api | /api/finance/v1/sales-reports/list | [...] прямой массив |
| wb.sales_reports_detailed | POST | finance-api | /api/finance/v1/sales-reports/detailed | [...] |
| wb.acquiring_list | POST | finance-api | /api/finance/v1/acquiring/list | [...] |
| wb.acquiring_detailed | POST | finance-api | /api/finance/v1/acquiring/detailed | [...] |
| wb.documents | GET | documents-api | /api/v1/documents/list | {data:{documents:[...]}} |
| wb.orders | GET | statistics-api | /api/v1/supplier/orders | [...] прямой массив |
| wb.sales | GET | statistics-api | /api/v1/supplier/sales | [...] прямой массив |
| wb.deductions | GET | seller-analytics | /api/analytics/v1/deductions | {data:{reports:[],total}} |
| wb.measurement_penalties | GET | seller-analytics | /api/analytics/v1/measurement-penalties | {data:{reports:[],total}} |
| wb.antifraud | GET | seller-analytics | /api/v1/analytics/antifraud-details | {details:[...]} |
| wb.claims | GET | returns-api | /api/v1/claims | [...] (по спеке, не подтверждено докой) |
| wb.acceptance_report | POST | seller-analytics | /api/v1/acceptance_report | {data:{taskId}} (async) |

### Documents API WB (детали)
- **categories:** GET /api/v1/documents/categories → `{data:{categories:[...]}}`
- **list:** GET /api/v1/documents/list, параметры: `locale, beginTime, endTime, sort, order, category, serviceName, limit(≤50), offset` → `{data:{documents:[...]}}`
- **download:** GET /api/v1/documents/download, параметры: `serviceName, extension` → `{data:{document:<base64>}}`
- **download/all:** POST /api/v1/documents/download/all, тело: `{params:[{serviceName,extension}]}` до 50 → `{data:{document:<base64>}}`
- **Rate limits documents:** 1 req/10s burst 5; download/all: 1 req/5min burst 5

### Авторизация WB
- Заголовок `Authorization: <token>` (БЕЗ префикса "Bearer ")
- 4 типа токенов: Personal, Service, Base, Test

---

## 6. API Ozon — сверен с официальной документацией

**Источник:** api-seller.ozon.ru, docs.ozon.ru/api/seller

### Авторизация Ozon
- Заголовки `Client-Id` + `Api-Key`
- TTL ключа: 6 месяцев (дока), в спеке 180 дней

### Эндпоинты Ozon (16 отчётов; accrual_types и b2b_sales_json удалены)

**Health-check:** POST /v1/finance/balance с `{date_from, date_to}` (макс 30 дней)

**Async-паттерн** (4 отчёта возвращают code → /v1/report/info → file URL):
- b2b_sales: POST /v1/finance/document-b2b-sales → {result:{code}} → /v1/report/info → file
- mutual_settlement: POST /v1/finance/mutual-settlement → code → /v1/report/info → file
- compensation: POST /v1/finance/compensation → code → /v1/report/info → file
- decompensation: POST /v1/finance/decompensation → code → /v1/report/info → file

**Реализован класс OzonAsyncReport** (3 шага: запрос→code→/v1/report/info→скачать XLSX)

~~**b2b_sales_json** — отдаёт JSON напрямую (без code).~~ (удалён)

---

## 7. Анти-бан меры

### WB
- Per-domain rate limiter (10с для documents, 60с для finance/analytics/returns, 6с для statistics)
- 429: чтение `X-Ratelimit-Retry` + `Retry-After` → backoff лимитера
- 429: max 3 попытки + человекочитаемая ошибка
- 5xx: экспонента 500мс→8с, 5 попыток

### Ozon
- Rate limiter 50 RPS (20мс интервал)
- 429: чтение `Retry-After` → backoff
- 429: max 3 попытки
- Circuit breaker: 5 ошибок → 5 минут

---

## 8. GUI — ключевые архитектурные решения

### Связь GTK ↔ tokio
- `async_channel` (НЕ glib::MainContext::channel — его нет в glib 0.20)
- UI→tokio: `mpsc::UnboundedSender<UiCommand>` (CommandSender)
- tokio→UI: `async_channel::Sender<UiEvent>` (EventForwarder)
- tokio-задачи читают receiver через `glib::MainContext::spawn_local`

### gtk-rs 0.9 API особенности
- `gtk4::prelude::*` + `libadwaita::prelude::*` нужны для trait-методов
- `ApplicationWindow::set_content` требует `libadwaita::prelude::*`
- `ToolbarView` + `HeaderBar` обязательно для кнопок управления окном на Windows
- `Popover::new()` + `popup()` крашит если не в дереве виджетов → использовать `MenuButton` + `set_popover()`
- `Calendar::set_active` требует `v4_10` → использовать `Popover::popdown()`
- `connect_changed` НЕ всегда срабатывает при `set_active` → вызывать хендлеры явно

### Thread-local паттерн для виджетов
```rust
thread_local! {
    static W_PROVIDER: Rc<RefCell<Option<ComboBoxText>>> = ...
    static W_CATEGORY_COMBO: Rc<RefCell<Option<ComboBoxText>>> = ...
    static CMD: Rc<RefCell<Option<CommandSender>>> = ...
}
```
Чтение через `.with(|w| w.borrow().clone())` — borrow_mut для set, clone для read.

### Типы команд/событий (channels.rs)
- UiCommand: LoadProviders, LoadProfiles, LoadAuthFields, LoadReports, LoadDocumentCategories, SaveProfile, DeleteProfile, CheckProfile, ListDocuments, Download, Cancel, SaveDownloadState, LoadDownloadState
- UiEvent: ProvidersLoaded, ProfilesLoaded, AuthFieldsLoaded, ReportsLoaded, DocumentCategoriesLoaded, DocumentsListed, DownloadFinished(DownloadResult), ProfileSaved/Deleted/Checked, Progress, Notify, DownloadStateLoaded

### DownloadResult (новый тип)
```rust
pub struct DownloadResult {
    pub files: Vec<DownloadedFile>,
    pub saved_paths: Vec<String>,
}
```

---

## 9. Известные проблемы и TODO

### Возможные баги (проверить)
1. **Автосохранение может восстанавливать test.documents** при провайдере WB — stale state. Защита от гонки в `on_reports_loaded` (сверка provider_id) снижает риск, но `test.documents` в сохранённом состоянии всё ещё может выбраться.

### Не реализовано
1. **Async-отчёты WB** (warehouse_remains, paid_storage, acceptance_report) — create→poll→download паттерн не реализован, только create-фаза
2. **max_parallel_jobs** планировщика — счётчик running не декрементируется
3. **TestProvider в release** — должен быть только dev
4. **Очистка rate-limiter от ожидания 60с в тестах** — `MDWF_WB_NO_RATELIMIT=1`

### ✅ Недавно сделано
- **Пагинация `/api/v1/documents/list`** (сделано). WB отдаёт максимум 50 документов
  за запрос, поля `total` в ответе нет (сверено со схемой `GetListData`).
  `WbDocumentsReport::list` теперь перебирает страницы по 50, пока не получит
  неполную (признак конца) или не наберёт `filter.limit` (`None` = без потолка,
  выгружаем все). Страховочный потолок — 200 страниц (10 000 документов).
  Запросы идут через per-domain rate-limiter (1 req/10с burst 5). Интеграционный
  тест `documents_api_paginates_list` проверяет truncate по ceiling и полную выгрузку.
- **Имя файла на диске = fileName из ответа /download** (сделано). Поле `name`
  ответа `/documents/list` у WB оказалось пустым/общим (напр. «Акт»), поэтому
  файлы сохранялись как `wildberries_wb1_wb.documents.zip` — без названия документа.
  Реальное осмысленное имя WB сообщает в `fileName` ответа `/download`
  (это то, что лежит внутри zip: «Акт №072600203230 от 26.07.2026.pdf»).
  Теперь `WbDocumentsReport::download` берёт `source_id` = `fileName`
  (с отрезанным расширением через `strip_extension`), fallback: `name` из меты
  UI → `serviceName`. `strip_extension` отличает точку-расширение от точки-даты
  (сегмент ≤5 симв, ASCII-alphanumeric, есть буква) — иначе «.2026» в дате
  отрезалось бы ошибочно.
- **Живой прогресс при загрузке списка документов** (сделано). Раньше при
  пагинации WB (1 req/10с) пользователь видел статичный «Запрос списка документов…»
  без понимания, идёт ли процесс и сколько ждать. Теперь:
  - В трейт `Report::list` добавлен параметр `progress: ProgressCallbackRef`
    (раньше прогресс был только у `download`). Все impl/callers обновлены
    (Ozon, TestProvider, тесты используют `NoopProgress`).
  - `WbDocumentsReport::list` шлёт `ProgressUpdate` на каждой странице:
    «Получено 150 документов, всего: 500, страница 4» (со согласованием слов через
    `num_words`), с `current`/`total`/`fraction` (fraction известен при заданном
    `limit`, иначе None — индикатор «качается»).
  - GUI: `ListDocuments` arm создаёт `ProgressForwarder` и передаёт в `list_documents`,
    события `UiEvent::Progress` отображаются в статусбаре (уже было подключено).
- **Удалён `ozon.accrual_types` (Справочник типов начислений)** (сделано). Был в ТЗ
  §2.2.1 как Beta, но это служебный метод (справочник), а не выгрузка для пользователя —
  удалён из дескрипторов и фабрики Ozon. Теперь 17 отчётов Ozon (было 18). Тест
  `reports_count_is_17` проверяет число и отсутствие `accrual_types`.

### Не подтверждено первоисточником (может быть неверно)
1. `returns-api.wildberries.ru` для claims — спека говорит `/api/v1/claims`, но в присланной доке этого раздела нет
2. `seller-analytics-api.wildberries.ru` для acceptance_report — спека говорит POST async
3. Формат ответа claims — догадка

---

## 10. Git история (ключевые коммиты)

- ЭТАП 0-14: основной проект (14 коммитов)
- fix(gui): кнопки управления окном (ToolbarView)
- fix(profiles): динамическая форма из AuthField[]
- fix(download): provider_id из ReportInfo (не из префикса type_id)
- fix(reports): перерисовка списка + авто-загрузка
- feat: автосохранение состояния (ui_state SQLite)
- feat: календарь (MenuButton + Calendar)
- fix(wb): домен api.wildberries.ru → finance-api (NXDOMAIN)
- fix(wb): полная синхронизация с докой (POST для финансов, форматы ответов)
- fix(ozon): health-check date_from/date_to + async code→/v1/report/info→file
- fix(gui): показ путей к файлам + кнопка «Открыть папку»
- feat: combo категорий вместо текстового поля
- fix(gui): категории только для wb.documents (type_id, не display_name)
- **fix(gui): гонка LoadReports — сверка provider_id результата с текущим провайдером** (РЕШЕНО: категории WB теперь грузятся при выборе wb.documents)
- fix(gui): блокировка connect_changed при программной перестройке combo (REPORT_CHANGED_HANDLER)
- feat(gui): русские названия категорий (DocumentCategoryInfo{label,value}) — combo показывает title, в API уходит name
- fix(wb): схема WbDocument приведена к OpenAPI-спеке (name/extensions/creationTime; убраны выдуманные amount/date) — осмысленные имена в списке документов
- feat(wb): человекочитаемые имена файлов на диске (DocumentSel{name,extension} → doc_meta → source_id=name), реальное extension из ответа, поштучное скачивание, нормализация имён (unknown-сегменты вырезаются)
- feat(wb): пагинация /documents/list — цикл по offset (страницы по 50), limit=пусто→все, limit=N→потолок, страховочный cap 200 страниц

---

## 11. Как запускать

```bash
# Подготовить окружение
source scripts/env.sh

# Сборка
cargo build --workspace

# GUI
cargo run -p mdwf-gui

# CLI
cargo run -p mdwf-cli -- providers list
cargo run -p mdwf-cli -- reports list --provider wildberries
cargo run -p mdwf-cli -- doctor

# Тесты
cargo test --workspace
cargo clippy --workspace --tests -- -D warnings

# Release
cargo build --release -p mdwf-gui -p mdwf-cli
./scripts/build-release.sh
```

---

## 12. Важные уроки (что НЕ делать)

1. **НЕ додумывать API** — всегда сверять с официальной документацией
2. **НЕ использовать `api.wildberries.ru`** — он не существует
3. **НЕ создавать Popover вне дерева виджетов** — краш STATUS_ACCESS_VIOLATION
4. **НЕ сравнивать display_name с type_id** — разные значения
5. **НЕ добавлять отчёты вне спецификации** без явного запроса
6. **НЕ доверять `connect_changed` при программном `set_active`** — вызывать явно
7. **НЕ запрашивать категории при каждой смене профиля** — только при выборе wb.documents
8. **НЕ доверять асинхронным результатам без проверки актуальности** — гонка: устаревший
   ответ `LoadReports(A)` может прийти после того, как пользователь уже переключился на
   `B`, и затереть список. Всегда сверять `provider_id` результата с текущим выбором.
   (Именно этот баг ломал загрузку категорий WB.)
9. **Разделять отображаемое имя и техническое значение** — показывай `title` (русское),
   но в API передавай `name` (технический). Комбо хранит карту label→value.
10. **Сверять схему ответа с OpenAPI-спекой, а не угадывать поля.** `dev.wildberries.ru`
    закрыт антиботом — источник истины: зеркало `github.com/eslazarev/wildberries-sdk`
    (генерируется из спецификации WB). Из-за «угаданных» полей `amount`/`date` в
    `WbDocument` список документов показывал технический `serviceName` вместо `name`.
11. **При добавлении опционального плейсхолдера ({doc_id}) в дефолтный шаблон имени —
    нормализовать результат.** Иначе незаполненные сегменты дают двойные подчёркивания
    и пустоты (`report__2026-06.json`). В `naming.rs::normalize_name` unknown-сегменты
    вырезаются, повторы `_` схлопываются, `..` → `.`.
