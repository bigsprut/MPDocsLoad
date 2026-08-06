# MDWF — Статус проекта и контекст для продолжения работы

> **Этот файл — для передачи контекста в новый диалог.**
> Прочитайте его целиком перед продолжением работы над MDWF.
> Обновлено: 2026-08-06.

---

## 1. Что это за проект

Marketplace Downloader Framework (MDWF) v1.4 — desktop-приложение на Rust
для выгрузки финансовых документов с маркетплейсов **Ozon** и **Wildberries**
через их официальные API. GTK4 + libadwaita GUI, CLI, cron-планировщик, REST API.

**Спецификация:** `MarketplaceDownloaderFramework_TechnicalDoc_v1.4_2026-07-10.md`
в корне проекта.
**Рабочая директория:** `D:\work\Learn\ZCode\MPDocsLoad`
**GitHub (Private):** `https://github.com/bigsprut/MPDocsLoad` (ветка `master`).

---

## 2. Среда разработки

- **Rust:** stable-x86_64-pc-windows-gnu (GTK-библиотеки MinGW-сборки из MSYS2)
- **GTK4 4.20.3 + libadwaita 1.8.3** в `D:\msys64\mingw64`
- **Запуск GUI:** `source scripts/env.sh && cargo run -p mdwf-gui`
- **CLI:** `cargo run -p mdwf-cli -- providers list` (и reports/download/doctor/schedule)
- **GitHub CLI (`gh`) установлен** в `C:\Program Files\GitHub CLI`, авторизован под `bigsprut`.
- **Режим работы с git:** после каждого изменения — коммит + `git push` на origin/master.

---

## 3. Структура проекта (12 крейтов)

```
crates/
├── core/              — трейты, типы, реестр (провайдер-агностик)
├── storage/           — SQLite (Catalog) + FileStore + дедуп SHA-256
├── secrets/           — OS keychain (keyring) + in-memory mock + account.rs (хелперы)
├── scheduler/         — cron + retry + автозапуск Windows (HKCU Run)
├── config/            — config.toml + пути (%APPDATA%\mdwf)
├── test-provider/     — TestProvider mock
├── providers/ozon/    — Ozon Seller API (21 отчёт)
├── providers/wildberries/ — WB OpenAPI (14 отчётов)
├── cli/               — mdwf CLI (clap)
├── gui/               — mdwf-gui (GTK4 + libadwaita)
└── api/               — REST API (feature 'server', axum)
```

