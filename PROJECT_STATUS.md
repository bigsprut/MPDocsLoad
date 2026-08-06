# MDWF — Статус проекта и контекст для продолжения работы

> **Этот файл — для передачи контекста в новый диалог.**
> Прочитайте его целиком перед продолжением работы над MDWF.
> Обновлено: 2026-08-07.

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

4. ✅ **Значок «уже загружен» в списке документов (cross-session) + открыть/перекачать** — СДЕЛАНО.
   В списке документов WB — зелёный ✓ для уже скачанных (в любом сеансе) + tooltip с путём/датой.
   Действия per-row: «📂 Открыть» (открывает файл ассоциированным приложением) и «↻ Перекачать»
   (переотправляет Download с заменой). Реализация: миграция v3 — колонка `downloads.document_id`
   (serviceName) с idempotent ALTER + backfill из `params.values["ids"]` для старых строк;
   `Catalog::list_downloaded_docs(profile_id, report_type)`; проброс document_id в `persist_files`;
   `UiCommand::ListDownloads`/`UiEvent::DownloadsListed`; автообновление значков после Download.
   Feature WB-only (Ozon не имеет Browsable/DocumentEntry).

5. ✅ **Иконка типа файла в списке документов** — СДЕЛАНО.
   В `download.rs::render_list` название документа получает префикс-эмодзи по
   первому расширению: 📊 (xlsx/xls/csv), 📦 (zip/rar/7z/gz/tar), 📄 (pdf/xml/прочее).
   Регистронезависимо (`to_ascii_lowercase`). Header не трогается — эмодзи в той же
   ячейке названия. Хелпер `ext_emoji`.

6. ✅ **Офлайн-режим навигации по скачанным документам с фильтрами** — СДЕЛАНО.
   Новая вкладка «Архив» (sidebar, после «Загрузка»). Показывает ВСЕ скачанные файлы
   всех профилей/провайдеров из локального SQLite (без сетевых запросов/токенов).
   Фильтры (опциональные): профиль (combo «(все)»+список) / отчёт (combo из реальных
   report_type в БД) / период (combo Месяц+Год, `(все)`=без фильтра). Колонки:
   Профиль | Отчёт | Период | Формат | Размер | Скачан | Действия. Действия per-row:
   📂 Открыть файл, 📁 Открыть папку, 📋 Копировать путь (недеструктивно, без удаления).
   Реализация: DTO `ArchiveEntry` (JOIN downloads+profiles), Catalog-методы
   `list_downloads_filtered`/`distinct_report_types`; команды `ListArchive`/
   `LoadArchiveReportTypes` + события `ArchiveListed`/`ArchiveReportTypesLoaded`;
   новый view `archive.rs`; `open_file`/`open_folder` вынесены в `views/mod.rs`
   (`pub(crate)`, переиспользуются download+archive). При старте — авто-загрузка всех
   записей. Сортировка по `downloaded_at DESC`.
   **Фикс фильтра периода для WB-документов** (отдельным коммитом, после вопроса
   пользователя): изначально фильтр смотрел на `downloads.period` точным равенством,
   но у WB-документов period=NULL → они не попадали ни в какой месяц. Корень проблемы:
   дата документа WB (`creationTime` → `DocumentEntry.date`) **не сохранялась в БД
   никак** — терялась на этапе выбора (`DocumentSel` без даты) и в `DocMetaItem`.
   Решение: (1) новая колонка `downloads.document_date` (миграция v4, без backfill);
   (2) протаскивание даты через всю цепочку: `DocumentSel.date` → `DocMetaItem.date`
   (→ `params.values["doc_meta"]`) → `DocMeta.date` в WB-провайдере →
   `DownloadedFile.document_date` → `NewDownload.document_date` → БД;
   (3) фильтр периода = **пересечение диапазонов** (inclusion), НЕ точное совпадение:
   фильтр «Июль 2026» → диапазон `[01.07, 31.07]`, файл показывается если его интервал
   даты (period YYYY-MM→месяц целиком, YYYY-MM-DD→точка, иначе document_date) пере-
   секается с диапазоном (SQL `CASE WHEN`, `list_downloads_filtered(date_range)`);
   (4) заодно починен плейсхолдер `{doc_date}` в имени файла (`FileNameContext.
   document_date` раньше был жёстко `None` → всегда «nodate»; дефолтный шаблон
   `{doc_date}` не содержит → текущие имена не меняются). Колонка «Период» архива
   показывает `period.or(document_date)`. Старые WB-записи (без даты) из фильтра по
   периоду выпадают, но видны при «все» — backfill невозможен (нет источника даты).

