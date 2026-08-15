# Marketplace Downloader Framework (MDWF) v1.4

Кросс-платформенное desktop-приложение на Rust для автоматизированной выгрузки
финансовых документов с маркетплейсов (**Ozon**, **Wildberries**) через их
официальные API. GTK4 + libadwaita GUI, CLI, cron-планировщик, опциональный REST API.

> **Принцип «только официальное API»** (спец. ADR-003): cabinet scraper исключён.
> Документы без API помечены `out-of-scope` (скачиваются вручную).

---

## Возможности

- **Два режима получения документов:**
  - *Browsable*: список → фильтр (категория/дата) → выбор чекбоксами → скачивание.
    (WB Documents API: УПД/УКД/акты; Ozon transaction-list/accrual/b2b-sales/...)
  - *Period*: тип отчёта + период → генерация → скачивание.
    (Ozon realization/compensation, WB sales-reports/detailed, ...)
- **GUI** на GTK4 + libadwaita: 9 разделов (Магазин, Отчёты, Загрузка, Архив, Настройки, Расписания, Журнал, Справка, О программе).
- **CLI** `mdwf`: providers / profiles / reports / download / schedule / out-of-scope / doctor.
- **Планировщик** с cron (ежемесячно/ежедневно/еженедельно) + автозапуск с Windows.
- **Идемпотентность**: дедупликация выгрузок по SHA-256 (SQLite UNIQUE-индекс).
- **Безопасность**: секреты в Windows Credential Manager (OS keychain), маскирование в логах.
- **Журнал событий**: выгрузки/ошибки/расписания с источником (вручную, CLI,
  расписание) — персистится в SQLite и переживает перезапуск.
- **Надёжность**: retry policy (429/5xx), circuit breaker, rate limits по доменам WB.

## Покрытие API

| Маркетплейс | Отчётов через API | Out-of-scope |
|-------------|-------------------|--------------|
| Ozon        | 21                | 5 (УПД с доп.услугами, отчёты партнёров, обеспечительные платежи, счета, акты сверки) |
| Wildberries | 13 + Documents API| 3 (акты сверки, счета, договоры) |

---

## Требования к среде (Windows 11)

1. **Rust** (gnu-тулчейн): `stable-x86_64-pc-windows-gnu` (GTK-библиотеки MinGW-сборки).
2. **MSYS2** с пакетами GTK4/libadwaita в `D:\msys64\mingw64`:
   ```bash
   pacman -S mingw-w64-x86_64-gtk4 mingw-w64-x86_64-libadwaita pkgconf
   ```
3. Git (для разработки).

## Быстрый старт (сборка и запуск)

```bash
# Подготовить окружение (PATH, PKG_CONFIG_PATH, gnu-тулчейн).
source scripts/env.sh

# Собрать всё.
cargo build --workspace

# Запустить GUI.
cargo run -p mdwf-gui

# Или CLI.
cargo run -p mdwf-cli -- providers list
cargo run -p mdwf-cli -- reports list --provider ozon
cargo run -p mdwf-cli -- doctor
```

В PowerShell используйте `. .\scripts\env.ps1`.

## Конфигурация и данные

| Файл/папка | Расположение |
|------------|--------------|
| `config.toml` | `%APPDATA%\mdwf\config.toml` |
| SQLite-каталог | `%APPDATA%\mdwf\mdwf.db` |
| Файлы выгрузок | `%USERPROFILE%\Documents\MDWF\downloads\{provider}\{period}\` |
| Логи | `%APPDATA%\mdwf\logs\` |

Настройки редактируются во вкладке «Настройки» GUI или `mdwf doctor` (CLI).

## Использование CLI

```bash
# Профили (нужны для выгрузки).
mdwf profiles add --provider ozon --name "Ozon-1" --client-id 1234567 --api-key "SECRET"
mdwf profiles add --provider wildberries --name "WB" --token "WB_TOKEN"

# Проверить подключение.
mdwf profiles check "Ozon-1"

# Список отчётов.
mdwf reports list --provider ozon
mdwf reports info ozon ozon.realization

# Выгрузка.
mdwf download --profile "Ozon-1" --report ozon.realization --period 2026-06

# Расписания.
mdwf schedule add --name "monthly" --profile "Ozon-1" \
  --report ozon.realization --cron "0 2 1 * *"
mdwf schedule list
mdwf schedule run
mdwf schedule autostart --enable

# Out-of-scope документы (недоступны через API).
mdwf out-of-scope --provider wildberries
```

Exit-коды — см. спец. §2.6.2 (0=OK, 4=AUTH, 5=NETWORK, 7=API, 64=OUT_OF_SCOPE, ...).

## Архитектура

Cargo workspace из 12 крейтов (спец. §2.4):

```
crates/
├── core/              # трейты, типы, реестр (провайдер-агностик)
├── storage/           # SQLite + FileStore + дедупликация SHA-256
├── secrets/           # OS keychain (Windows Credential Manager) + in-memory mock
├── scheduler/         # cron + автозапуск Windows (HKCU Run-ключ)
├── config/            # config.toml + пути к данным (dirs)
├── test-provider/     # TestProvider mock (для разработки GUI/CLI без реальных API)
├── providers/ozon/    # Ozon Seller API (21 отчёт, retry, circuit breaker)
├── providers/wildberries/ # Wildberries OpenAPI (Documents API, 5 доменов)
├── cli/               # mdwf (clap, 14 exit-кодов)
├── gui/               # mdwf-gui (GTK4 + libadwaita)
└── api/               # mdwf-api (REST, feature 'server', axum)
```

**Framework First** (спец. §1.3): ядро (`crates/core`) не упоминает маркетплейсы.
Добавление WB не требует правок core.

## Тесты

```bash
cargo test --workspace               # ~220 тестов
cargo test -p mdwf-providers-ozon    # 59 (включая mock-сервер wiremock)
cargo test -p mdwf-providers-wildberries  # 55 (5 доменов, 3-шаговый Documents API)
cargo test -p mdwf-cli --test e2e    # 7 E2E через CLI
```

## Сборка release-дистрибутива

Release-бинарник требует GTK-рантайм рядом с `.exe`. Скрипт `scripts/build-release.sh`
собирает release и копирует необходимые DLL из MSYS2:

```bash
# Release-бандл + инсталлер Inno Setup одной командой:
bash scripts/build-setup.sh          # из Git Bash/MSYS2
.uild-setup.cmd                    # или из PowerShell/двойным кликом
# Результат: dist/mdwf/ (relocatable-бандл ~100 МБ, все DLL)
#          и installer/Output/MDWFSetup-<version>.exe
```

## Ссылки

- Техническая спецификация: `MarketplaceDownloaderFramework_TechnicalDoc_v1.4_2026-07-10.md`
- Ozon Seller API: https://docs.ozon.ru/api/seller/ (локальная копия — docs/)
- Wildberries OpenAPI: https://dev.wildberries.ru/

## Лицензия

MIT OR Apache-2.0 — тексты в `LICENSE-MIT` и `LICENSE-Apache-2.0`;
сторонние компоненты дистрибутива — `NOTICE.md`.