**Данные приложения (общие для GUI и CLI):**
- Конфиг/БД: `C:\Users\MAN-MADE\AppData\Roaming\mdwf\` (config.toml, mdwf.db)
- Файлы: `C:\Users\MAN-MADE\Documents\MDWF\downloads\{provider}\{period}\` (в config.toml — `D:\work\Learn\ZCode\MPDocsLoad\MDWF\downloads`)

---

## 4. ⚠️ КРИТИЧНО: секреты и безопасность

**Модель хранения секретов (сделано в этом чате, коммит 8744978):**
- `auth_metadata` в SQLite хранит **только несекретные** поля (`AuthField.secret==false`), напр. `client_id`.
- Секреты (`AuthField.secret==true`: ozon `api_key`, wb `token`) — **только в OS keyring** (Windows Credential Manager через крейт `keyring`, service `dev.mdwf.MDWF`).
- Keyring-ключ детерминированный: `mdwf:{provider_id}:{profile_name}:{field_id}` (хелпер `crates/secrets/src/account.rs::account_key`).
- Провайдеры **не меняются**: `authenticator` читает секрет из `auth_metadata`; app/CLI/API-слой подмешивает секрет in-memory перед вызовом (`load_profile_secrets`).

**CLI и GUI полностью консистентны** (единый config.toml + mdwf.db + keyring с одинаковыми ключами). Изменение в GUI = изменение для CLI (при следующем запуске).

**⚠️ ВНИМАНИЕ — перевыпуск ключей:**
В начале этого чата аудитом обнаружено, что реальные токены Ozon/WB лежали
открытым текстом в `mdwf.db`. Файл зачищен (VACUUM), и впредь секреты хранятся
только в keyring. **НО старые значения какое-то время были в открытом виде в
файле — пользователь должен перевыпустить оба ключа** в личных кабинетах
(Ozon: Настройки → Seller API; WB: Профиль → Доступ к API). **Профили сброшены,
пользователь создаст их заново** (секреты уйдут в keyring).

**Старые профили НЕ мигрируются** — миграции нет. После удаления `clear_profiles()`
из старта (коммит dd66056) профили переживают перезапуск.

---

## 5. Текущий бэклог (6 пунктов, от пользователя)

Пользователь поставил 6 задач. Статус:

1. ✅ **Удалить deprecated/Premium отчёты Ozon** — СДЕЛАНО + расширено до **21 отчёта**.
   Убраны `transaction_list`/`transaction_totals` (deprecated → отключены 8 сентября 2026),
   `realization_by_day` (Premium), `stock_on_warehouses` (deprecated → заменён на
   `/v1/analytics/stocks`). **accrual_postings/by_day ВОЗВРАЩЕНЫ** (реализованы по доке:
   posting_numbers / date+last_id — через новый `OzonPaginatedReport`, не browsable).
   Теперь **21 отчёт Ozon**: 8 существующих + 8 seller-отчётов (create→code→file) +
   3 inline-списка (cash_flow/stocks/turnover) + 2 accrual. Все Period, сверенo с docs.ozon.ru.

2. ✅ **Перенос выбора маркетплейс+профиля в раздел «Магазин» + иконка/имя продавца в заголовке** — СДЕЛАНО.
   Новый раздел «Магазин» (первая вкладка): выбор маркетплейса+профиля + CRUD профилей
   (объединил бывшую вкладку «Профили»). Выбор — единый источник правды: вкладки «Загрузка»
   и «Отчёты» больше не имеют собственных combos магазина (read-only индикатор + активный
   провайдер из `ACTIVE_SHOP`). Persist выбора — в `ui_state`/`"active_shop"` (SQLite).
   Заголовок окна: иконка маркетплейса (gresource SVG: Ozon #005bff / WB #cb11ab / test /
   placeholder) + имя продавца. Имя продавца: Ozon `POST /v1/seller/info` → `company.legal_name`
   (полное юр. наименование, точнее краткого `name`; новый trait-метод `account_display_name`
   с default `Ok(None)`; WB — default, нет эндпоинта).
   gresource pipeline введён: `build.rs` + `glib-build-tools` + `resources.gresource.xml`.
   Вкладка «Профили» удалена (код перенесён в `shop.rs`).

3. ✅ **Проверить сохранение токенов (шифрование)** — СДЕЛАНО (коммиты 8744978, dd66056).
   Секреты только в keyring ВЕЗДЕ (GUI+CLI+API). См. секцию 4.

4. ⏳ **Значок «уже загружен» в списке документов (cross-session) + открыть/перекачать** — НЕ НАЧАТО.
   В списке документов — значок статуса, что документ уже был скачан (не только в этом сеансе,
   а и в любом предыдущем). Возможность открыть и повторно скачать с заменой.

5. ⏳ **Иконка типа файла в списке документов** — НЕ НАЧАТО. Небольшой объём.

6. ⏳ **Офлайн-режим навигации по скачанным документам с фильтрами** — НЕ НАЧАТО.
   Крупная новая функциональность.

**Дополнительно (отдельно от 6 пунктов):**
- ⏳ **Живые тесты API Ozon** с реальными токенами — НЕ ДЕЛАТЬ без явного добра пользователя.
  После того как пользователь перевыпустит ключи и создаст профили заново.

---

## 6. Что сделано в этом чате (коммиты, хронология)

Последние коммиты на `master` (свежие сверху):
```
dd66056 fix: убрать clear_profiles() из старта (баг: профили пропадали между запусками)
20d9dbd docs: добавить справочник Ozon Seller API (копия docs.ozon.ru)
8744978 feat(security): секреты профилей только в OS keyring (везде)
de2b2d5 refactor(ozon): удалить deprecated и Premium-отчёты
fc727b3 refactor(ozon): удалить сложные отчёты cash_flow/analytics/act_discrepancy
e4e7534 fix(ozon): тела запросов приведены в соответствие с API v2.1
4f64847 feat(gui): выбор месяца двумя combo + автообновление диапазона
93c4c63 refactor(ozon): удалить ozon.b2b_sales_json
09d96bb refactor(ozon): удалить ozon.accrual_types
01dc171 fix(wb): диагностика имени файла + регрессионный тест
0ccb096 feat: живой прогресс при загрузке списка документов
67c927d fix(wb): имя файла на диске из fileName ответа /download
c82d6c2 feat(wb): пагинация /documents/list
8440c57 feat(wb): человекочитаемые имена категорий, документов и файлов
```

**Ключевые изменения этого чата:**

### WB (Wildberries)
- **Человекочитаемые имена**: категории (label/value), документы (поле `name` из `/list`),
  файлы на диске (`fileName` из ответа `/download`, через `strip_extension`).
  Схема `WbDocument` приведена к официальной OpenAPI-спеке (зеркало `eslazarev/wildberries-sdk`).
- **Пагинация** `/documents/list` (страницы по 50, `limit`=потолок, `None`=все).
- **Живой прогресс** при загрузке списка: в трейт `Report::list` добавлен `ProgressCallbackRef`,
  `WbDocumentsReport::list` шлёт `ProgressUpdate` на каждой странице.
- **Имя файла на диске** = `fileName` ответа `/download` (с отрезанным расширением), fallback `name`→id.

### Ozon
- **Тела запросов приведены к API v2.1** (`build_download_body(type_id, params)`):
  realization/posting → `month`+`year` integer; balance/buyout → `date_from`/`date_to`;
  async (compensation/decompensation/b2b_sales/mutual_settlement) → `date` YYYY-MM.
  Сверено с **первоисточником docs.ozon.ru** (копия в `docs/ozon-seller-api-reference.md`).
- **Удалены отчёты**: accrual_types (служебный), b2b_sales_json (дублёр),
  cash_flow/analytics/act_discrepancy (требуют сложных схем/UI),
  transaction_list/totals (deprecated), realization_by_day (Premium).
  Теперь **21 отчёт Ozon** (было 18 → 10 → 8 → 21). Полный список эндпоинтов —
  в `crates/providers/ozon/src/reports.rs::all_report_descriptors` и `make_report`.
  Группы: финансовые (realization*, balance, buyout, cash_flow, accrual*),
  штрафы (compensation, decompensation), реестры (b2b_sales, mutual_settlement),
  seller-отчёты (products, returns, postings, discounted, warehouse_stock,
  placement_by_products/supplies, marked_products_sales), аналитика (analytics_stocks,
  analytics_turnover).
- **Все отчёты Ozon сохраняются как Excel (.xlsx)** (раньше часть была JSON):
  - `realization_posting` → серверный Excel от Ozon через async
    `/v1/report/realization/posting/create` (готовый xlsx, как в личном кабинете).
  - buyout, balance, realization, cash_flow, analytics_stocks/turnover, accrual_postings/by_day
    → конвертация JSON в .xlsx через `rust_xlsxwriter` (модуль `xlsx.rs`) с **русскими
    заголовками колонок** из docs.ozon.ru. balance — 3 листа (Доходы/расходы, Услуги, Итоги);
    accrual_by_day — 2 листа (Начисления, Сборы); accrual_postings — денормализация.

### GUI
- **Выбор месяца двумя combo** (Январь…Декабрь + год) вместо текстового поля YYYY-MM.
  При смене месяца — автообновление диапазона (1-е число .. сегодня/конец месяца).

### Безопасность
- **Секреты только в OS keyring** (см. секцию 4).

### Документация
- `docs/ozon-seller-api-reference.md` — копия docs.ozon.ru (для сверки, контент Ozon).

---

## 7. API WB — сверен с официальной документацией

**Источник:** dev.wildberries.ru (разделы: Баланс, Финансы, Документы, Отчёты, Аналитика).
Схема WB также сверена через зеркало `github.com/eslazarev/wildberries-sdk`
(генерируется из OpenAPI-спеки WB; сам dev.wildberries.ru за антиботом).

### Домены (ВСЕ подтверждены nslookup + докой)
| Домен | Назначение |
|-------|-----------|
| `finance-api.wildberries.ru` | Баланс, отчёты реализации, эквайринг |
| `documents-api.wildberries.ru` | Категории, список, скачивание документов |
| `statistics-api.wildberries.ru` | Заказы, продажи |
| `seller-analytics-api.wildberries.ru` | Штрафы, антифрод, возвраты, приёмка |
| `returns-api.wildberries.ru` | Возвраты (claims) — по спеке, не проверено в доках |

**api.wildberries.ru НЕ существует (NXDOMAIN) — не использовать!**

### Documents API WB (детали)
- **categories:** GET /api/v1/documents/categories → `{data:{categories:[{name,title}]}}`
- **list:** GET /api/v1/documents/list, параметры: `locale, beginTime, endTime, sort, order, category, serviceName, limit(≤50), offset` → `{data:{documents:[...]}}`. Поля документа (схема GetListDataDocumentsInner): `serviceName, name, category, extensions, creationTime, viewed`.
- **download:** GET /api/v1/documents/download, параметры: `serviceName, extension` → `{data:{fileName, extension, document(base64)}}`.
- **Rate limits documents:** 1 req/10s burst 5.

### Авторизация WB
- Заголовок `Authorization: <token>` (БЕЗ префикса "Bearer ").

---

## 8. API Ozon — сверен с первоисточником docs.ozon.ru

**Источник:** `docs/ozon.ru/api/seller/` (копия в `docs/ozon-seller-api-reference.md`).
Сайт за антибот-челленджем — прямого автоматического доступа нет.

### Авторизация Ozon
- Заголовки `Client-Id` + `Api-Key`. TTL ключа: 6 месяцев.

### Тела запросов (сверены с docs.ozon.ru, исправлено в этом чате)
- `/v2/finance/realization`, `/v1/finance/realization/posting` → `{month, year}` **integer**.
- `/v1/finance/realization/by-day` → `{day, month, year}` integer (УДАЛЁН — Premium Plus/Pro).
- `/v1/finance/balance`, `/v1/finance/products/buyout` → `{date_from, date_to}` YYYY-MM-DD.
- `/v1/finance/compensation`, `/decompensation`, `/document-b2b-sales`, `/mutual-settlement` → `{date}` YYYY-MM (async: code → /v1/report/info → file).

---

## 9. Анти-бан меры

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

## 10. GUI — ключевые архитектурные решения

### Связь GTK ↔ tokio
- `async_channel` (НЕ glib::MainContext::channel — его нет в glib 0.20)
- UI→tokio: `mpsc::UnboundedSender<UiCommand>` (CommandSender)
- tokio→UI: `async_channel::Sender<UiEvent>` (EventForwarder)

### Thread-local паттерн для виджетов
```rust
thread_local! {
    static W_PROVIDER: Rc<RefCell<Option<ComboBoxText>>> = ...
    static W_CATEGORY_COMBO: Rc<RefCell<Option<ComboBoxText>>> = ...
    static W_MONTH_COMBO / W_YEAR_COMBO: ... // выбор месяца двумя combo
    static CMD: Rc<RefCell<Option<CommandSender>>> = ...
    static CATEGORIES: Rc<RefCell<Vec<(String,String)>>> = ... // label→value
    static CHECKS: Rc<RefCell<Vec<(DocumentSel, CheckButton)>>> = ...
}
```

### Типы команд/событий (channels.rs)
- UiCommand: LoadProviders, LoadProfiles, LoadAuthFields, LoadReports, LoadDocumentCategories, SaveProfile, DeleteProfile, CheckProfile, ListDocuments, Download { documents: Vec<DocumentSel>, .. }, Cancel, SaveDownloadState, LoadDownloadState
- DocumentSel { id, name, extension } — выбранный документ (name для имени файла).
- UiEvent: ProvidersLoaded, ProfilesLoaded, AuthFieldsLoaded, ReportsLoaded, DocumentCategoriesLoaded, DocumentsListed, DownloadFinished, Progress, Notify, DownloadStateLoaded.

---

## 11. Известные проблемы и TODO

### Не реализовано
1. **Async-отчёты WB** (acceptance_report) — create→poll→download паттерн не реализован, только create-фаза
2. **max_parallel_jobs** планировщика — счётчик running не декрементируется
3. **TestProvider в release** — должен быть только dev
4. **Очистка rate-limiter от ожидания 60с в тестах** — `MDWF_WB_NO_RATELIMIT=1`
5. **Автосохранение может восстанавливать test.documents** при провайдере WB — stale state (есть частичная защита от гонки).

### Не подтверждено первоисточником (может быть неверно)
1. `returns-api.wildberries.ru` для claims — спека говорит `/api/v1/claims`, но в доке этого раздела нет
2. Формат ответа claims — догадка

---

## 12. Важные уроки (что НЕ делать)

1. **НЕ додумывать API** — всегда сверять с официальной документацией. Ozon — через
   `docs/ozon-seller-api-reference.md` (копия docs.ozon.ru, т.к. сайт за антиботом).
   WB — через зеркало `eslazarev/wildberries-sdk` (генерируется из OpenAPI-спеки).
2. **НЕ использовать `api.wildberries.ru`** — он не существует.
3. **НЕ создавать Popover вне дерева виджетов** — краш STATUS_ACCESS_VIOLATION.
4. **НЕ сравнивать display_name с type_id** — разные значения.
5. **НЕ доверять `connect_changed` при программном `set_active`** — вызывать явно.
6. **НЕ запрашивать категории при каждой смене профиля** — только при выборе wb.documents.
7. **НЕ доверять асинхронным результатам без проверки актуальности** — гонка.
8. **Разделять отображаемое имя и техническое значение** — combo хранит карту label→value.
9. **Схема ответа API — сверять с OpenAPI-спекой, а не угадывать поля.**
10. **Секреты — только в OS keyring.** В SQLite — только несекретные поля (client_id).
    Account-ключ: `mdwf:{provider}:{name}:{field}`.
11. **При добавлении опционального плейсхолдера в шаблон имени файла — нормализовать результат**
    (`naming.rs::normalize_name` вырезает unknown-сегменты).
12. **CLI и GUI должны быть консистентны**: единый config.toml + mdwf.db + keyring.
13. **НЕ вызывать `clear_profiles()` на каждом старте** — профили должны переживать перезапуск.
14. **gresource pipeline**: `glib-build-tools::compile_resources(&["dir"], "manifest.xml", "compiled.gresource")`
    генерирует **бинарный** `.gresource` (НЕ `.rs`!). Регистрация через
    `gio::resources_register_include!("compiled.gresource")` (НЕ `glib::`, НЕ `resources.rs`).
    Путь иконки: `resource:///префикс/имя.svg`, виджет — `Image::set_resource(Some(path))`.
    В glib 0.20 `icon_name()` принимает `impl Into<GString>` (без `Option`).