**Дополнительно (отдельно от 6 пунктов):**
- ⏳ **Живые тесты API Ozon** с реальными токенами — НЕ ДЕЛАТЬ без явного добра пользователя.
  После того как пользователь перевыпустит ключи и создаст профили заново.

---

## 6. Что сделано в этом чате (коммиты, хронология)

Последние коммиты на `master` (свежие сверху):
```
151e25c feat: протаскивание даты документа WB до каталога + {doc_date} (П.6 фикс)
8dc2450 feat(storage): колонка document_date + фильтр периода как пересечение (П.6 фикс)
e693375 feat(gui): вкладка «Архив» офлайн-навигации + фильтры + действия (П.6)
33cd1af feat(storage): ArchiveEntry + list_downloads_filtered + distinct_report_types (П.6/1)
bf142af feat(gui): иконка типа файла (эмодзи) в списке документов (П.5)
646b90c feat(gui): значок «уже загружен» в списке документов + открыть/перекачать (П.4)
d4cafcb feat: типизированная обработка кодов возврата API (Ozon+WB, прод)
912d6c6 feat(ozon): все JSON-отчёты сохраняются как Excel (.xlsx) с русскими колонками
f0860df fix(gui): иконки на PNG напрямую через from_resource (были пустыми)
1189807 feat(ozon): расширение списка отчётов (8 → 21), сверенo с docs.ozon.ru
1022f78 fix(gui): иконки маркетплейсов через IconTheme (были пустыми листками)
32e73ea fix(ozon): имя продавца legal_name + удалить accrual_postings/by_day (баг 400)
95dfa43 feat(gui): раздел «Магазин» + иконка/имя продавца в заголовке
41e5b37 docs: обновить PROJECT_STATUS.md для нового чата
dd66056 fix: убрать clear_profiles() из старта (баг: профили пропадали между запусками)
20d9dbd docs: добавить справочник Ozon Seller API (копия docs.ozon.ru)
8744978 feat(security): секреты профилей только в OS keyring (везде)
```

**Ключевые изменения этого чата (П.5 + П.6):**

### П.5 — Иконка типа файла в списке документов WB
- В `download.rs::render_list` название документа получает **префикс-эмодзи** по
  первому расширению: 📊 (xlsx/xls/csv), 📦 (zip/rar/7z/gz/tar), 📄 (pdf/xml/прочее).
- Хелпер `ext_emoji(ext)` — `match` по `to_ascii_lowercase` (регистронезависимо, т.к.
  WB не гарантирует регистр расширений). Header не трогается — эмодзи в той же ячейке.

### П.6 — Вкладка «Архив» (офлайн-навигация по скачанным документам)
- **Scope**: показывает ВСЕ скачанные файлы всех профилей/провайдеров из локального
  SQLite. Сетевых запросов и токенов НЕ требует (offline). Профиль — опциональный фильтр.
- **Storage**: DTO `ArchiveEntry` (плоский, JOIN downloads+profiles → profile_name/provider_id;
  без file_hash) + методы `Catalog::list_downloads_filtered(profile_id, report_type, period)`
  (все строки, включая Period-отчёты без document_id; ORDER BY downloaded_at DESC;
  динамический WHERE по опциональным фильтрам) и `distinct_report_types()`. Тесты в catalog.rs.
- **Channels**: `ViewId::Archive`; `UiCommand::ListArchive { profile_name, report_type, period }`
  (все Option, None=«все») + `LoadArchiveReportTypes`; `UiEvent::ArchiveListed(Result<Vec<ArchiveEntry>>)`
  + `ArchiveReportTypesLoaded(Vec<String>)`.
- **App loop**: обработчики `ListArchive` (резолв profile_name→profile_id, вызов каталога) и
  `LoadArchiveReportTypes` (forward distinct_report_types). При старте — авто-загрузка всех записей.
