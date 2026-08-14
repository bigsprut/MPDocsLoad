# MDWF — Статус проекта и контекст для продолжения работы

> **Этот файл — для передачи контекста в новый диалог.**
> Прочитайте его целиком перед продолжением работы над MDWF.
> Обновлено: 2026-08-14.

## 0. Снимок состояния (для быстрого старта)

- **HEAD:** `06b04cc` (master = origin/master, рабочее дерево чистое; наработки
  2026-08-14 запушены: UX-пакет, P0-P2 фиксы, symbolic-иконки, REPORT_DEFS 4→1,
  sidebar, **живой WB-аудит**).
- **Зелёное состояние:** `cargo test --workspace` exit=0 (23 сюита);
  clippy `-D warnings`=0 по всем крейтам.
- **Сделано этим чатом (2026-08-14, вечер):**
  1. **Инсталлер пересобран** после UX-пакета (`MDWFSetup-1.4.0.exe`, 16:05);
     ПЕРЕСОБРАН СНОВА после WB-фиксов (см. ниже) — выдача пользователю актуальна.
  2. **Живой WB-аудит ЗАВЕРШЁН** (главный фронт §0 закрыт): сверка всех
     эндпоинтов с официальной OpenAPI-спекой (зеркало eslazarev/wildberries-sdk,
     specs/*.yaml) + baseline-прогон + фиксы + повторный живой прогон —
     **12/12 отчётов OK**. Ключевое: detailed-финансы молча обрезались на
     1000 строк (теперь rrdId-курсор: 3217 строк за июль); claims без
     обязательного is_archive (400); acceptance_report переписан на
     GET create→poll→download. Коммит `06b04cc`.
- **Ближайшие задачи (в порядке приоритета):**
  1. Опционально/отложено: WB-отчёты сохраняются как JSON (Ozon конвертирует
     в xlsx с русскими колонками) — потенциальный UX-хвост.
  2. Отложено: типизация ошибки breaker-open (сейчас Internal); WB
     RateLimiter.backoff без max (низкий импакт); blocking-SQLite-in-async.
- **Живые API-тесты разрешены пользователем** (токены перевыпущены, профили
  `oz_prof1` + `wb_prof` созданы, ключи валидны).
- Ключевые уроки этого дня: **#53-58** (cron-локаль, cargo test ≠ build,
  CssProvider мёртв в GTK 4.20, нода ≠ css-класс, гниющие координаты →
  re-OCR, тихие потери данных WB без сверки со спекой).

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
- Файлы: `C:\Users\MAN-MADE\Documents\MDWF\downloads\{provider}\{год}\` — подпапка **год** периода («2026-07»/«2026-07-15» → «2026»; file_store.rs::year_folder). Полный период остаётся в имени файла (`{period}` в шаблоне). Старые файлы в месячных папках НЕ переносятся — пути в БД указывают туда и продолжают работать. (в config.toml — `D:\work\Learn\ZCode\MPDocsLoad\MDWF\downloads`)

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
   В `download.rs::render_list` и `archive.rs::render_archive` рядом с названием —
   настоящая графическая иконка типа файла (PNG из gresource). Источник: **vscode-icons**
   (MIT, github.com/vscode-icons/vscode-icons) — официальные фирменные иконки:
   Excel (зелёный с X), PDF (Adobe-стиль), JSON, XML, ZIP, TXT, generic.
   SVG-исходники + PNG 48×48 (через `rsvg-convert`). Хелпер `ext_icon_resource(ext)`
   → путь в gresource `/org/mdwf/icons/file-*.png`. Image+Label в одном GtkBox
   (не сбивает выравнивание колонок). Эмодзи-вариант был промежуточным — заменён.

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
- ✅ **Живые тесты API Ozon** — пользователь разрешил; токены перевыпущены,
  профиль `oz_prof1` создан. Живой клик-тест GUI тоже выполнялся (методология
  урока #51). **Живой WB-аудит ВЫПОЛНЕН** (см. снимок состояния §0 и секцию 14).

---

## 6. Что сделано в этом чате (коммиты, хронология)

Последние коммиты на `master` (свежие сверху):
```
06b04cc fix(wb): живой аудит API — пагинация финансов, claims, acceptance poll→download
7b71524 docs: PROJECT_STATUS — снимок состояния §0 для хэндоффа (HEAD 9ee0e62, бэклог, уроки #53-57)
9ee0e62 fix(gui): активный пункт sidebar — жирный+accent (CssProvider в GTK 4.20 не применяется)
cc4ebfc fix(gui): явное выделение активного пункта sidebar
61a3c22 docs: PROJECT_STATUS — symbolic-иконки + унификация таблицы отчётов
2fb645f feat(gui): миграция эмодзи-кнопок на Adwaita symbolic-иконки
96b185c feat(ozon): авто-fill ID складов для warehouse_stock (/v2/warehouse/list)
c64e9ae feat(cli): флаги --posting-numbers/--warehouse-ids/--skus (остаток A)
864e2e6 fix(ozon): A/B/C — период→даты, поллинг report/info, accrual_by_day по дням
df0ad6b feat(gui): официальные иконки типов файлов (vscode-icons, MIT)
9a3d043 feat(gui): стандартные иконки Adwaita (промежуточный)
426590c feat(gui): PNG-иконки типов файлов (эмодзи заменены на графику)
4fcf449 docs: PROJECT_STATUS — 4 доп. фичи (persist/CLI/удаление/индекс)
a3b571c feat: удаление записи+файла в Архиве (delete_download + MessageDialog)
574e1ee feat(gui): persist фильтров Архива между сеансами (ui_state[archive_screen])
8010b76 feat: индекс idx_downloads_filter + CLI archive list
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

### Аудит Ozon API (живой прогон, фикс A/B/C + marked + warehouse)
Сверка кода с первоисточником `docs.ozon.ru` (локальная копия `docs/ozon-seller-api-reference.md`,
свежая от 6 авг 2026; прямой доступ к сайту блокируется антиботом). Живой прогон всех
21 отчётов (oz_prof1, 2026-07) выявил 19 FAIL из 21. Три корневые причины:

- **A — даты не подставлялись из `--period`**: `build_download_body` читал
  `values["date_from"]/["date_to"]` (заполняет только GUI), а CLI/расписание передавали
  только `period` → тело без дат → 4xx «invalid DateFrom/Filter». Фикс: fallback
  `period YYYY-MM → (первый..последний день месяца)` (хелпер `period_to_date_range`).
- **B — нет поллинга `/v1/report/info`**: один запрос, при `waiting`/`processing`
  сразу ошибка. Фикс: цикл `sleep(5с)→повтор` до success/failed, таймаут ~10 мин
  (120 попыток).
- **C — `accrual_by_day` шлёт `date: YYYY-MM`**: дока требует `YYYY-MM-DD` (один день),
  `last_id` пагинирует день. Фикс: перебор ВСЕХ дней месяца (хелпер `month_days`),
  по каждому — `last_id` пагинация.
- **+ `marked_products_sales`**: сервер требует date-only YYYY-MM-DD (10 символов),
  код шлёт ISO datetime (24). Фикс: прямой date_from/date_to.
- **+ `warehouse_stock`**: авто-fill ID складов через `/v2/warehouse/list` (fetch_warehouse_ids)
  если `--warehouse-ids` не передан; если FBS-складов нет — понятная ошибка.

**Результат живого прогона после фиксов (18 OK из 21):**
- ✅ OK (18): realization, realization_posting, buyout, balance, cash_flow,
  accrual_by_day, compensation, decompensation, mutual_settlement, products,
  discounted, placement_by_products, placement_by_supplies, marked_products_sales,
  analytics_turnover, analytics_stocks (auto-fill SKU), **returns** (полный /v1/returns/list),
  **accrual_postings** (auto-fill posting_numbers).
- ⚠️ warehouse_stock — `/v2/warehouse/list` вернул `warehouses:[]` → у продавца нет
  FBS/rFBS-складов (FBO-схема); отчёт неприменим.
- ✅ accrual_postings — **auto-fill** (коммит 881f71a): если `posting_numbers` не
  переданы — `fetch_posting_numbers` через `/v2/posting/fbo/list` (номера отправлений
  за период), батчинг ≤200. Живой прогон: 665 отправлений → xlsx 2300 строк начислений.
  FBO-only (для FBS нужен /v3/posting/fbs/list). Был ⚠️ «нужен --posting-numbers».
- ✅ returns — **полный отчёт** (коммит 84c7e51): переведён с `/v2/report/returns/create`
  (требовал обязательный `filter.status`, одно значение из 35) на `/v1/returns/list` (JSON).
  Намеренно НЕ шлёт filter.status/return_schema → **все возвраты: все статусы, FBO+FBS**.
  Пагинация last_id+has_next; xlsx с 20 русскими колонками (вложенные поля).
  Живой прогон: 420 строк (vs 383 для одного статуса ранее). Был «unknown status»
  (год считали серверным багом — наш пропуск API-change марта 2025).
- ❌ Не-наши проблемы (ПЕРЕПРОВЕРЕНО этим чатом — запросы валидны по доке+changelog):
  `b2b_sales` (нет документа B2B-продаж у аккаунта — неприменимо, как warehouse_stock),
  `postings` (create успешен → запрос валиден; падает генерация на стороне Ozon).

**CLI-флаги** (остаток A): `--posting-numbers`, `--warehouse-ids`, `--skus` (CSV).
**Живые API-тесты теперь разрешены пользователем** (раньше — нет).

### auto-fill SKU для analytics_stocks (этот чат, коммит fab7769)
`ozon.analytics_stocks` («Аналитика по остаткам») требует обязательный `skus[]`
(≤100 за запрос), раньше передавался только через CLI `--skus` → в **GUI отчёт
был нерабочий** (нет поля ввода → 4xx). По образцу warehouse auto-fill:
- **`client.fetch_skus()`** — `POST /v3/product/list` (cursor `last_id`, `limit`
  1000, `filter.visibility:"ALL"`) → собирает числовые SKU всех товаров (поле
  `sku`, int64 — именно их ждёт `/v1/analytics/stocks`, НЕ `offer_id`-артикул).
- **`PaginationKind::Skus`** (новый вариант, заменил `Single`) — auto-fill если
  SKU не переданы + батчинг ≤100 + **pacing 1с между батчами** + batch-level
  retry (5× с паузой 10с). Pacing нужен: метод имеет жёсткий per-second rate
  limit, и без него каталог ~2840 SKU = 29 батчей триггерит 429
  «You have reached request rate limit per second».
- **Живой прогон** (oz_prof1, 2026-07): auto-fill нашёл 2840 SKU, 29 батчей за
  ~36с, файл `ozon.analytics_stocks_2026-07.xlsx` (290 КБ). Без `--skus`.
- Эндпоинт сверен с `docs/ozon-seller-api-reference.md` (урок №22). Серверный
  retry в клиенте (3 попытки) оказался недостаточен — добавлен batch-level retry.

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
- **Форматы файлов Ozon — по РЕАЛЬНОМУ содержимому (magic bytes), не захардкожено**
  (фикс `ac06d33`: раньше `OzonAsyncReport` писал всем `.xlsx`, но часть отчётов Ozon
  отдаёт CSV → файл `.xlsx` с CSV внутри = обман; найдено живым прогоном):
  - Серверный xlsx (настоящий Excel, ZIP/PK): compensation, decompensation,
    mutual_settlement, b2b_sales, placement_by_*, marked_products_sales, discounted.
  - Конвертация JSON→.xlsx через `rust_xlsxwriter` (`xlsx.rs`, русские заголовки):
    buyout, balance (3 листа), realization, cash_flow, analytics_stocks/turnover,
    accrual_postings/by_day (2 листа). Расширение честное (контент — настоящий xlsx).
  - **`.csv` (Ozon отдаёт CSV, расширение по magic bytes):** `products` («;»+BOM),
    `realization_posting` («,»). Не конвертируются в xlsx — честное `.csv`.
  - `detect_format(bytes)`: PK→xlsx, `{`/`[`→json, `<`→xml, прочий текст→csv.

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

## 7. API WB — сверен с официальной спекой + живым прогоном (аудит 2026-08-14)

**Источник:** официальная OpenAPI-спека WB через зеркало `github.com/eslazarev/
wildberries-sdk`, каталог `specs/*.yaml` (14 файлов, генерируются из
dev.wildberries.ru; сам сайт за антиботом). Всё ниже ПОДТВЕРЖДЕНО живым прогоном
(wb_prof, период 2026-07, 12/12 отчётов OK).

### Домены (ВСЕ подтверждены nslookup + спекой)
| Домен | Назначение |
|-------|-----------|
| `finance-api.wildberries.ru` | Баланс, отчёты реализации, эквайринг |
| `documents-api.wildberries.ru` | Категории, список, скачивание документов |
| `statistics-api.wildberries.ru` | Заказы, продажи |
| `seller-analytics-api.wildberries.ru` | Штрафы, антифрод, приёмка |
| `returns-api.wildberries.ru` | Возвраты (claims) — подтверждено спекой 09-communications |

**api.wildberries.ru НЕ существует (NXDOMAIN) — не использовать!**

### Баланс
- `GET /api/v1/account/balance`, БЕЗ параметров. Ответ — ПЛОСКИЙ объект
  `{currency, current, for_withdraw}` (никакого `{data:[...]}`). Лимит 1/мин.

### Финансы (все POST, тело JSON; даты RFC3339 Москва, годится и `YYYY-MM-DD`)
- **list-методы** (`/api/finance/v1/sales-reports/list`, `/acquiring/list`):
  тело `{dateFrom*, dateTo*, limit≤1000, offset}`; ответ — ПРЯМОЙ массив,
  `total` НЕТ → конец по неполной странице.
- **detailed-методы** (`/sales-reports/detailed`, `/acquiring/detailed`):
  тело `{dateFrom*, dateTo*, limit≤100000, rrdId}`; пагинация — КУРСОР rrdId
  последней строки; конец данных — **204 No Content** (клиент обязан трактовать
  как пустой массив). offset НЕ существует. Живой прогон: июль = 3217 строк
  (до фикса молча обрезалось до 1000). Есть также `POST .../detailed/{reportId}`
  (reportId из list-строк, int64).
- Лимит 1/мин.

### Документы (documents-api, GET)
- **categories:** GET /api/v1/documents/categories → `{data:{categories:[{name,title}]}}`
- **list:** GET /api/v1/documents/list, параметры: `locale, beginTime, endTime, sort, order, category, serviceName, limit(≤50), offset` → `{data:{documents:[...]}}`. Поля документа (схема GetListDataDocumentsInner): `serviceName, name, category, extensions, creationTime, viewed`.
- **download:** GET /api/v1/documents/download, параметры: `serviceName, extension` → `{data:{fileName, extension, document(base64)}}`.
- **Rate limits documents:** 1 req/10s burst 5.

### Статистика (statistics-api, GET)
- `/api/v1/supplier/orders`, `/sales`: параметры ТОЛЬКО `dateFrom` (RFC3339,
  фильтр по **lastChangeDate** — дате изменения, не дате заказа) и `flag`
  (0|1). **dateTo и limit НЕ существуют** — не отправлять. Ответ — прямой
  массив; курсорная пагинация (при >80k строк dateFrom = lastChangeDate
  последней строки) — на объёмах месяца не требуется. Данные хранятся 90 дней.

### Удержания (seller-analytics, GET)
- `/api/analytics/v1/deductions`, `/measurement-penalties`: `dateFrom`
  (date-time, опц.), **`dateTo` (date-time, ОБЯЗАТЕЛЕН)**, **`limit`
  (ОБЯЗАТЕЛЕН, ≤1000)**, `offset`. Ответ `{data:{reports:[...],total}}` —
  пагинация по total.

### Антифрод (seller-analytics, GET)
- `/api/v1/analytics/antifraud-details`: единственный параметр `date`
  (ГГГГ-ММ-ДД, опц.; без него — все данные с авг 2023). dateFrom/dateTo/limit
  НЕ существуют. Ответ `{details:[{nmID,sum,currency,dateFrom,dateTo}]}`
  (недельные интервалы) → фильтрация по периоду ЛОКАЛЬНО (пересечение).
  Лимит: 1 запрос/10 мин.

### Возвраты claims (returns-api, GET) — подтверждено спекой
- `GET /api/v1/claims`: параметры `is_archive` (**ОБЯЗАТЕЛЬНЫЙ** bool: false —
  на рассмотрении, true — в архиве), `id`/`nm_id` (опц.), `limit` (1–200),
  `offset`. **dateFrom/dateTo НЕ существуют** — сервер отдаёт заявки за
  фиксированные последние 14 дней → фильтрация по периоду ЛОКАЛЬНО по `dt`.
  Ответ `{claims:[...], total}`. Поля: `id(UUID), claim_type, status, nm_id,
  user_comment, wb_comment, dt, imt_name, order_dt, photos[], ...`. Выгружаем
  ОБА среза (is_archive false+true). Лимит 20/мин.

### Приёмка acceptance_report (seller-analytics, GET, async) — реализовано
- 1) `GET /api/v1/acceptance_report?dateFrom&dateTo` (**ГГГГ-ММ-ДД**, ≤31 дня)
  → `{data:{taskId}}`; 2) poll `GET .../tasks/{id}/status` → `{data:{status}}`
  до `done` (ошибочные: error/failed/cancel*); 3) `GET .../tasks/{id}/download`
  → прямой массив строк; **204 = данных за период нет**. Таймаут 10 мин;
  дескриптор `max_range_days=31` → GUI режет длинные интервалы на окна.

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
1. ~~**Async-отчёты WB** (acceptance_report)~~ — **СДЕЛАНО** (аудит 2026-08-14):
   полная реализация GET create→taskId→poll status→download, max_range_days=31.
2. ~~**max_parallel_jobs** планировщика~~ — **ПОЧИНЕНО** (коммит c42a86b): RunningGuard
   (Drop) декремментит счётчик; раньше застопоривался после N задач.
3. ~~**TestProvider в release**~~ — **ПОЧИНЕНО** (P2-4): TestProvider регистрируется
   только в debug-сборках.
4. **Очистка rate-limiter от ожидания 60с в тестах** — `MDWF_WB_NO_RATELIMIT=1`
5. **Автосохранение может восстанавливать test.documents** при провайдере WB — stale state (есть частичная защита от гонки).
6. **WB-отчёты сохраняются как JSON** — Ozon конвертирует в xlsx с русскими
   колонками, WB пока отдаёт pretty-JSON (потенциальный UX-хвост; в бэклоге).

### Не подтверждено первоисточником (может быть неверно)
- (пусто — claims-эндпоинт и формат ПОДТВЕРЖДЕНЫ спекой 09-communications.yaml
  и живым прогоном 2026-08-14)

### Не-наши проблемы Ozon (ПЕРЕПРОВЕРЕНО этим чатом — не наш баг, в отличие от returns)
- `ozon.b2b_sales` — `getFinanceDocumentID: finance document not found`. Запрос
  `{date: YYYY-MM}` **корректен** (дока подтверждает — это единственный обязательный
  параметр). Означает: у аккаунта **нет документа реестра продаж юр. лицам** за период
  (не продаёт B2B). **Неприменимо к аккаунту**, как warehouse_stock без FBS-складов.
- `ozon.postings` — `Failed to build report. Try again later`. Запрос корректен
  (filter.processed_at_from/to обязательны с changelog — мы их шлём). **Create
  успешен** (получаем code → формат валиден), падает только генерация на стороне Ozon.
  Воркэраунд: меньший диапазон дат через GUI (возможно объём/таймаут).
- **Перепроверка подтвердила**: оба — честно не наш баг (запросы валидны по доке +
  changelog). В отличие от `returns`, который год считали серверным, а он был нашим
  пропуском API-change (теперь полный отчёт через /v1/returns/list).
  Возможно специфика аккаунта `oz_prof1` или отсутствие данных за период.

### oz_prof1: нет FBS/rFBS-складов
`/v2/warehouse/list` вернул `warehouses:[]` → аккаунт работает по FBO-схеме.
`ozon.warehouse_stock` («Остатки на FBS-складе») неприменим (теперь понятная
ошибка вместо 4xx). Для аккаунта с FBS-складами авто-fill отработает.

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
22. **НЕ «придумывать» API — сверять с первоисточником И проверять живым прогоном.**
    Аудит Ozon выявил: 19 из 21 отчётов падали из-за того, что код строился по догадкам,
    а не по доке/тестам. Три корневые причины: (A) даты не подставлялись из `--period`
    (только GUI заполнял `values`); (B) нет поллинга `/v1/report/info` (waiting=ошибка);
    (C) `accrual_by_day` неверный формат даты. Урок: **после любой правки API — живой
    прогон** (CLI `download` с реальным профилем), смотреть фактическую ошибку сервера,
    не угадывать. Сообщения об ошибках Ozon (`invalid DateFrom`, `value length must be
    10 runes`, `at least 1 item`) — точные подсказки, что именно не так.
23. **Ozon async-отчёты требуют поллинга.** Паттерн: create → code → `/v1/report/info`
    в цикле (sleep ~5с, таймаут ~10 мин) до `success`/`failed`. `waiting`/`processing` —
    НОРМАЛЬНЫЕ промежуточные статусы, НЕ ошибки. Без поллинга отчёты
    compensation/decompensation/mutual_settlement/products/discounted/realization_posting
    и др. падают «отчёт ещё генерируется».
24. **Разные эндпоинты Ozon требуют разные форматы дат** — НЕ один формат на всех:
    `month`+`year` int (realization); `date` YYYY-MM (compensation); `date_from`/`date_to`
    YYYY-MM-DD (balance/buyout/placement); `date`/`filter.date_from` ISO datetime
    (returns/postings/cash_flow); `date` YYYY-MM-DD (accrual_by_day, marked).
    Сверяй формат ПО ДОКЕ для каждого эндпоинта отдельно.
25. **Локальная копия `docs/ozon-seller-api-reference.md` — рабочий источник** (свежая,
    от 6 авг 2026). docs.ozon.ru за антиботом (WebFetch не проходит). Если копия
    устареет — попросить пользователя скачать свежую (НЕ придумывать).
26. **Per-method rate limits могут быть жёстче глобального.** Глобальный лимит Ozon —
    50 RPS (клиент умеет: `RateLimiter` 20мс интервал), но конкретные эндпоинты
    имеют свои, часто недокументированные per-second лимиты. `/v1/analytics/stocks`
    на каталоге 2840 SKU (29 батчей по 100) давал 429 «rate limit per second» при
    темпе ~48 RPS, хотя глобальный лимит не исчерпан. Решение: **pacing между
    батчами** (1с) + batch-level retry (поверх per-request retry в клиенте, который
    даёт лишь 3 попытки 500/500/1000мс — маловато для throttle, снимаемого >3с).
    Урок: циклы из многих запросов к одному эндпоинту — **явно пейсить**, не
    полагаться только на глобальный limiter.
27. **GUI и CLI разделяют code-path провайдера.** Все 5 фиксов аудита Ozon
    (A/B/C/marked/warehouse) лежат в shared-коде `crates/providers/ozon/`, не в
    CLI-обвязке → GUI получает их автоматически. Проверка «работает ли фикс в GUI»
    = аудит code-path (GUI вызывает тот же `Report::download`), а не ручной клик.
    Единственное, что GUI не получал от CLI — опциональные параметры (`skus`,
    `posting_numbers`, `warehouse_ids`), у которых не было виджетов ввода.
    `warehouse_ids` и `skus` теперь auto-fill на стороне провайдера (GUI работает
    без виджетов); `posting_numbers` — пока CLI-only (нужен отдельный auto-fill).
28. **Регрессии в тестах незамеченными — проверять `cargo test` после правок.**
    Фикс `marked_products_sales` (date-only) из прошлого чата сломал тест
    `build_body_marked_products_nested_date` (ассерты ждали ISO datetime), но это
    всплыло только при отдельной проверке в этом чате. Урок: после любой правки
    тела запроса — `cargo test -p <crate>` обязателен, даже если «правка маленькая».
29. **НИКОГДА не захардкоживать формат/расширение файла.** `OzonAsyncReport`
    сохранял все async-отчёты как `.xlsx`, но Ozon отдаёт разные форматы: `products`
    и `realization_posting` — CSV, остальные — настоящий xlsx. Файл `.xlsx` с CSV
    внутри — обман пользователя (раскрыл живой прогон + проверка magic bytes:
    `PK`=настоящий xlsx, `EF BB BF 22`=CSV с BOM, `72 6F 77`=CSV «row…»).
    Правильно: `detect_format(bytes)` по сигнатуре (PK→xlsx, `{`/`[`→json, `<`→xml,
    текст→csv). Расширение обязано СООТВЕТСТВОВАТЬ реальному содержимому — это
    инвариант, проверяемый magic bytes, а не предположением о сервере. Урок:
    расширение ≠ «как мы хотим назвать», расширение = «что реально в файле».
30. **НЕ хранить отображаемую строку как путь в БД.** CLI сохранял в
    `downloads.file_path` строку «filename (size байт)» (формат вывода в консоль)
    вместо реального пути на диске — ломало «Архив» (📂/📁/📋 не работали для
    CLI-файлов). GUI делал правильно (`full_path.display()`). Выявило только при
    очистке обманных .xlsx — 23 записи аудита имели искажённые пути. Фикс: CLI
    использует `save_with_dir` (возвращает директорию) + `dir.join(file_name)`,
    как GUI. Урок: каталог и UI-вывод — РАЗНОЕ; в БД пишем реальный путь,
    отображение форматируем отдельно в точке вывода.
31. **Проверять changelog API — обязательные параметры появляются со временем.**
    `ozon.returns` год падал «unknown status» и числился «серверным багом Ozon».
    На самом деле с 5 марта 2025 Ozon сделал `filter.status` обязательным
    (`/v2/report/returns/create`, proto-enum, одно значение — массив отвергается).
    Локальная копия дока имела changelog (line 46296), но мы не сверялись с ним.
    Урок: «серверная ошибка» может быть пропущенным API-change — всегда смотреть
    changelog метода (раздел «Изменения» в доке). Стабильная одинаковая ошибка на
    разных периодах = красный флаг, что мы шлём невалидный запрос, а не «баг Ozon».
32. **Архив/списки: показывать человекочитаемые имена, не type_id.** В Архиве
    колонка «Отчёт» и combo фильтра показывали технические type_id (`ozon.products`)
    — непонятно. Резолв type_id→display_name через `capabilities().reports`
    (синхронно, без API), combo по паттерну label→value (видим имя, фильтр по type_id).
    Урок: всё, что видит пользователь — человекочитаемые имена; type_id только
    внутри (БД/API), с tooltip для точной идентификации при необходимости.
33. **Иконка exe: PNG-in-ICO + winres, без ImageMagick.** В MSYS2-окружении НЕТ
    ImageMagick (`convert` — это Windows-утилита FAT→NTFS!), Pillow, icotool. ICO
    собирается вручную (заголовок 6 байт + директория 16 байт/изображение + PNG-блобы;
    PNG-in-ICO работает на Win10/11). Затем `winres` (build-dep) встраивает `.ico` в exe
    через `windres` (должен быть в PATH → `scripts/env.sh`). Проверка —
    `ExtractAssociatedIcon`. Урок: не рассчитывать на `convert`/`magick` в MSYS2.
34. **Relocatable GTK-бандл на Windows.** Бандл должен сам находить свои ресурсы:
    `main.rs::setup_bundle_env` ДО `gtk::init` ставит `XDG_DATA_DIRS` (→ share/icons
    Adwaita + glib-2.0/schemas) и `GDK_PIXBUF_MODULE_FILE` (→ lib/gdk-pixbuf/.../
    loaders.cache) на соседние с exe папки — иначе на чистой машине битые иконки/схемы.
    DLL — `ntldd -R` (рекурсивно, отсечь системные), НЕ хардкод-список. gdk-pixbuf
    loaders лежат в `lib/gdk-pixbuf-2.0/2.10.0/loaders/` (поддиректория!), cache —
    уровнем выше; пересобирается инсталлером (postinstall.bat) под путь установки
    (relocatable — gdk-pixbuf вычисляет prefix по расположению cache). Урок: GTK на
    Windows relocatable только через env-сетап в main.rs + правильную структуру share/lib.
35. **Relocatable-бандл проверять СКРАБЛЕННЫМ env, а не «запуском на дев-машине».**
    Дев-машина ВРАЁТ: GTK4/libadwaita стоят глобально в `D:\msys64\mingw64` и есть
    в PATH → бандл работает ИЗ-ЗА системы, а не из своих ресурсов. Чистая Windows
    этого не имеет. Методология (`scripts/test-clean-env.sh`): запуск `dist/mdwf/
    mdwf-gui.exe` через `env -i` с PATH = ТОЛЬКО System32 (вырезано msys64/mingw),
    сняты `GTK_*`/`GDK_*`/`GSETTINGS_*`/`FONTCONFIG_*`/`XDG_*`/`GLIB_*`, `timeout N`
    (exit 124 = ОК). Только так видна реальная полнота бандла. Дополнительно
    `scripts/test-installed.sh` воспроизводит flow инсталлера (копирование в путь
    С ПРОБЕЛОМ → postinstall.bat → запуск) — имитация `C:\Program Files\MDWF`.
    ИТОГ проверки v1.4.0: бандл ПОЛОН — все DLL на месте (app стартует без MSYS2),
    установленный через реальный `setup.exe` апп делает живой API-вызов (UI построился).
36. **GNU `timeout` в MSYS НЕ убивает native Windows GUI-процесс.** SIGTERM не
    доходит до Windows `.exe` → процесс-«зомби» остаётся жить после `timeout N`.
    Для GUI-тестов обязателен `taskkill //IM mdwf-gui.exe //F` ДО и ПОСЛЕ каждого
    прогона (см. `scripts/test-clean-env.sh`). Без него зомби накапливаются.
37. **`gtk::Application` (libadwaita `adw::Application`) с фиксированным APP_ID →
    single-instance ломает тесты.** Если живёт «зомби»-primary (см. #36), следующий
    запуск видит его по APP_ID, форвардит `activate` и **немедленно выходит (exit 0,
    без окна)** — выглядит как «краш через 0.9с», но это артефакт. Симптом: в логе
    `scheduler loop started` → `MDWF GUI exited` через <1с, без ошибок. Не баг
    продукта — лечится `taskkill` primary. Вывод: при «мгновенном чистом выходе» GUI
    СНАЧАЛА проверить `tasklist` на живой одноимённый процесс, потом уж искать баг.
38. **Чёрное окно GTK4 на Windows — форсировать `GSK_RENDERER=gl`.** Дефолтный
    рендерер NGL на некоторых GPU-драйверах (на чистой машине без MSYS2) даёт
    полностью ЧЁРНОЕ окно (известная проблема GTK4-on-Windows). `gl` (legacy GL)
    совместим с практически любой GPU и достаточен для business-app (формы/данные).
    Ставится в `main.rs::setup_bundle_env` ДО `gtk::init`: `set_var("GSK_RENDERER","gl")`
    только если пользователь не задал свой (overridable). `cairo` (software) — крайний
    fallback. Проверка: яркость окна avg>30 + black_fraction<5% (не чёрное). Урок
    вылез при жалобе юзера «чёрное окно» — дев-машина рендерила нормально (NGL там
    работал), баг был только на чистой машине.
39. **Иконка запущенного окна на Windows — НЕ через gresource.** GTK4
    `gtk_window_set_default_icon_name` на Windows упорно НЕ находит иконку из
    `IconTheme::add_resource_path` + gresource: проверены паттерны `themed/<size>x<size>/apps/`
    (+index.theme), `scalable/apps/`, `<size>x<size>/apps/` — везде `has_icon=false`
    (при том что `has_icon("open-menu-symbolic")=true` — тема живёт). Рабочий путь:
    **ships как ФАЙЛЫ в on-disk теме hicolor бандла** (`share/icons/hicolor/<size>x<size>/apps/mdwf.png`),
    `build-release.sh` их кладёт. exe-иконка (winres) — отдельно, для проводника/ярлыков.
    В коде: `gtk4::Window::set_default_icon_name("mdwf")` после gtk/adw init.
40. **`icon-theme.cache` нужно регенерировать `--force` после добавления иконки.** При
    копировании темы hicolor из MSYS2 подтягивается её `icon-theme.cache`, в котором
    НЕТ новой иконки (mdwf) → GTK доверяет кэшу, не сканит директории → `has_icon=false`
    → брендовая иконка не ставится на окно. `gtk4-update-icon-cache` БЕЗ `--force`
    тихо пропускает обновление (кэш не меняется) — нужен именно `--force` (или удалить
    кэш → GTK fallback на скан). Проверено: `--force` → кэш содержит mdwf → has_icon=true.
    Кэш self-contained (без абсолютных путей) → relocatable, переживает копирование
    инсталлером. См. `build-release.sh` (шаг иконок hicolor).
41. **«Чёрное окно» при запуске GUI = консоль терминала, НЕ рендеринг GTK.** Rust-
    бинар по умолчанию собирается в **console subsystem** → рядом с GUI-окном
    выскакивает чёрное окно терминала (cmd). Фикс: `#![windows_subsystem = "windows"]`
    в `crates/gui/src/main.rs` (только GUI-бинарь; CLI `mdwf.exe` остаётся console —
    ему терминал нужен). Через `cfg_attr(all(windows, not(debug_assertions)), ...)`
    — release прячет консоль, debug оставляет (видно логи при разработке). Проверка
    PE-заголовка: `objdump -p mdwf-gui.exe | grep Subsystem` → `0x2 (Windows GUI)`
    (было `0x3 Windows CUI`). Это гарантия на уровне ОС — Windows не аллоцирует
    консоль для GUI-subsystem PE. Урок: я сначала неправильно понял жалобу «чёрное
    окно» (искал баг рендеринга GTK, добавил GSK_RENDERER=gl — это отдельная
    полезная закалка, но НЕ та проблема) — надо было уточнить, что именно за окно.
    Спойлер: GTK-окно рендерилось нормально всё время.
42. **Single-instance + инсталлер `AppMutex` через named mutex.** Крейт
    `#![forbid(unsafe_code)]` → Win32 `CreateMutexW` напрямую нельзя. Решение: крейт
    `single-instance` (внутри CreateMutex, снаружи safe API). В `main.rs`:
    `SingleInstance::new(SINGLE_INSTANCE_NAME)` ДО `adw::Application` (иначе
    gtk::Application-second-instance форвардит `activate` и выходит — урок #37);
    `if !instance.is_single()` → диалог «уже запущен» через `glib::MainLoop` (НЕ
    deprecated `gtk4::main`) + чистый выход. `_instance` держим до конца `main`
    (дропнуть = отпустить mutex). **Имя mutex ДОЛЖНО совпадать** с `AppMutex` в
    `installer/mdwf.iss` (`MDWF_App_Mutex`) — Inno проверяет этот mutex и не даст
    ставить инсталлер поверх запущенного MDWF. Константа `SINGLE_INSTANCE_NAME`
    с комментарием-предупреждением. Проверка: `OpenMutex` из другого процесса
    видит mutex пока MDWF запущен; второй инстанс → exit 124 (блокирует на диалоге),
    в логе НЕТ «MDWF GUI starting» (ветка диалога, не app). Тонкость тестов: bash-
    переменная пути почему-то пустела во второй команде — литеральный путь надёжнее.
43. **Иконка приложения в exe — НЕ использовать winres при glib-build-tools.** Симптом:
    в проводнике/ярлыках/таскбаре ВСЕГДА дефолтная иконка (а не бренд). Корень: крейт
    `winres` генерит `resource.o` (с иконкой + version-info), но НЕ эмитит для него
    `cargo:rustc-link-arg` — потому что `glib-build-tools::compile_resources` эмитит
    `cargo:rustc-link-lib=static=resource` (→ `libresource.a`, gresource), и winres-овский
    объект с тем же базовым именем «resource» теряется. Итог: `.rsrc` exe = 1536 байт
    (только version-info или пусто), иконки НЕТ. Проверка: `objdump -h exe | grep rsrc`
    + Python-скан `.rsrc` на DIB-заголовки (biSize=40) и брендовые цвета. Фикс: в build.rs
    ВРУЧНУЮ — `.rc` (иконка + VERSIONINFO, АБСОЛЮТНЫЙ путь к .ico со слешами `/`, т.к.
    windres cwd=OUT_DIR) → `windres -O coff` → `cargo:rustc-link-arg=<o>`. winres убран.
44. **ICO: малые размеры (16-128) — BMP/DIB, 256 — PNG. НЕ all-PNG.** `make-icon.sh`
    раньше заворачивал ВСЕ размеры как PNG-blob'ы в ICO. Windows для малых/средних
    значков (16/32/48 — таскбар/список/проводник) ждёт BMP/DIB; PNG-in-ICO надёжно
    работает только для 256. All-PNG ICO → Windows показывает дефолт, а .NET
    `Icon(file,size)` падает. Pillow/icotool/ImageMagick в MSYS2 НЕТ → энкодер
    `scripts/ico_encode.py` (чистый stdlib zlib+struct): PNG→decode→BMP-DIB
    (BITMAPINFOHEADER + BGRA bottom-up + AND-mask) для 16/24/32/48/64/128, 256 — PNG.
    Проверка: .NET `Icon(ico,48)` OK + брендовый фиолетовый в пикселях; .NET на all-PNG
    версии падал. См. также #43 (иконка должна ещё и влинковаться в exe).
45. **`period_kind` в ReportDescriptor — источник правды о периоде отчёта.** Параметры
    `param_period_month`/`param_date_range` в дескрипторе ненадёжны: 5 Ozon-отчётов
    заявляли `param_date_range`, но тело запроса (`build_download_body`) шлёт только
    `period` (один месяц) — UI показывал диапазон, а уходил один месяц. Решение: явный
    `PeriodKind { Month, Range, Day, None }` в дескрипторе (`#[serde(default)]` = Range),
    заполняемый по сверке с API. Он ведёт UI («Скачать по периоду»: Month → цикл по
    месяцам, Range → один запрос) и инфо-панель. Урок: дескриптор-параметры описывают
    форму ввода, но НЕ семантику периода — для логики нужен отдельный явный enum.
46. **Месячные API-отчёты + multi-month интервал → цикл по месяцам в UI.** API месячных
    отчётов (Ozon realization/compensation/…, 6 шт.) принимает строго один месяц. Если
    пользователь выбрал квартал/год, нельзя просто взять стартовый месяц (потеряется
    `date_to`). Правильно: «Скачать по периоду» итерирует все месяцы `[date_from..date_to]`
    и шлёт N отдельных `UiCommand::Download` (по `period` на каждый). Период для
    Range/Day/None — один запрос за весь диапазон. Хелпер `months_in_current_range()`
    (chrono `checked_add_months`). Считается и для инфо-ноты («соберём по месяцам: N мес.»).
47. **SQL: оборачивать OR-предикат в скобки при AND-джойне WHERE.** Динамический
    WHERE собирается `clauses.join(" AND ")`. Если один из clauses содержит `OR`
    без внешних скобок — `... AND a=? AND b=? AND (x) OR (y)` парсится как
    `(a AND b AND x) OR (y)` (AND сильнее OR) → ветка `y` проверяется БЕЗ фильтров
    a/b → лишние строки. В архиве фильтр «дата начала ИЛИ конца ∈ [f,t]» давал h3
    (wb-документ) при фильтре profile=ozon — именно из-за этого. Правильно: весь
    OR-clause в двойных скобках `((x) OR (y))`. Поймано тестом
    `list_downloads_filtered_all_and_combinations` (combo-кейс: ждали 1, получили 2).
48. **Архив: интервальный фильтр вместо периода + совпадение по граничной дате.**
    Во вкладке «Архив» month/year combos заменены виджетом интервала (тот же
    `widgets::interval_picker`). `ListArchive`/`ArchiveState` несут `date_range`
    (Option<(from,to)>) вместо `period`. Предикат каталога изменён с «пересечения
    диапазонов» на «дата НАЧАЛА или КОНЦА отчёта попадает в [f,t]» (start_expr/end_expr
    CASE: period YYYY-MM→месяц, YYYY-MM-DD→точка, иначе document_date). Кнопка «✕ Дата»
    сбрасывает фильтр (все записи); выбор интервала авто-применяется. Урок #47 (скобки).
49. **При смене типа виджета — обновить ВСЕ места, знающие старый тип.** Кнопка
    «Открыть папку» была `LinkButton`, потом стала `Button` (фикс 2-х проводников),
    а очистка старой продолжала искать LinkButton → кнопки размножались после каждой
    загрузки. Урок: тип виджета — часть контракта; grep'ить старый тип по крейту при
    такой замене.
50. **PowerShell stdout в редирект = OEM (cp866), а не UTF-8.** Кириллица из ps1-скрипта,
    записанная в файл, не матчится UTF-8 паттернами bash (grep/awk) — «слово не найдено»
    без видимой причины. Фикс: в начале скрипта `[Console]::OutputEncoding =
    [System.Text.Encoding]::UTF8`. Плюс: ps1-файлы держать ASCII-only (без BOM PS 5.1
    читает как ANSI); кириллицу передавать через UTF-8 файл, читаемый `Get-Content
    -Encoding UTF8`. Ещё: `SetForegroundWindow` из фонового процесса блокируется Windows —
    нужен AttachThreadInput-трюк или WScript.AppActivate.
51. **GUI-автотесты без a11y: OCR + SendInput + DPI-aware.** GTK4 на Windows не
    экспортирует UI Automation/AT-SPI → widget-level автотеста нет. Рабочая связка
    (scripts/gui-test/): DPI-aware скриншот → Windows OCR (`Windows.Media.Ocr`,
    читает кириллицу, отдаёт прямоугольники слов) → координата = центр слова →
    SendInput-клик. Все скрипты обязаны звать `SetProcessDPIAware`, иначе координаты
    OCR (физические) не совпадут с курсором (виртуализированные). ⚠️ Клики идут по
    реальному рабочему столу — не запускать, пока юзер за машиной.
52. **Сохранённые сущности показывать человекочитаемо, не «как они хранятся».** Список
    расписаний Планировщика показывал сырой cron («0 2 1 * *»), который пользователь
    задаёт через ДРУГОЙ интерфейс — человеческий диалог «Когда…» (частота/день/время),
    где cron собирается сам. Показывать обратно техническое поле = разрыв: вводили
    по-человечески, видим цифры. Плюс `period_offset` (за какой месяц) вообще НЕ
    отображался в строке — терялась половина смысла расписания («когда» было, «за какой
    период» — нет). Правильно: единый `describe_schedule(s)` собирает ЧТО (отчёт) /
    ЗА КАКОЙ ПЕРИОД / КОГДА в одно предложение; сырые поля (cron, offset) — dim/tooltip
    для справки, не primary-контент. Урок: хранение (cron+offset) и отображение —
    РАЗНЫЕ задачи; UI чтения должен зеркалить UI ввода по понятности, а не формат
    хранения. Заодно: clippy `-- -D warnings` обязателен после правок — UX-пакет
    накопил ~20 предупреждений, потому что clippy не гоняли (см. урок #28).
53. **Хранилище в UTC, UI/логика — в локальном таймзоне; не путать.** Расписания
    хранятся как RFC3339-UTC в SQLite, и `claim_schedule` сравнивает такие строки
    лексикографически (= хронологически) — инвариант ТОЛЬКО при одинаковом
    смещении `+00:00`. Но cron юзер задаёт по своим часам («02:00» = локально).
    Прежде `next_run` считал cron в UTC → для MSK «02:00» срабатывало в 05:00.
    Правильно: считать cron в `chrono::Local`, результат приводить к UTC для
    хранения (8 callers не менялись — шлют `Utc::now()`, хранят `to_rfc3339()`
    UTC). Показывать юзеру — тоже локально (`fmt_local`), иначе UTC путает.
    Тесты — через generic `next_run_in<Z: TimeZone>` + `FixedOffset`, НЕ через
    `Local` (иначе зависят от таймзоны машины и флапают). Соседние уроки этого
    пакета: **(breaker)** защита должна не только «считать» состояние, но и
    консультироваться перед запросом — `check()` написали, в `post_url` забыли
    воткнуть → мёртвый breaker = ложное чувство безопасности. **(limiter)**
    `Instant::duration_since(future)` насыщается до 0 — reservation-алгоритм
    (слот `last + interval`, `backoff` через `max`) в эту яму не падает;
    конкурентность (лимитер за `Arc`) требует слот-бронирования, а не «now+wait».
    **(magic bytes)** расширение = РЕАЛЬНЫЙ контент (`%PDF`→pdf), не
    предположение о сервере (b2b_sales — PDF, а сохраняли .csv). Общее: даже
    «рабочий» код скрывает баги, которые ловит только целенаправленный ревью, а не
    живой прогон (breaker/limiter/PDF не проявлялись как «ошибка юзеру»).
54. **UX для обычного пользователя ≠ UX для разработчика.** Сеньор-разбор 9 вкладок
    глазами бухгалтера/селлера выявил системную утечку ЖАРГОНА в интерфейс:
    type_id как префикс каждого отчёта («ozon.realization — …»), `{:?}`-рендер
    enum-категорий (Finance/Penalties), ISO-даты YYYY-MM-DD, «Q1»/«Н37», сырые id
    маркетплейсов в форме («ozon» нижним регистром), поля «Client-Id»/«Api-Key»
    как HTTP-заголовки, HTTP-коды в ошибках, три глагола (скачать/загрузка/
    выгрузка) для одного действия. Разработчику это прозрачно; обывателю — стена
    из технических строк. Правила: (1) идентификаторы (type_id, cron, id) —
    ВНУТРИ, пользователю — display_name/русский; (2) enum → русский display_ru(),
    а не `{:?}`; (3) ошибки — причина+подсказка восстановления в точке показа
    (friendly_error), а не «HTTP 401»; (4) ОДИН глагол на действие во ВСЕХ
    поверхностях (кнопки+статус+журнал+справка); (5) формат даты — локальный
    (ДД.ММ.ГГГГ). Тесты НЕ ловят жаргон (строки компилируются и «работают») —
    ловит только UX-ревью глазами целевого юзера.
55. **Сменил отображение combo — проверь ВСЕ чтения (active_text vs active_id).**
    Combo «маркетплейс» показывал «ozon» (сырой id) и код читал `active_text()` → id.
    При переводе на «Ozon» (display_name) через `append(id, display_name)` значение
    стало доступно только через `active_id()` — а `on_auth_fields_loaded` всё ещё
    звал `active_text()` → сравнение с provider_id всегда ложно → поля диалога НЕ
    отрисовывались (регрессия QW6, всплыла при работе над [id]-суффиксами). Урок:
    тип значения combo (активный текст vs активный id) — часть контракта; при смене
    способа заполнения grep'ить ВСЕ `active_text`/`active_id` этого combo. Правильнее
    вообще НЕ парсить видимый текст («Display [id]»), а хранить id как значение
    (`active_id`) — это и устойчивее, и даёт человекочитаемый текст без суффиксов.
    Соседний урок: цикл команд, блокирующийся на `do_download.await`, НЕ мешает
    отмене — `cancel_current()` дёргает токен через общий `Arc<Mutex>` с UI-потока,
    в обход цикла; провайдер лишь должен опрашивать `is_cancelled()` в своих циклах.
56. **`cargo test` НЕ перелинковывает основной bin-exe.** Живой клик-тест запустил
    debug-бинарник, собранный ДО последних коммитов, — на скриншоте был старый UI
    (плоский sidebar, «[ozon]»-суффиксы). cargo test компилирует bin как
    unittest-артефакт (другое имя), а `target/debug/mdwf-gui.exe` обновляет только
    `cargo build`. Правило: перед ручной проверкой GUI — явный `cargo build -p mdwf-gui`
    (или `cargo run`). Проверять свежесть по timestamp exe. Сам клик-тест (методология
    из урока #51) отработал: фаза-1 без кликов (скриншот окна по HWND + OCR —
    подтвердили секции sidebar / «✎ Изменить» / отсутствие [id]) → фаза-2 клики
    только по словам внутри rect окна (origin от shot.ps1 «WxH+X+Y», координаты
    слова из OCR -Words). ДД.ММ.ГГГГ проверен и на восстановленном ISO-стейте.
57. **CssProvider в GTK 4.20 (MinGW-бандл) мёртв + имя ноды ≠ css-класс.** Пять
    вариантов (display-wide и widget-scoped, APPLICATION и USER priority, класс
    .mdwf-nav-row, безусловный красный rgb(200,30,30) на ВСЕ строки, селектор
    без точки `navigation-sidebar row`) — НОЛЬ визуального эффекта, без
    CSS-parse warning'ов в stderr. `navigation-sidebar` — это ИМЯ НОДЫ
    (элементный селектор) внутреннего списка StackSidebar в Adwaita; вешать
    css-класс с таким именем на свой ListBox и матчить `.navigation-sidebar`
    бесполезно. Рабочий путь БЕЗ CSS: штатные style-классы виджета —
    `label.set_css_classes(&["heading", "accent"])` на активной строке
    (жирный + фирменный цвет; verified пиксель-сканом: акцент-кластер ровно
    один и переезжает на шаг строки при клике). Соседний урок: захардкоженные
    координаты пиксель-выборок гниют (окно двигается между запусками) — перед
    каждым сэмплом заново OCR; и НЕ доверять усреднённому сэмплу строки
    (подложка selection затемняет текст) — искать акцент-ЦВЕТ (R−G>60),
    кластеризовать по y.
58. **Тихие потери данных невидимы без сверки со спекой (WB-аудит).** Три класса
    багов, которые НЕ проявляются как ошибки: (а) **обрезка** — detailed-финансы
    WB отдавали ровно `limit:1000` строк и файл был ВАЛИДНЫМ, просто неполным
    (июль: 1000 вместо 3217; детективный признак — размер ровно на лимите);
    (б) **отсутствующий обязательный параметр** — claims без `is_archive` =
    гарантированный 400, но код о нём не знал (спека — единственный источник
    обязательности; сервер молчал бы, если бы параметр был опциональным);
    (в) **лишние параметры** — statistics `dateTo`/`limit` не существуют
    (могут игнорироваться или ломаться). Паттерны: **204 = конец курсора**
    (не «ошибка протокола» — клиент обязан вернуть пустой массив); когда API
    не поддерживает фильтр по периоду (claims: окно 14 дней; antifraud: только
    date) — фильтровать ЛОКАЛЬНО и честно писать ограничение в описание
    дескриптора; универсальный «ResponseShape-подход» (данные где-то в JSON)
    слабее явного enum семейства запроса (метод+параметры+пагинация в одном
    месте — `WbQueryKind`). И главное: «пустой результат» ≠ «сломано», но и
    ≠ «работает» — пустые deductions/claims/antifraud проверены доп.
    прогонами (другой период / без фильтра), чтобы отличить «нет данных»
    от «не та форма ответа» (`.unwrap_or_default()` глотает разницу!).



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
cargo test --workspace          # ~168 тестов (storage: 19, incl. Архив + delete)
cargo clippy --workspace --tests -- -D warnings

# Release-бандл + инсталлятор ОДНОЙ командой (нужен Inno Setup 6/7):
#   Вариант A — двойной клик по build-setup.cmd (в корне; для Windows: сам найдёт
#               bash из MSYS2/Git Bash и запустит сборку).
#   Вариант B — из Git Bash/MSYS2: bash scripts/build-setup.sh
# → installer/Output/MDWFSetup-<version>.exe (внутри: build-release.sh → бандл
#   dist/mdwf/ ~100 МБ: 2 exe, 77 DLL через ntldd -R, иконки Adwaita/hicolor,
#   схемы, gdk-pixbuf-лоадеры; затем авто-поиск ISCC + компиляция .iss).
# GUI relocatable: main.rs сам настраивает XDG_DATA_DIRS/GDK_PIXBUF_MODULE_FILE
# на соседние share/lib. Инсталлер [Run] пересобирает cache/схемы под путь.
# Inno Setup: jrsoftware.org/isdl.php (если нет — скрипт подскажет).
# ВАЖНО: bash тут = MSYS2-оболочка на Windows (тулчейн проекта: GTK, ntldd,
#   glib-compile-schemas), НЕ Linux. build-setup.cmd — Windows-обёртка над ней.

# Проверка бандла: timeout 8 ./dist/mdwf/mdwf-gui.exe (exit 124 = работает)

# git: после каждого изменения — коммит + push на origin/master
```

---

## 14. С чего начать в новом чате

**В этом чате завершены**:
- **П.5** (иконка типа файла) — официальные PNG из vscode-icons (Excel/PDF/JSON/XML/ZIP/TXT).
- **П.6** (вкладка «Архив») + фикс фильтра периода для WB-документов + `{doc_date}`.
- **4 доп. фичи**: persist фильтров Архива, CLI `archive list`, удаление записи+файла,
  индекс `idx_downloads_filter`.
- **Большой аудит Ozon API** (живой прогон): фикс A/B/C + marked + warehouse auto-fill.
  С 19 FAIL до 18 OK из 21 отчёта.

Все 6 пунктов исходного бэклога закрыты + 4 доп. + аудит Ozon. Последний коммит `7b009a0`.

**Чат 2026-08-12 (GUI-проверка фиксов Ozon + auto-fill SKU):**
- **Аудит code-path GUI vs CLI**: все 5 фиксов (A/B/C/marked/warehouse) в shared-коде
  провайдера → GUI получает автоматически. 15 отчётов заведомо работают в GUI.
- **Починен сломанный тест** `build_body_marked_products_nested_date` (регрессия:
  фикс date-only применён, ассерты ждали ISO datetime). `cargo test` был красный.
  Коммит `6225e4a`.
- **auto-fill SKU для `analytics_stocks`** (коммит `fab7769`): `client.fetch_skus()`
  через `/v3/product/list` + `PaginationKind::Skus` (батчинг ≤100, pacing 1с,
  batch-level retry). Живой прогон: 2840 SKU → 290 КБ xlsx за ~36с. Теперь отчёт
  работает и в CLI, и в GUI (без поля ввода SKU).
- **Фикс обмана с расширением** (коммит `ac06d33`): `products`/`realization_posting`
  сохранялись как `.xlsx`, но Ozon отдаёт CSV. `detect_format(bytes)` по magic bytes
  → честное расширение (xlsx/csv/json/xml). Юзер заметил «.xlsx, а внутри не Excel».
- **Фикс пути CLI в каталоге** (коммит `fe30e40`): CLI хранил отображаемую строку
  «filename (size байт)» вместо реального пути → ломало «Архив». Каталог почищен:
  4 обманные записи products/realization_posting перекачаны как честные `.csv`,
  19 malformed-записей исправлены UPDATE пути. Бэкап БД создан. Урок #30.
- **Человекочитаемые имена в Архиве** (коммит `a13e61b`): колонка «Отчёт» и combo
  фильтра показывали type_id → теперь display_name (резолв через capabilities).
  Урок #32.
- **Починен отчёт по возвратам** (коммиты `c65e9a5` → `84c7e51`): `ozon.returns` падал
  «unknown status» — с марта 2025 `filter.status` обязателен (наш пропуск API-change,
  не серверный баг). Сначала фикс через `--return-status` (один статус), затем переведён
  на `/v1/returns/list` — полный отчёт: все статусы + FBO/FBS одним xlsx (420 строк).
  Урок #31 (проверять changelog).
- **accrual_postings auto-fill** (коммит `881f71a`): `fetch_posting_numbers` через
  `/v2/posting/fbo/list` + `PaginationKind::AccrualPostings` (батчинг ≤200). Живой прогон:
  665 отправлений → 2300 строк начислений. Теперь работает без `--posting-numbers`.
- **Вкладка «Журнал»** (коммит `0f19b90`): была заглушкой → лента событий (выгрузки/
  ошибки/запуски расписаний), cap 500, кнопка «Очистить». `UiEvent::Log(LogEntry)`.
- **Вкладка «Планировщик»** (коммиты `c42a86b`, `3122a1c`): была заглушкой → полный cron-планировщик.
  CRUD расписаний, вкл/выкл, «выполнить сейчас», автозапуск с ОС + **Windows Task Scheduler**.
  Фоновый `Runner::run_loop` (в `App::new` при `enabled_on_start`) — когда GUI открыт.
  **Windows Task Scheduler** (гибрид): один системный таск `MDWF_Scheduler` каждые 5 мин
  запускает CLI `mdwf schedule run` → работает без GUI, переживает логаут. `GuiJobExecutor`
  переиспользует `do_download` (персистит + каталог + лог), применяет `period_offset`.
  **Защита от двойного** (in-process ∩ Windows-таск): `Catalog::claim_schedule` — атомарный
  bump `next_run_at`; кто первый забрал, тот выполняет. `run_due_schedules` теперь due-check
  (раньше гонял все включённые). Фикс `max_parallel_jobs` (RunningGuard). `wintasks.rs`
  (schtasks.exe, disable через Query pre-check — cp866-вывод не парсится).
- **Клик-тест GUI**: пользователь проверял отчёты вручную (returns, open_folder и др.).
  Полный sweep 15 отчётов формально не завершён, но ключевые рабочие.
- **Фикс «Открыть папку» (2 проводника)** (коммит `f5cb652`): LinkButton дублировал
  открытие (URI + clicked) → заменён на Button; `open_folder` через `cmd /c start`
  вместо прямого `explorer` (квирк 2 окон).
- **Дистрибуция: полный relocatable-бандл + Inno Setup** (коммиты `98e49fc`, `6dee243`,
  `a3de175`, `4d71e0a`, `88cb934`, `7b009a0`): см. секцию 13. Бандл 99 МБ (77 DLL через
  `ntldd -R`, иконки Adwaita/hicolor, схемы, gdk-pixbuf-лоадеры), relocatable
  (`main.rs::setup_bundle_env`). `installer/mdwf.iss` → `setup.exe`. Одна команда:
  `build-setup.cmd` (двойной клик, сам ищет bash+ISCC) или `bash scripts/build-setup.sh`.
  **Inno Setup 7.1.0 установлен per-user** на этой dev-машине
  (`%LOCALAPPDATA%\Programs\Inno Setup 7\ISCC.exe`) — iscc доступен.
- **Иконка приложения** (коммит `7b009a0`): SVG (градиент Ozon→WB + знак download) →
  `app-icon.ico` (7 размеров, PNG-in-ICO, ручная сборка — нет ImageMagick/Pillow/icotool,
  `convert` это Windows-утилита). `build.rs` через `winres` встраивает в `mdwf-gui.exe`.

**Чат 2026-08-13 (виджет стандартного интервала + метаданные отчётов):**
- **Просьба юзера**: заменить отдельный выбор месяца/года кнопкой выбора стандартного
  интервала (неделя/месяц/квартал/год) — виджетом с годом-спиннером сверху и вкладками
  с ярлычками; список выбора (не combo), один клик = выбор.
- **Core**: `PeriodKind { Month, Range, Day, None }` + `period_kind`/`description` в
  `ReportDescriptor` (serde-default, обратная совместимость). Чинит прошлый баг: 5
  Ozon-дескрипторов заявляли param_date_range, но тело шлёт только `period` — теперь
  `period_kind` источник правды. Заполнено для всех 21 Ozon + 13 WB + test-provider
  (по сверке с API: 6 Ozon строго месячные, accrual_by_day — цикл по дням, диапазонные,
  без даты).
- **DTO**: `ReportInfo` += period_kind/description; проброс в app-loop.
- **Виджет** `widgets/interval_picker.rs`: SpinButton года + StackSwitcher(Неделя/
  Месяц/Квартал/Год) + Stack из FlowBox-сеток (один клик → chrono-расчёт [from,to]).
  Кнопка «📅 Интервал» в download.rs открывает его в popover, проставляет date_from/date_to.
- **download.rs**: month/year combos УБРАНЫ; период выводится из `date_from`.
  «Скачать по периоду»: для `Month` — **цикл по всем месяцам [date_from..date_to]**
  (квартал=3, год=12), date_to не теряется; для Range/Day/None — один запрос. Инфо-панель
  (бывшая mode_hint) показывает описание отчёта + период-ноту с числом месяцев.
- **Проверено**: workspace build + `cargo test` (0 failed) + sanity-launch (рендерится,
  avg=43, black_fraction=0%). Известный край: 4 диапазонных Ozon-отчёта с капом ≤31 дня
  (balance/buyout/placement_*) — интервал >31д одним запросом упадёт 4xx (не оконную в
  этом шаге). Уроки #45-46.
- **Архив (доп.)**: во вкладке «Архив» month/year combos также заменены виджетом
  интервала. `ListArchive`/`ArchiveState` несут `date_range` вместо `period`. Предикат
  каталога изменён с «пересечения» на «дата НАЧАЛА или КОНЦА отчёта попадает в [f,t]».
  Кнопка «✕ Дата» сбрасывает фильтр. Баг SQL-скобок (OR без outer parens) пойман тестом
  — урок #47. Урок #48 (архив).

**Структура вкладок GUI (sidebar):** Магазин → Отчёты → Загрузка → Архив →
Настройки → Планировщик → Журнал → **Справка** → О программе. (F1 = Справка.)

**Чат 2026-08-13 (UX-пакет: помощь, диалоги, фиксы; коммиты 973c6f7…1c09ce4 + 432c9eb):**
- **fix: размножение кнопки «📁 Открыть папку»** (973c6f7). Очистка старой кнопки
  искала `gtk4::LinkButton`, а создаётся `gtk4::Button` (наследие фикса «двойного
  проводника») → кнопки плодились после каждой загрузки, растягивая окно. Фикс:
  фильтр ищет `gtk4::Button`; удаляются ВСЕ кнопки-дети result_box, добавляется одна.
- **Вкладка «Справка»** (3ef8351): подробное руководство для пользователей (9 разделов:
  быстрый старт, Магазин/ключи, Загрузка/интервалы, Архив, Планировщик, Журнал,
  Настройки, хранение данных, FAQ 401/403-429-пустой отчёт-FBS-профили). Self-contained
  (Labels+Pango, без внешних файлов — работает в бандле). `ViewId::Help`; пункт меню
  + F1 (`app.help` + accel); `show_view_in_window()` для app-действий.
- **Кнопка выбора папки в Настройках** (ced3e9e): «📁» у «Папка выгрузки» →
  `FileChooserDialog` (SelectFolder, модальный) → путь в поле. Стартовая папка —
  родитель текущего значения, иначе Документы.
- **Пояснения в Планировщике** (d2a9d70): пошаговая инструкция в форме, tooltip на
  каждое поле, расшифровка cron, пояснение периода, tooltip'ы «вкл»/«▶»/«🗑».
- **Контекстная помощь на КАЖДОЙ вкладке** (d3328b0): новый виджет
  `widgets/tab_help.rs` — `title_row_with_help(заголовок, css, blocks)`: заголовок + «?»
  справа → popover со справкой (HelpBlock::H/T/B, скролл 560px). Подключён к 7 вкладкам,
  у каждой свой материал (const `*_HELP: &[HelpBlock]` в каждом view).
- **Планировщик: диалог вместо cron-цифр** (1c09ce4): поле Cron и «Период» (ручной
  ввод «0 2 1 * *»/«-1») ЗАМЕНЕНЫ. Кнопка «Когда…» показывает расписание человеческим
  текстом (`describe_cron()`: «1-го числа каждого месяца, 02:00») и открывает диалог:
  Как часто (Ежемесячно/Еженедельно/Ежедневно) → Какого числа (1–28) / В какой день
  (Пн…Вс) → Во сколько (spin ч:м) → Выгружать (прошлый/текущий/позапрошлый месяц).
  Лишние строки скрываются по частоте. Диалог собирает cron → скрытые W_CRON/W_PERIOD
  (add_schedule не менялся). Период в форме — combo с названиями. Пресеты/ручной ввод
  cron убраны.
- **GUI click-test harness** (в 973c6f7): `scripts/gui-test/` — shot.ps1 (DPI-aware
  скриншот), ocr.ps1 (Windows OCR; UTF-8 stdout через `[Console]::OutputEncoding`,
  иначе cp866 и кириллица не матчится в bash!), click.ps1 (SendInput клик/ввод),
  focus.ps1 (SetForegroundWindow через AttachThreadInput+AppActivate — простой вызов
  из фонового процесса блокируется), run_interval_demo.sh (демо-флоу с trap-очисткой).
  ⚠️ Клики идут по РЕАЛЬНОМУ рабочему столу — запускать только когда он свободен
  (в чате live-демо остановлено: клик едва не попал в Excel юзера).
- **Кнопка «📄 Открыть файл» после загрузки** (432c9eb): в «Загрузке» после завершения
  выгрузки рядом с «📁 Открыть папку» — «📄 Открыть файл» (первый скачанный файл,
  ассоциированным приложением; «(первый из N)» + tooltip со всеми путями при
  нескольких). Обе кнопки очищаются перед добавлением — размножения нет.
- **Актуализация setup**: MDWFSetup-1.4.0.exe пересобран после интервального виджета
  (Aug 13 10:32). ПОСЛЕ этого сделаны: справка, кнопка папки, фикс «Открыть папку»,
  tab-help, диалог планировщика, «Открыть файл» — инсталлер СТАРЫЙ, при релизе
  пересобрать (build-setup.cmd).
- Уроки #49-51 (см. ниже).

**Чат 2026-08-13 (человекочитаемые расписания в Планировщике; коммит e63223b):**
Жалоба юзера: в списке созданных расписаний — «непонятные цифры» (сырой cron),
непонятно что/как/когда будет выполняться.
- **Строка расписания → карточка с описанием.** Вместо колонки с `cron_expr`
  монозаписью — человекочитаемое предложение: «Выгружать «Отчёт по реализации»
  (за прошлый месяц), 1-го числа каждого месяца, 02:00». Сырой cron и техданные
  (профиль • след. запуск • статус • cron) — в приглушённой dim-строке + tooltip.
- **`describe_period(offset)`** (новый): period_offset → «за прошлый месяц» / «за
  текущий» / «за позапрошлый» (± фоллбэки). Раньше `period_offset` вообще НЕ
  отображался в строке — пользователь не видел, за какой месяц выгрузка.
- **`describe_schedule(s)`** (новый): объединяет ЧТО (report_names: «…» если один,
  через запятую если несколько) + ЗА КАКОЙ ПЕРИОД + КОГДА (`describe_cron`,
  переиспользован — уже применялся для кнопки «Когда…» диалога).
- Тесты (5) на чистые хелперы: период/расписание для месяц/неделя/день, множественные
  отчёты (без кавычек), пустой отчёт. `cargo test --workspace` — 0 failed.
- ⚠️ **Найдено: GUI-крейт красный по clippy (`-D warnings`)** — ~20 предсуществующих
  предупреждений от UX-пакета (needless `&` в `title_row_with_help`-вызовах всех view,
  `map().unwrap_or()`, `_instance` used_underscore_binding, complex types в
  interval_picker, empty-line-after-doc-comment в archive). Эта правка clippy-чистая
  (ни одна ошибка не на новой функции), но мастер был красен ДО неё. Отдельная задача —
  почистить. Урок #52.

**Чат 2026-08-14 (сеньор-ревью + закрытие P0-багов; коммит 81b2ece):**
По просьбе юзера проведён независимый сеньор-анализ архитектуры и кода (не
опираясь на самодокументирование этого файла). Из ~19 kLOC / 12 крейтов найден
ряд конкретных багов. По итогам закрыты 4 P0-бага (бэклог отдельно: WB-аудит,
event-channel, record_download id, версионирование миграций, runner-тесты).
- **Bug #1 — cron в UTC vs локальный UI**: GUI задаёт «02:00» локально, а
  `next_run` считал cron в UTC → для MSK срабатывало в 05:00. Фикс: `next_run`
  трактует поля в `chrono::Local`, возвращает UTC-инстант (инвариант
  `claim_schedule` — RFC3339-UTC лексикографически — сохранён; все 8 callers
  не менялись). Generic `next_run_in<Z: TimeZone>` для детерминированных тестов
  (FixedOffset, не Local — иначе флапают от таймзоны машины). Новый `pub
  fmt_local()` показывает `next_run_at` юзеру в локальном времени (GUI + CLI).
- **Bug #2 — circuit breaker мёртв** (`post_url` звал on_success/on_failure, но
  НЕ `check()` → предохранитель считал отказы, не размыкая запрос): `self.breaker
  .check()?` в начале цикла (half-open после cooldown). Интеграционный тест
  `breaker_blocks_after_threshold` (wiremock; новый `with_base_url_and_retry`
  с tiny RetryPolicy — тест идёт мс, а не ~8с экспоненты).
- **Bug #3 — RateLimiter.backoff терял штраф** (`acquire` перезаписывал `last`,
  `duration_since(future)` насыщалось до 0 → штраф 30с→20мс; `backoff`
  присваивал вместо `max`): переписан на reservation-алгоритм (слот =
  `last+interval`; `backoff` через `max`) — конкурентные запросы (лимитер за
  `Arc`) теперь разносятся во времени. 2 новых async-теста (прежде — 0).
- **Bug #4 — PDF сохранялся как .csv** (`detect_format` не знал `%PDF` →
  `b2b_sales` падал в else→csv): ветка `b"%PDF"`→pdf; downstream уже поддерживал.
- **Тесты**: +11 (cron×7, limiter×2, breaker integration×1, pdf×1).
  `cargo test --workspace` — 0 failed. clippy затронутых крейтов (scheduler +
  ozon) чист (`FixedOffset::east` deprecated → `east_opt()`).
- **Не вошло (бэклог)**: живой WB-аудит (главный фронт), event-channel потеря
  `UiEvent`, `record_download` stale-id на upsert, версионирование миграций
  (`PRAGMA user_version`), runner-тесты claim/should_run, GUI-clippy-долг (~20).
  Урок #53.

**Чат 2026-08-14 (UX-аудит + Quick wins; коммиты cb26519…09f7aac):**
Сеньор-разбор всех 9 вкладок глазами обычного пользователя (бухгалтер/селлер,
не разработчик). Главный диагноз: сквозь UI просачивался жаргон разработчика,
а на длинных выгрузках нет прогресса/отмены. Закрыты 7 Quick wins (дёшево,
высокий эффект); средние (#8 прогресс+отмена, #9 first-run, #10 выбрать всё,
#11 валидация настроек, #13 группировка вкладок) — в бэклоге.
- **QW1 — type_id спрятан**: списки «Отчёты» и комбик «Загрузки» показывают
  только display_name (раньше «ozon.realization — Отчёт о реализации»);
  type_id — в tooltip «Идентификатор».
- **QW2 — категории по-русски**: английский `{:?}`-enum (Finance/Penalties…) →
  «Финансы/Штрафы/…». Новый `ReportCategory::display_ru()` в ядре.
- **QW3 — кварталы/недели**: «Q1»→«1 кв.»; tooltip недели показывает даты
  «Неделя 37 (09.09–15.09)». (Полный переход дат на ДД.ММ.ГГГГ — к средним,
  сквозная правка календаря/парсинга.)
- **QW4 — человекочитаемые ошибки**: `friendly_error()` в app.rs маппит
  CoreError → причина+подсказка (401/403→«ключ истёк, перевыпустите и обновите
  профиль»; 429→«подождите 1–2 мин»; сеть→«проверьте интернет»; 5xx→«сбой на
  стороне маркетплейса»). Покрывает UI (notify) и Журнал (общая строка).
- **QW5 — единый глагол «скачивание»**: убрана путаница скачать/загрузка/
  выгрузка. Статус/журнал/ошибки/Справка/tab_help — синхронно (вкладку
  «Загрузка» как имя раздела оставил).
- **QW6 — форма профиля без жаргона**: маркетплейс в диалоге был сырыми id
  «ozon»/«wildberries» → «Ozon»/«Wildberries» (append(id,display_name)+
  active_id(), 4 точки согласованы). Поля: «Client-Id»→«Номер кабинета
  (Client-Id)», «Api-Key»→«API-ключ».
- **QW7 — расширения как типы**: xlsx→«Excel», pdf→«PDF» (хелпер ext_label),
  вместо сырых расширений в списках.
- Тесты зелёные. Урок #54.

**Чат 2026-08-14 (UX medium-tier + точечные фичи/фиксы; коммиты 18618d9…0db870f):**
- **Редактирование расписания** (кнопка «✎»): name + cron + период; report/profile/
  enabled сохраняются. `Catalog::update_schedule(id,…)` + `UiCommand::UpdateSchedule`;
  `show_when_dialog` стал параметрическим (переиспользуется формой и диалогом изменения).
- **Клик по отчёту → переход в «Загрузку»** (как обещает помощь): прежде только tooltip.
  `download::set_pending_report` + переключение вкладки через подъём до Stack.
- **#8 Отмена**: кнопка «⏹ Отмена» → `cs.cancel_current()` (работает несмотря на
  блокировку loop'а — токен в общем Arc). Поле `cancel` в Download/ListDocuments,
  проверки `is_cancelled()` в poll-loop Ozon. (Пагинационные sweeps/WB-листинг токен
  получают, но циклы пока не опрашивают — следующий шаг.) Прогресс-бар (#8 часть 1)
  — `UiEvent::Progress.fraction` прежде выбрасывался.
- **#9 First-run**: empty-state «📋 Профилей пока нет…» в «Магазине».
- **#10 «Выбрать всё»/«Снять»** над списком документов WB.
- **#11 Валидация настроек**: разумные границы SpinButton (было 1..100000 везде) +
  шаблон имени файла (пустой → отказ, нет `{ext}` → предупреждение).
- **[id]-суффиксы убраны**: «Ozon [ozon]»→«Ozon» в главных combos через `active_id()`
  (заодно упростил резолв — убрал парсинг «Display [id]»).
- **fix регрессии QW6**: `on_auth_fields_loaded` читал `active_text()` (стал
  display_name) → поля диалога добавления не отрисовывались; переведено на `active_id()`.
- Урок #55.

**Чат 2026-08-14 (закрытие хвостов; коммиты e538cfd…5fd5fa7):**
- **Редактирование профиля** («✎ Изменить»): предзаполнение несекретных полей;
  секретное — пусто («оставьте пустым, чтобы не менять» — keyring не затирается,
  store_profile_secrets не трогает пустые). upsert_profile UPDATE по id.
- **clippy-долг GUI = 0** (--fix 15 + вручную: SelectFn alias, floating doc,
  instance). Теперь весь workspace чист под `-D warnings`.
- **Пагинация-cancel**: is_cancelled() во всех sweep-циклах OzonPaginatedReport
  (CashFlow/Offset/по-дням/SKU-батчи+retry/ReturnsList/AccrualPostings) и в
  листинге WB /documents/list.
- **Группировка вкладок**: StackSidebar → кастомный ListBox (.navigation-sidebar)
  с секциями «Магазин и выгрузка» / «Автоматизация» / «Прочее»; двусторонняя
  связка со стеком без циклов.
- **ДД.ММ.ГГГГ** (хвост UX): пользователь видит/вводит ДД.ММ.ГГГГ, в API/БД — ISO.
  Хелперы disp_date/parse_date_flex/to_iso; поля Загрузки/Архива, календарь,
  интервал-пикер, списки документов, колонки архива, restore; DATE_RANGE хранит ISO.
- ⚠️ Инсталлер не пересобран с UX-пакетом — перед выдачей build-setup.cmd.

**Чат 2026-08-14 (закрытие P1 + P2 целиком; после клик-теста):**
- **P1-a потеря UiEvent**: forward() жертвует только Progress при переполнении
  bounded(256); терминальные — ретрай 100×2мс (раньше DownloadFinished мог
  молча пропасть → «вечное скачивание…»).
- **P1-b stale-id upsert**: record_download → RETURNING id (bundled SQLite≥3.35).
- **P1-c ReturnsList**: защита от зависшего курсора (id непарсим → останов,
  а не 200 страниц дублей).
- **P1-d оконная разбивка**: ReportDescriptor.max_range_days (balance=30,
  buyout/placement_*=31) + GUI режет длинные интервалы на окна ≤ капа
  (цикл с общим cancel-токеном). Год ≈ 12-13 окон.
- **P1-e FBS auto-fill**: /v4/posting/fbs/list (v3 отключается 31.08.2026!),
  cursor-пагинация, сверен с докой. auto-fill = FBO + FBS объединённо.
- **P2-1**: PRAGMA user_version (=4, warn при БД>код); 4 Runner-теста
  (claim-атомарность, due-фильтры, NULL→pending, failed→status+next) — прежде 0.
- **P2-2 мёртвый код удалён**: pagination.rs ×2 (Ozon Pages/Cursor/Offset +
  WB TaskId/RrdidCursor/DateCursor/OffsetLimit), WbDomain::burst(),
  ResponseShape::Report, UiCommand::Cancel (+arm), period_to_range.
- **P2-3**: CoreError::Protocol (2xx-не-JSON, нет result/code) ≠ Internal(=баг);
  CLI: Protocol→ApiError.
- **P2-4**: xlsx числа ЧИСЛАМИ (write_cell parse f64) + интернер вместо Box::leak;
  file_store атомарно (tmp+rename); TestProvider только debug; warehouse cap 200.
- **P2-5**: keyring в spawn_blocking (tokio из dev→deps в secrets); вместо
  мега-рефакторинга «таблица 4→1» — тест-страж descriptors_and_dispatch_are_in_sync
  (дрейф таблиц ловится тестом). Полная унификация таблиц — осознанно отложена.
- cargo test --workspace exit=0; clippy -D warnings exit=0 по ВСЕМ крейтам.
  Уроки #56 (cargo test ≠ build).

**Чат 2026-08-14 (symbolic-иконки + финальная унификация таблицы):**
- **Таблица отчётов 4→1**: единая static REPORT_DEFS (21 ReportDef: дескриптор-данные
  + Dispatch::Direct/Async/Paginated(endpoint,kind)); all_report_descriptors() и
  make_report() ГЕНЕРИРУЮТСЯ из неё (прежде 2×21 дублированных arm; реальный дрейф:
  «Декомпенсации» vs «Декомпенсации (штрафы/антифрод)»). Матчами остались только
  логические build_download_body/xlsx (под тестом-стражем). ReportCategory/PaginationKind
  → Copy. −250 строк.
- **Symbolic-иконки вместо эмодзи** (монохром, темизация, DPI-масштаб; бандл
  Adwaita/symbolic офлайн): хелперы icon_button/icon_only_button/icon_label_child;
  все кнопки Загрузки/Магазина/Расписаний/Архива. icon-only — только строки списков
  (tooltip обязателен). gtk4 feature v4_6 (MenuButton::set_child).
- Живая OCR-верификация: магазин (+ Добавить/Изменить/Проверить/Удалить), Загрузка
  (все 6 кнопок), Расписания — тексты подтверждены, скриншоты dist/_uitest/win6-8.

**Чат 2026-08-14 (выделение активного пункта sidebar):**
- Жалоба: активный пункт сливался с заголовками секций. Две причины: заголовки
  были жирные heading (громче пунктов) → переведены на dim-label (иерархия
  «секция тише пункта»); штатная selection-подложка navigation-sidebar едва
  заметна (~5% белого).
- CssProvider-путь проигран эмпирически (5 вариантов, см. урок #57) → рабочий
  фикс: `mark_active` ставит лейблу активной строки css-классы
  `["heading", "accent"]` (жирный + акцентный цвет), остальным — сброс.
  Вызывается из stack-notify «visible-child» (клик И программное переключение)
  и при старте («shop»).
- Верификация живым клик-тестом: клик «Отчёты» → акцент-кластер (пиксель-скан
  R−G>60) один и сместился ровно на шаг строки; vision-проверка кропа: «Отчёты»
  розовый жирный, «Магазин» сброшен, выделен ровно один пункт.
- cargo build/clippy(-D warnings)/test(5) — зелёные. Урок #57.

**Чат 2026-08-14 (инсталлер + живой WB-аудит; коммит 06b04cc):**
- **Инсталлер пересобран** (`build-setup.sh` → `MDWFSetup-1.4.0.exe` 26 МБ,
  16:05, бинари 16:04 > HEAD 7b71524 16:00): UX-пакет+symbolic+sidebar в
  выдаче. После WB-фиксов пересобран ЕЩЁ РАЗ (актуален для выдачи).
- **WB-аудит** (приоритет §0-2): (1) сверка ВСЕХ эндпоинтов с официальной
  OpenAPI-спекой через зеркало eslazarev/wildberries-sdk (specs/*.yaml —
  субагент скачал и разобрал 14 файлов); (2) baseline живой прогон старым
  кодом (5 отчётов снято: orders/sales/balance/list OK, **detailed = ровно
  1000 строк** — evidence тихой обрезки; прогон остановлен при начале правок,
  остальные 7 предсказаны спекой); (3) фиксы (06b04cc); (4) повторный живой
  прогон — **12/12 OK** + доп. проверки пустых (claims август, acceptance
  август, antifraud без фильтра).
- **Найдено и исправлено (все — по спеке, до фикса не работали или теряли данные):**
  - **detailed-финансы** (sales-reports/acquiring): offset:0+limit:1000 →
    rrdId-курсор (limit 100000), конец по 204; клиент 204 → `[]` (прежде —
    ложная Protocol-ошибка). Июль: 1000 → **3217 строк** (8.46 МБ).
  - **claims**: обязателен `is_archive` (400 без него); дат НЕТ (окно 14 дней);
    limit≤200; ответ `{claims,total}`; два среза (активные+архивные) +
    локальный фильтр по `dt`.
  - **acceptance_report**: был POST {limit,offset} (не по спеке) → полная
    реализация GET create→taskId→poll status (done/error/таймаут 10 мин)→
    download (204=нет данных); `max_range_days=31`.
  - **удержания**: dateTo (концом дня) и limit=1000 обязательны; offset до total.
  - **статистика**: только dateFrom (по lastChangeDate!)+flag=0; dateTo/limit
    убраны; описания дескрипторов честно отражают семантику.
  - **antifraud**: только date → выгрузка всего + локальный фильтр пересечения.
  - **period YYYY-MM** → конец месяца (`+1 месяц −1 день`; прежде `+30 дней`
    ломал февраль).
  - Рефакторинг: `HttpMethod`+`ResponseShape` → **`WbQueryKind`** (метод+
    параметры+пагинация в одном enum); `fetch_all` обходит страницы для
    list() И download(), прогресс постранично, отмена между страницами.
- **Тесты**: +7 unit, +5 wiremock-integration (rrdId+204, claims-срезы,
  обязательные параметры удержаний, acceptance-флоу, дат-параметры
  статистики). cargo test --workspace = 23 сюита OK; clippy -D warnings = 0.
- Живой итог по отчётам: orders 708КБ / sales 279КБ / balance 68Б (плоский
  объект — спека подтверждена) / list 6 строк / **detailed 3217** /
  acquiring [] / deductions [] / measurement [] / antifraud [] / acceptance
  [] (флоу завершён, данных нет) / claims [] (заявок нет, запросы валидны).
  Пустые != сломано — проверены доп. прогонами (урок #58).
- Урок #58.

**Чат 2026-08-13 (фикс чёрного окна + брендовая иконка запущенного окна) — PROD-качество:**
Жалоба юзера: «чёрное окно после запуска» + «вижу только дефолтную иконку» + «мне
нужен прод». Диагностика ОБЪЕКТИВНАЯ (пиксель-анализ скриншота через PowerShell
`System.Drawing`, т.к. vision-инструмент URL с Windows-путём не парсит):
- **«Чёрное окно» — это была КОНСОЛЬ терминала, не рендеринг GTK!** Сначала
  неправильно понял (искал баг GTK, добавил `GSK_RENDERER=gl` — оставил как
  отдельную полезную закалку, но НЕ та проблема). Реальный фикс: `#![windows_subsystem
  = "windows"]` в `main.rs` (release) → exe становится GUI-subsystem, консоль не
  аллоцируется. Проверка PE: `Subsystem 0x2 (Windows GUI)`. Урок #41.
- **Чёрное GTK-окно на чистой машине** (отдельная закалка, не жалоба юзера): дефолтный
  NGL-рендерер падает на некоторых GPU-драйверах. Фикс: `GSK_RENDERER=gl` в
  `setup_bundle_env` (overridable). Проверка: установленное приложение — `avg=163.7`,
  `black_fraction=0%`. Урок #38.
- **Брендовая иконка**: exe-иконка (winres) была вшита правильно (есть 256×256), но
  иконка ЗАПУЩЕННОГО окна не выставлялась (`WM_GETICON=0`). Фикс:
  `gtk4::Window::set_default_icon_name("mdwf")` + ships как файлы в on-disk теме
  hicolor бандла (`build-release.sh` кладёт `share/icons/hicolor/<size>x<size>/apps/mdwf.png`,
  регенерит `icon-theme.cache --force`). Проверка: `has_icon(mdwf)=true`, `WM_GETICON≠0`.
  gresource-путь (`add_resource_path`) на Windows НЕ работает (verified has_icon=false
  при всех паттернах) — см. уроки #39, #40.
- **`make-icon.sh`** теперь генерит и disk-PNG (синхрон с `.ico`).
- **Single-instance + `AppMutex`** (отдельная просьба юзера): named mutex
  `MDWF_App_Mutex` через крейт `single-instance` (forbid(unsafe_code) → обёртка) в
  `main.rs` ДО `adw::Application` + `AppMutex=MDWF_App_Mutex` в `.iss`. Второй запуск
  GUI → диалог «уже запущен» (`glib::MainLoop`); Inno не ставит поверх запущенного.
  Проверено: mutex held (OpenMutex), второй инстанс→exit 124 (диалог). Урок #42.
- **Итог**: установленное через настоящий `setup.exe` приложение рендерится (0%
  чёрного) и показывает брендовую иконку. Обе претензии закрыты на реальной установке.
  Уроки #38-40.

**Чат 2026-08-12 (проверка `setup.exe` на чистой Windows) — ВЕРИФИКАЦИЯ ЗАВЕРШЕНА, бандл ГОТОВ:**
- **Методология**: два тест-скрипта симуляции чистой машины. `scripts/test-clean-env.sh`
  запускает бандл через `env -i` с PATH=только System32 (вырезано msys64/mingw),
  сняты все `GTK_*`/`GDK_*`/`XDG_*`/`GSETTINGS_*` — дев-машина иначе ВРЁТ (GTK стоит
  глобально). `scripts/test-installed.sh` воспроизводит flow инсталлера (копирование
  в путь с ПРОБЕЛОМ → `postinstall.bat` → запуск). Уроки #35-37.
- **Результаты (всё зелёное)**:
  - ✅ Бандл стартует без MSYS2 (`timeout` → exit 124) — **DLL-набор полон**, ничего
    не недостаёт. Единственный warning — безобидный `win32 session dbus binary not found`.
  - ✅ Настоящий `MDWFSetup-1.4.0.exe /VERYSILENT` в путь с пробелом: install-лог Inno
    подтверждает копирование файлов + `[Run] postinstall.bat` → **`Process exit code: 0`**.
  - ✅ `loaders.cache` после реальной установки — **относительные пути**
    (`lib\gdk-pixbuf-2.0\...\*.dll`), бандл по-настоящему relocatable (работает в любом пути).
  - ✅ Установленное приложение стартует без MSYS2 + делает **живой API-вызов**
    (`POST /v1/seller/info` к Ozon) — т.е. UI полностью построился.
  - ✅ CLI `mdwf.exe` в бандле: `providers list` (3 провайдера) + `reports list`
    (21 Ozon / 13 WB) — exit 0.
  - ✅ **Деинсталлятор** `unins000.exe /VERYSILENT`: exit 0, каталог очищен полностью.
- **Главный инсайт тестирования**: «мгновенный чистый exit (0.9с, без окна, без ошибки)»
  оказался **артефактом теста**, не багом — `timeout` в MSSYS не убивает native GUI
  (зомби), а `gtk::Application` с фиксированным APP_ID форвардит `activate` на живой
  зомби-primary и сразу выходит. Лечится `taskkill //IM mdwf-gui.exe //F` перед прогоном
  (встроено в тест-скрипты). Уроки #36, #37.
- **Скриншоты** установочного прогона: `dist/_clean_test/screen*.png` (на диске; vision-
  инструмент URL с Windows-путём не парсит — можно открыть глазами). Процессные
  доказательства рендеринга: app доходит до API-вызова из UI-сетапа, gdk-pixbuf-лоадеры
  валидны, gresource-иконки вшиты в бинарник, Adwaita+схемы в бандле.
- **⚠️ Наблюдение (не баг, продукт-решение)**: инсталлер требует прав админа
  (`PrivilegesRequired` по умолчанию → `{autopf}` = Program Files). У MDWF НЕТ системных
  компонентов (keyring/config/scheduler — все per-user), поэтому per-user install
  (`%LOCALAPPDATA%\Programs`, без админа) был бы дружелюбнее и разрешал бы тихую установку
  без UAC. На усмотрение пользователя — переключить при необходимости
  (`PrivilegesRequiredOverridesAllowed=commandline dialog` + `/CURRENTUSER`).

**Что уже проверено живым прогоном (Ozon, oz_prof1):**
- ✅ **18 из 21** отчётов Ozon скачиваются корректно простым `--period` (без ручных
  параметров): все auto-fill'ы (`analytics_stocks` SKU, `warehouse_stock` склады,
  `accrual_postings` отправления) делают GUI-нерабочие ранее отчёты доступными.
- ⚠️ `warehouse_stock` — у аккаунта нет FBS-складов (FBO-схема), отчёт неприменим.
- ✅ `returns` — полный отчёт (`/v1/returns/list`, все статусы + FBO/FBS).
- ✅ `accrual_postings` — auto-fill posting_numbers (`/v2/posting/fbo/list`), 665→2300 строк.
- ❌ `b2b_sales`/`postings` — перепроверены, честно НЕ наш баг (нет B2B-документа /
  Ozon-side генерация падает).

**Живые API-тесты теперь РАЗРЕШЕНЫ пользователем** (раньше — под запретом).
Профили `oz_prof1` (Ozon) и `wb_prof` (WB) созданы, ключи валидны (health_check OK).

**Возможные дальнейшие задачи (на усмотрение пользователя):**
1. **Аудит WB API** (по аналогии с Ozon) — живой прогон всех 13 отчётов WB, сверка с
   dev.wildberries.ru / eslazarev/wildberries-sdk. Главный неисследованный фронт.
2. **Серверные ошибки Ozon** (postings — Ozon-side генерация; b2b_sales — неприменим,
   нет B2B-документа). Перепроверены этим чатом — честно не наш баг.
3. **FBS-поддержка для auto-fill**: accrual_postings сейчас FBO-only
   (`/v2/posting/fbo/list`). Для FBS-аккаунтов добавить `/v3/posting/fbs/list`.
4. **Доработки из известных проблем** (секция 11): async-отчёты WB (acceptance_report),
   и др. (max_parallel_jobs починен, Журнал/Планировщик сделаны).
5. **GUI-клик-тест Журнала/Планировщика**: добавить расписание с cron «через минуту»,
   дождаться фоновой автозагрузки, проверить запись в Журнале.
6. ✅ **Проверка `setup.exe` на чистой Windows** — ВЫПОЛНЕНО этим чатом (бандл полон,
   инсталлер/deинсталлятор работают, relocatable). Детали выше. Возможное улучшение:
   **per-user install без админа** (MDWF не имеет системных компонентов).

**⚠️ Перед сборкой:** закрыть запущенный GUI (exe блокирует линковку на Windows).

**Живые тесты API разрешены, но уточнять при сомнениях. Не придумывать API — сверять
с первоисточником (урок №22).**