15. **Имя продавца для заголовка**: trait-метод `account_display_name(auth) -> Option<String>`
    с **default** `Ok(None)` (WB/test наследуют, не ломая trait). Ozon: `POST /v1/seller/info`
    → `company.legal_name` (полное юр. наименование, точнее краткого `name`; без обёртки `result`,
    тело пустое). Ошибка fetch НЕ блокирует смену магазина (seller_name=None, заголовок fallback на имя профиля).
16. **Единый источник правды выбора магазина**: persist в `ui_state`/`"active_shop"` (SQLite),
    не в config.toml. Все вкладки читают оттуда, а не из собственных combos.

---

## 13. Как запускать

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
cargo test --workspace          # 126 тестов
cargo clippy --workspace --tests -- -D warnings

# Release
cargo build --release -p mdwf-gui -p mdwf-cli
./scripts/build-release.sh

# git: после каждого изменения — коммит + push на origin/master
```

---

## 14. С чего начать в новом чате

**П.2 завершён** (этот чат). Следующая задача — **П.4** (по бэклогу секции 5):
> Значок «уже загружен» в списке документов (cross-session) + открыть/перекачать.
> В списке документов — значок статуса, что документ уже был скачан (не только в этом
> сеансе, а и в любом предыдущем). Возможность открыть и повторно скачать с заменой.

Данные для статуса уже есть в каталоге: таблица `downloads` (FileStore + дедуп SHA-256,
см. `crates/storage/src/catalog.rs::record_download`). Нужно сопоставлять `DocumentEntry`
из списка с уже записанными `downloads` (по document_id / имени файла) и рисовать значок.

**Перед следующими задачами** желательно, чтобы пользователь:
1. Закрыл запущенный GUI (exe блокирует линковку при сборке).
2. Перевыпустил ключи Ozon/WB (старые были в открытом виде в БД).
3. Создал профили заново в новом разделе **«Магазин»** (заменил «Профили») → секреты уйдут в keyring.
4. Выбрал магазин в разделе «Магазин» → проверить, что заголовок показывает иконку + имя продавца.
5. Подтвердил, что скачивание работает (тогда можно живые тесты API).