- **View `archive.rs`** (по образцу download.rs): панель фильтров (Профиль / Отчёт / Месяц+Год),
  кнопка «🔍 Применить», ListBox с заголовком и строками (Профиль | Отчёт | Период | Формат |
  Размер | Скачан | Действия). Действия per-row: 📂 Открыть файл, 📁 Открыть папку, 📋 Копировать
  путь (через `gdk::Display::clipboard()`). Хуки on_profiles_loaded/on_report_types_loaded/on_archive_listed.
- **`views/mod.rs`**: `open_file`/`open_folder` вынесены из download.rs как `pub(crate)` —
  переиспользуются download+archive. download.rs оставил тонкие приватные обёртки.

### П.6 фикс — фильтр периода для WB-документов + плейсхолдер {doc_date}
- **Корень бага**: фильтр периода Архива смотрел на `downloads.period` точным
  равенством, но у WB-документов period=NULL. Причина: дата документа
  (`creationTime` → `DocumentEntry.date`) **не сохранялась в БД** — терялась на
  этапе выбора (`DocumentSel` без даты) и в `DocMetaItem` (→ `params.doc_meta`).
- **Решение**: новая колонка `downloads.document_date` (миграция v4, без backfill —
  данных нет). Дата протекает через всю цепочку: `DocumentSel.date` →
  `DocMetaItem.date` (→ `params.values["doc_meta"]`) → `DocMeta.date` в WB-провайдере →
  `DownloadedFile.document_date` → `NewDownload.document_date` → БД.
- **Фильтр = пересечение диапазонов** (inclusion), НЕ точное совпадение: фильтр
  «Июль 2026» → диапазон `[01.07, 31.07]` (хелпер `period_to_range`), файл показывается
  если его интервал даты (period YYYY-MM→месяц целиком, YYYY-MM-DD→точка, иначе
  document_date) пересекается с диапазоном. SQL: `CASE WHEN` + `date(substr(period,1,7)
  ||'-01','+1 month','-1 day')` для конца месяца. `list_downloads_filtered(date_range)`.
- **`{doc_date}` в имени файла** заработал: `FileNameContext.document_date` теперь
  берётся из `f.document_date` (раньше жёстко `None` → всегда «nodate»). Дефолтный
  шаблон `{doc_date}` не содержит → текущие имена не меняются.
- **Колонка «Период»** архива: `period.or(document_date)` — месяц запроса (Ozon) или
  дата документа (WB). Старые WB-записи (без даты) из фильтра по периоду выпадают,
  но видны при «все».

**Ключевые изменения ПРЕДЫДУЩЕГО чата (контекст сохранён ниже):**

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
- **Раздел «Магазин»** (первая вкладка sidebar): единый источник правды выбора маркетплейса+профиля
  + CRUD профилей (объединил бывшую вкладку «Профили»). Вкладки «Загрузка» и «Отчёты» больше не
  имеют собственных combos (read-only индикатор + ACTIVE_SHOP). Persist выбора — `ui_state`/`"active_shop"`.
- **Заголовок окна**: кастомный title-widget — иконка маркетплейса (PNG из gresource: Ozon #005bff /
  WB #cb11ab / test / placeholder) + имя продавца. Имя: Ozon `/v1/seller/info` → `company.legal_name`
  (trait-метод `account_display_name`, default `Ok(None)`; WB — default).
  gresource pipeline: `build.rs` + `glib-build-tools` + PNG (НЕ SVG — SVG-рендеринг на Windows/MinGW
  ненадёжен; PNG грузится напрямую через `Image::set_resource`).
- **Значок «уже загружен»** в списке документов WB (cross-session): зелёный ✓ + tooltip с путём/датой,
  per-row действия «📂 Открыть» (ассоциированным приложением) и «↻ Перекачать». Миграция v3
  (`downloads.document_id`), `list_downloaded_docs`, автообновление после Download. П.4 бэклога.

### Обработка ошибок API (прод, Ozon+WB)
- **`CoreError::Api { status, message, retryable }`** — типизированный вариант с человекочитаемым
  сообщением (НЕ сырой JSON). Helper-методы `is_auth_failure()`/`is_rate_limited()`/`is_transient()`.
- `parse_ozon_error` (gRPC-gateway `{code,message,details[]}`) и `parse_wb_error`
  (`{error,errorText}`/`{message,detail}`/`{data:{errors}}`). `map_status_error` → `CoreError::Api`.
- health_check (оба): типизированные matches — 5xx→Degraded, 401/403→auth, 429→rate limit.
- CLI exit codes: WB-401→`AuthError` (раньше ApiError), 429→`RateLimit`, 4xx→`UsageError`.
- 2xx JSON-parse failure → `Internal` (раньше неверно Network); WB 409 non-retryable;
  Ozon `download_file` проверяет статус; Ozon `Item-Retry-After` (минуты→секунды).

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
- 429: max 3 попытки + человекочитаемая ошибка (причина из body через `parse_wb_error`)
- 5xx: экспонента 500мс→cap 30с, 5 попыток
- 409 теперь non-retryable (логический конфликт, не transient)

### Ozon
- Rate limiter 50 RPS (20мс интервал)
- 429: чтение `Retry-After` + `X-Ratelimit-Retry` (секунды) + `Item-Retry-After`
  (минуты — product import, переводим в секунды) → backoff
- 429: max 3 попытки + причина из body через `parse_ozon_error`
- Circuit breaker: 5 ошибок → 5 минут

### Обработка кодов возврата API (прод, Ozon+WB)
Раньше все HTTP-ошибки (400/401/403/404/409/422/429/5xx) сваливались в
`CoreError::Internal(String)` с сырым текстом body. Теперь — типизированный
`CoreError::Api { status: u16, message: String, retryable: bool }`:
- `parse_ozon_error(status, body)` — парсит gRPC-gateway формат `{code, message, details[]}`,
  берёт `message`, дополняет `details`. Fallback — первые 500 симваков body.
- `parse_wb_error(status, body)` — парсит WB-форматы по порядку: `{error, errorText}`,
  `{message, detail}`, `{data:{errors:[...]}}`, `{"message":"..."}`. Fallback — 500 симваков.
- `map_status_error` (оба провайдера) → `CoreError::Api` с человекочитаемым message
  (НЕ сырой JSON).
- Helper-методы на CoreError: `is_auth_failure()` (401/403/SecretNotFound),
  `is_rate_limited()` (429), `is_transient()` (429/5xx/Network).
- health_check (оба провайдера): типизированные matches вместо хрупкого
  `msg.contains("401")`. 5xx → Degraded (transient), 401/403 → auth (явно),
  429 → rate limit (явно), 400/404 → Down.
- CLI exit codes: WB-401 теперь `AuthError` (раньше `ApiError`), 429 → `RateLimit`,
  400/409/422 → `UsageError`, 404 → `NotFound`, 5xx → `ApiError`.
- 2xx с некорректным JSON → `Internal` (раньше неверно `Network`).
- Ozon `download_file`: проверка статуса, на 4xx/5xx → `CoreError::Api`.

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
17. **Обработка кодов возврата API (прод)**: HTTP-ошибки — типизированный `CoreError::Api
    {status, message, retryable}` с распарсенным message (не сырой JSON-блоб). Парсеры
    `parse_ozon_error`/`parse_wb_error` извлекают человекочитаемое описание из тела ответа
    (Ozon gRPC-gateway `{code,message,details}`, WB — несколько форматов). health_check
    использует helper-методы `is_auth_failure()`/`is_rate_limited()`/`is_transient()`, НЕ
    хрупкий `msg.contains("401")`. CLI exit codes теперь корректно маппят auth/rate-limit.
18. **2xx — не всегда успех**: JSON-parse failure на 2xx — это `Internal` (ошибка протокола),
    НЕ `Network`. А `download_file` по ссылке из report/info тоже должен проверять статус
    (404/403 на ссылке → `Api`, не молчаливый `Network`).
19. **Проверять, что данные реально сохраняются в БД, прежде чем фильтровать по ним.**
    Фильтр периода Архива изначально смотрел на `downloads.period` точным равенством,
    но у WB-документов period=NULL → фильтр не работал. Корень: дата документа
    (`creationTime` → `DocumentEntry.date`) **терялась** до записи (DocumentSel и
    DocMetaItem без даты). Урок: если фильтр/отображение опирается на поле — убедись,
    что это поле **пишется** при сохранении, а не только существует в схеме. Дата
    должна протекать через всю цепочку (UI-выбор → params → провайдер → DownloadedFile
    → NewDownload → БД), иначе теряется на любом звене.
20. **Фильтр по периоду = пересечение диапазонов (inclusion), НЕ точное равенство.**
    Точное `period = 'YYYY-MM'` ломается, когда источником даты бывают разные поля
    (период запроса vs дата документа) и разные форматы (YYYY-MM vs YYYY-MM-DD).
    Правильно: фильтр задаёт диапазон `[from, to]`, файл показывается если его
    «интервал даты» (period YYYY-MM→месяц целиком, YYYY-MM-DD→точка, иначе
    document_date) пересекается с диапазоном фильтра (`file_start<=filter_to AND
    file_end>=filter_from`). SQLite: `date(substr(period,1,7)||'-01','+1 month','-1 day')`
    даёт последний день месяца (внимание: `substr(period,1,7)` — первые 7 символов
    для YYYY-MM, НЕ 8; и конкатенация с дефисом `'-01'`, не `'01'`).
21. **Плейсхолдеры шаблона имени файла** (`{doc_date}`, `{period}`, …) работают только
    если соответствующее поле `FileNameContext` реально заполнено. Жёсткий `None` →
    плейсхолдер всегда даёт fallback-значение («nodate»). Проверяй, что данные доходят
    до `FileNameContext`, а не только до каталога. Дефолтный шаблон изменений не
    вызывает — только если пользователь сам включил плейсхолдер в настройках.

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
cargo test --workspace          # ~167 тестов (storage: 18, incl. 2 новых для Архива)
cargo clippy --workspace --tests -- -D warnings

# Release
cargo build --release -p mdwf-gui -p mdwf-cli
./scripts/build-release.sh

# git: после каждого изменения — коммит + push на origin/master
```

---

## 14. С чего начать в новом чате

**В этом чате завершены**: П.5 (иконка типа файла, эмодзи в списке документов WB),
П.6 (вкладка «Архив» — офлайн-навигация по скачанным документам с фильтрами профиль/
отчёт/период + действия открыть файл/папку/копировать путь), и **фикс П.6** — фильтр
периода Архива для WB-документов (раньше не работал: дата документа не сохранялась в
БД) + плейсхолдер `{doc_date}` в имени файла. Все 6 пунктов бэклога закрыты.
Все изменения запушены (последний коммит `151e25c`).

**Структура вкладок GUI (sidebar):** Магазин → Отчёты → Загрузка → **Архив** (новое) →
Настройки → Планировщик → Журнал → О программе.

**Что можно проверить вживую (GUI):**
1. Вкладка **«Архив»**: при старте показывает все скачанные файлы (если они есть в БД).
   Фильтры: профиль `(все)` + список, отчёт из реальных report_type, период Месяц+Год.
   **Фильтр периода теперь работает и для WB-документов** (по дате документа) — но
   только для файлов, скачанных ПОСЛЕ этого фикса (старые записи без даты в фильтр по
   периоду не попадают, видны при «все»). Кнопки 📂/📁/📋 в строках. Архив НЕ требует
   токенов/сети — только локальный SQLite.
2. Вкладка **«Загрузка»**: в списке документов WB — эмодзи-префикс названия (📊/📦/📄).
3. После скачивания нового WB-документа — проверить в «Архиве», что он попадает в
   фильтр по месяцу своей даты (creationTime).

**⚠️ ВАЖНО перед продолжением** — пользователь должен:
1. **Закрыть запущенный GUI** (exe блокирует линковку при сборке на Windows).
2. **Перевыпустить ключи Ozon/WB** (старые были в открытом виде в БД до коммита 8744978).
3. **Создать профили заново** в разделе **«Магазин»** → секреты уйдут в keyring.
4. **Выбрать магазин** → проверить: заголовок (иконка + `legal_name` для Ozon), список
   отчётов, скачивание (Excel-отчёты, значок «уже загружен», иконка типа файла в списке).
5. **Подтвердить, что скачивание работает** → тогда можно живые тесты API (без явного
   разрешения пользователя НЕ запускать).

**Живые тесты API с реальными токенами — НЕ делать без явного добра пользователя.**

**Возможные дальнейшие улучшения (не из исходного бэклога, на усмотрение пользователя):**
- Persist фильтров вкладки «Архив» в `ui_state` (сейчас фильтры не сохраняются между сеансами).
- CLI-аналог «Архива» (`mdwf-cli archive list --provider/--report/--period`).
- Действие «Удалить запись+файл» в Архиве (сейчас намеренно недеструктивно — нужна подтверждение).
- Индекс `downloads` по `(profile_id, report_type, period)` для больших архивов.
