# Marketplace Downloader Framework — Technical Documentation

**Version:** v1.4
**Date:** 2026-07-10
**Language:** Russian
**Implementation language:** Rust (edition 2021)
**GUI framework:** GTK4 + libadwaita
**Document type:** Technical Documentation (In-Depth Research)

---

## Overview

### Назначение документа

Документ описывает архитектуру, требования и порядок разработки **Marketplace Downloader Framework (MDWF)** — кросс-платформенного desktop-приложения на Rust для автоматизированной выгрузки финансовых документов с маркетплейсов (Ozon, Wildberries, будущие платформы) через их официальные API.

### Целевая аудитория

1. Разработчики Rust, реализующие ядро фреймворка и провайдеры маркетплейсов.
2. Архитекторы, принимающие решения о расширении платформы.
3. ИИ-ассистенты (Claude, GPT, Gemini), ведущие разработку в паре с человеком.
4. Технические писатели, сопровождающие документацию.

### Предпосылки

Читатель должен владеть: языком Rust (edition 2021), асинхронным программированием (tokio), основами GTK4 и GObject, форматами TOML/JSON/SQLite, принципами REST API и OAuth2. Знакомство с Ozon Seller API и Wildberries OpenAPI желательно, но не обязательно.

### Структура документа

Документ состоит из 19 глав (00–18): главы 00–03 — концепция, цели, архитектура, структура проекта; главы 04–08 — слои приложения (GUI, CLI, Settings, Logging, Scheduler); главы 09–13 — ядро, контракт провайдера, реализации Ozon и Wildberries, модель отчётов; главы 14–16 — обеспечение качества; главы 17–18 — порядок разработки и будущие расширения.

### Соглашения документа

Тон — объективный и нейтральный. Имена команд, путей и параметров выделены моноширинным шрифтом (`code`). Действия описаны в повелительном наклонении («установите», «выполните», «проверьте»). Все шаги воспроизводимы. Терминология единообразна (см. глоссарий в §0.5).

---

## Opening Hook

Бухгалтеры и финансовые менеджеры компаний, торгующих на нескольких маркетплейсах одновременно, ежемесячно тратят от 4 до 8 человеко-часов на ручной сбор финансовых документов: вход в каждый личный кабинет, навигация по разделам, выбор периода, скачивание PDF/Excel, переименование файлов, складирование в сетевую папку. При числе маркетплейсов от двух и числе отчётов от пяти на каждый — вероятность человеческой ошибки (пропуск периода, перезапись файла, скачивание не того формата) приближается к 30%.

**Marketplace Downloader Framework (MDWF)** решает эту проблему, автоматизируя выгрузку всех документов, доступных через официальные API маркетплейсов. Документ определяет архитектуру фреймворка, спроектированного по принципу «только официальное API» (cabinet scraper исключён из-за нарушения ToS маркетплейсов), с GTK4-интерфейсом профессионального уровня и расширяемой плагинной архитектурой для добавления новых маркетплейсов без изменения ядра.

---

## 1. Background / Context

### 1.1 Предметная область

Маркетплейсы (Ozon, Wildberries, Яндекс Маркет и др.) предоставляют продавцам два способа доступа к финансовым документам:

1. **Личный кабинет** — веб-интерфейс, требующий ручных действий.
2. **Официальное API** — программный интерфейс для автоматизации.

Не все документы доступны через API. Часть документов (акты сверки, счета, договоры) можно получить только через личный кабинет. Автоматизация личного кабинета через браузер (cabinet scraper) запрещена условиями использования (ToS) маркетплейсов и может привести к блокировке аккаунта продавца.

### 1.2 Глоссарий

| Термин | Определение |
|--------|-------------|
| **Маркетплейс** | Платформа электронной торговли, предоставляющая продавцам API и личный кабинет (Ozon, Wildberries и др.) |
| **Провайдер** | Конкретная реализация трейта `MarketplaceProvider` для одного маркетплейса |
| **Отчёт** | Логическая единица данных, выгружаемая с маркетплейса через API |
| **Профиль** | Набор учётных данных для одного продавца на одном маркетплейсе |
| **Задача (Job)** | Единица работы планировщика |
| **Capabilities** | Самоописание провайдера: список поддерживаемых отчётов, типов авторизации |
| **Ядро (Core)** | Модули, не зависящие от конкретного маркетплейса |
| **Out-of-scope** | Документы, недоступные через API; пользователь получает их вручную |

### 1.3 Архитектурные принципы

1. **Framework First** — ядро не зависит от конкретных маркетплейсов. Код, упоминающий Ozon/Wildberries, находится только в `src/providers/<name>/`.
2. **Самоописывающиеся провайдеры (Capabilities)** — GUI, CLI и Scheduler строятся динамически из самоописания провайдера.
3. **Только официальное API** — cabinet scraper полностью исключён. Документы без API помечены out-of-scope.
4. **Явная обработка ошибок** — весь код возвращает `Result<T, E>`; `unwrap()`/`panic!()` запрещены вне тестов.
5. **Структурированное логирование** — через крейт `tracing` с поддержкой spans и полей.
6. **Локальность и безопасность данных** — все данные хранятся локально; секреты в OS keychain.
7. **Идемпотентность** — повторная выгрузка не создаёт дубликатов (дедупликация по SHA-256).
8. **Тестируемость** — внешние зависимости абстрагируются через трейты.

### 1.4 Архитектурные решения (ADR)

| ADR | Решение | Обоснование |
|-----|---------|-------------|
| ADR-001 | Rust + tokio | Производительность, типобезопасность; для финансовых данных критична корректность |
| ADR-002 | GTK4 + libadwaita для GUI | Нативные виджеты, профессиональный Adwaita-дизайн, доступность (ATK), retained-mode эффективен |
| ADR-003 | Только официальное API, без cabinet scraper | ToS маркетплейсов запрещают автоматизацию личного кабинета; риск блокировки аккаунта |
| ADR-004 | SQLite для каталога | Локальность, нулевая конфигурация, надёжность |
| ADR-005 | Feature-флаги вместо dyn loading | Простота сборки, отсутствие проблем с ABI |
| ADR-006 | thiserror для core, anyhow для app | Типизированные ошибки в публичном API; удобство в приложении |
| ADR-007 | Один tokio runtime | Проще reasoning, меньше ресурсов |
| ADR-008 | Один бинарник GUI + CLI | CLI встраивается в GUI как subcommand; shared code |
| ADR-009 | TOML для настроек, SQLite для каталога | TOML — человекочитаемый; SQLite — для запросов и индексов |

---

## 2. Main Body

### 2.1 Цели проекта (глава 01)

#### 2.1.1 Назначение продукта

MDWF — кросс-платформенное настольное приложение на Rust для автоматизированной выгрузки, нормализации и локального хранения финансовых и аналитических документов с маркетплейсов. Продукт ориентирован на малый и средний бизнес, ведущий торговлю через несколько площадок.

#### 2.1.2 SMART-цели

| № | Цель | Критерий достижения |
|---|------|---------------------|
| G1 | Поддержка Ozon Seller API | Все 20 отчётов из таблицы 11.1 реализованы и покрыты тестами |
| G2 | Полная автоматизация выгрузки API-доступных документов | Планировщик скачивает все отчёты за месяц без ручного вмешательства |
| G3 | Нулевой риск блокировки аккаунта | Только официальные API; cabinet scraper отсутствует |
| G4 | Добавление Wildberries без изменения ядра | `git diff src/core/` пуст после добавления WB |
| G5 | Кросс-платформенность | Сборка на Windows 10/11, Linux, macOS 12+ |
| G6 | GUI на основе Capabilities | Окно настроек строится динамически из самоописания провайдера |
| G7 | Производительность | Полная выгрузка за месяц < 90 секунд |
| G8 | Качество кода | Покрытие ≥ 80% core; clippy::pedantic без предупреждений |
| G9 | Расширяемость | Новый маркетплейс добавляется за ≤ 5 рабочих дней |
| G10 | Нулевой риск блокировки | Только официальные API; соблюдение rate limits |

#### 2.1.3 Нецели (Out of Scope)

- Бухгалтерский учёт (проводки, налоги, декларации)
- Аналитика продаж (встроенные дашборды)
- Управление ценами и остатками (read-only инструмент)
- Многопользовательский режим (v1.x — однопользовательский)
- Мобильное приложение
- Поддержка маркетплейсов вне СНГ в v1.x
- Обход защиты маркетплейсов (антидетект, прокси, обход капчи)
- **Cabinet scraper** — полностью исключён. Автоматизация личного кабинета нарушает ToS маркетплейсов.

### 2.2 Доступность документов через API

#### 2.2.1 Ozon Seller API

| Документ | Через API | Эндпоинт | Статус |
|----------|-----------|----------|--------|
| Отчёт о реализации (месячный) | ✅ | `POST /v2/finance/realization` | Stable |
| Отчёт о реализации (позаказный) | ✅ | `POST /v1/finance/realization/posting` | Beta |
| Отчёт о реализации (за день) | ✅ | `POST /v1/finance/realization/by-day` | Premium Plus/Pro |
| Выкупы маркетплейсом | ✅ | `POST /v1/finance/products/buyout` | Stable |
| УПД по выкупленным товарам (ЕАЭС) | ✅ | `POST /v1/finance/products/buyout` | Stable |
| Продажи юрлицам (PDF) | ✅ | `POST /v1/finance/document-b2b-sales` | Stable (Admin) |
| Продажи юрлицам (JSON) | ✅ | `POST /v1/finance/document-b2b-sales/json` | Stable (Admin) |
| Отчёт о взаиморасчётах | ✅ | `POST /v1/finance/mutual-settlement` | Stable (Admin) |
| Компенсации | ✅ | `POST /v1/finance/compensation` | Beta |
| Декомпенсации (штрафы/антифрод) | ✅ | `POST /v1/finance/decompensation` | Beta |
| Начисления по дням | ✅ | `POST /v1/finance/accrual/by-day` | Beta |
| Начисления по отправлениям | ✅ | `POST /v1/finance/accrual/postings` | Beta |
| Справочник типов начислений | ✅ | `POST /v1/finance/accrual/types` | Beta |
| Список транзакций | ✅ | `POST /v3/finance/transaction/list` | ⚠️ Deprecated → 6 июля 2026 |
| Итоги транзакций | ✅ | `POST /v3/finance/transaction/totals` | ⚠️ Deprecated → 6 июля 2026 |
| Баланс | ✅ | `POST /v1/finance/balance` | Beta |
| Финансовый отчёт (ДДС) | ✅ | `POST /v1/finance/cash-flow-statement/list` | Stable |
| Акт о расхождениях FBS (PDF) | ✅ | `POST /v1/carriage/act-discrepancy/pdf` | Stable |
| Аналитика | ✅ | `POST /v1/analytics/data` | Premium Plus only |
| УПД с доп. услугами | ❌ | Нет API; out-of-scope | — |
| Отчёты партнёров | ❌ | Нет API; out-of-scope | — |
| Обеспечительные платежи | ❌ | Нет API; out-of-scope | — |
| Счета на оплату | ❌ | Нет API; out-of-scope | — |
| Акты сверки | ❌ | Нет API; out-of-scope | — |

**Источник:** официальная PDF-документация Ozon Seller API v2.1 (665 страниц).

#### 2.2.2 Wildberries API

Сверено с актуальной официальной документацией: `dev.wildberries.ru/docs/openapi/financial-reports-and-accounting#tag=Dokumenty`.

| Документ | Через API | Эндпоинт | Статус |
|----------|-----------|----------|--------|
| Баланс продавца | ✅ | `GET /api/v1/account/balance` | Stable |
| Реестр реализации (список) | ✅ | `POST /api/finance/v1/sales-reports/list` | Stable |
| Детализация реализации (по периоду) | ✅ | `POST /api/finance/v1/sales-reports/detailed` | Stable |
| Детализация реализации (по ID) | ✅ | `POST /api/finance/v1/sales-reports/detailed/{reportId}` | Stable (BigInt) |
| Эквайринг (список) | ✅ | `POST /api/finance/v1/acquiring/list` | Stable |
| Эквайринг (детализация) | ✅ | `POST /api/finance/v1/acquiring/detailed` | Stable |
| Документы — список категорий | ✅ | `GET /api/v1/documents/categories` | Stable |
| Документы — список по категории | ✅ | `GET /api/v1/documents/list` | Stable |
| Документы — скачивание | ✅ | `GET /api/v1/documents/download` (base64 ZIP/XLSX/XML) | Stable |
| Документы — батч-скачивание (до 50) | ✅ | `POST /api/v1/documents/download/all` | Stable |
| УПД | ✅ | `category=upd` | Stable (forum/1602) |
| УПД (покупка у юрлица) | ✅ | `category=upd-purchase-from-legal` | Stable |
| УКД (продажа юрлицу) | ✅ | `category=sale-to-le-signed` | Stable |
| Уведомление о выкупе | ✅ | `category=redeem-notification` | Stable |
| Акт за МП-услуги | ✅ | `category=act-income-mp` | Stable |
| Аналитический отчёт приёмки | ✅ | `POST /api/v1/acceptance_report` (async) | Stable |
| Заказы (операционные) | ✅ | `GET /api/v1/supplier/orders` | ⚠️ Планируется deprecation |
| Продажи (операционные) | ✅ | `GET /api/v1/supplier/sales` | ⚠️ Планируется deprecation |
| Штрафы за подмены | ✅ | `GET /api/analytics/v1/deductions` | Stable |
| Штрафы за габариты | ✅ | `GET /api/analytics/v1/measurement-penalties` | Stable |
| Самовыкупы (антифрод) | ✅ | `GET /api/v1/analytics/antifraud-details` | Stable |
| Возвраты (claims) | ✅ | `GET /api/v1/claims` | Stable |
| Акты сверки | ❌ | Нет API; out-of-scope | — |
| Счета на оплату | ❌ | Нет API; out-of-scope | — |
| Договоры | ❌ | Нет API; out-of-scope | — |

**Динамическое обнаружение категорий WB:** категории (`upd`, `upd-purchase-from-legal`, `sale-to-le-signed`, `redeem-notification`, `act-income-mp`) возвращаются динамически через `GET /api/v1/documents/categories`. MDWF не хардкодит список, а получает их при старте, кэширует и обновляет при ротации токена.

### 2.3 Архитектура фреймворка (глава 02)

#### 2.3.1 Слоистая архитектура

```
┌─────────────────────────────────────────────────────────────────────┐
│  Слой представления (Presentation Layer)                              │
│  ┌─────────────────────┐  ┌──────────────────┐  ┌─────────────────┐  │
│  │       GUI (GTK4)    │  │     CLI (clap)   │  │   REST API *    │  │
│  │  динамически из     │  │  динамически из  │  │  (опционально,  │  │
│  │   Capabilities      │  │   Capabilities   │  │   feature flag) │  │
│  └──────────┬──────────┘  └────────┬─────────┘  └────────┬────────┘  │
└─────────────┼──────────────────────┼─────────────────────┼──────────┘
              │          Слой приложения (Application Layer)          │
│  ┌────────────────────┐  ┌──────────────────┐  ┌─────────────────┐  │
│  │   Scheduler        │  │   Job Runner     │  │  Profile Manager│  │
│  └─────────┬──────────┘  └────────┬─────────┘  └────────┬────────┘  │
│  ┌─────────▼──────────────────────▼─────────────────────▼────────┐  │
│  │                  Settings / Config Store                       │  │
│  │            (TOML + OS keychain для секретов)                   │  │
│  └────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────┬───────────────────────────────────────┘
                              │
┌─────────────────────────────▼───────────────────────────────────────┐
│                   Доменный слой (Domain Layer)                       │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐ ┌─────────┐ │
│  │ Marketpl.│  │ Provider │  │  Report  │  │Downloadr │ │  Auth   │ │
│  │ Provider │◄─┤  trait   │  │  trait   │  │  trait   │ │ trait   │ │
│  │ registry │  │          │  │          │  │          │ │         │ │
│  └────┬─────┘  └──────────┘  └──────────┘  └──────────┘ └─────────┘ │
│       │           (все трейты — в src/core/, без упоминаний маркетплейсов)│
└───────┼─────────────────────────────────────────────────────────────┘
        │
┌───────▼─────────────────────────────────────────────────────────────┐
│                Слой провайдеров (Provider Layer)                     │
│  ┌─────────────────────┐  ┌─────────────────────┐  ┌──────────────┐ │
│  │  providers/ozon/    │  │ providers/wildberr. │  │ providers/...│ │
│  │  OzonProvider       │  │  WildberriesProvider│  │  (будущие)   │ │
│  └─────────────────────┘  └─────────────────────┘  └──────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
        │
┌───────▼─────────────────────────────────────────────────────────────┐
│              Слой инфраструктуры (Infrastructure Layer)              │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────┐  │
│  │ reqwest  │ │ tokio    │ │ tracing  │ │ keyring  │ │   sqlite  │  │
│  │ (HTTP)   │ │ (async)  │ │ (logs)   │ │ (secrets)│ │ (catalog) │  │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘ └───────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

#### 2.3.2 Ключевые трейты

**`MarketplaceProvider`** — корневой трейт провайдера:

```rust
#[async_trait]
pub trait MarketplaceProvider: Send + Sync + 'static {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn version(&self) -> &'static str;
    fn docs_url(&self) -> &'static str;
    fn capabilities(&self) -> &Capabilities;

    async fn authenticator(&self, profile: &Profile) -> CoreResult<Arc<dyn Authenticator>>;
    async fn report_factory(&self, report_type: &str) -> CoreResult<Arc<dyn Report>>;
    async fn health_check(&self, auth: &dyn Authenticator) -> CoreResult<HealthStatus>;
}
```

**`Authenticator`** — абстракция авторизации:

```rust
#[async_trait]
pub trait Authenticator: Send + Sync {
    fn apply(&self, req: RequestBuilder) -> RequestBuilder;
    fn expires_at(&self) -> Option<DateTime<Utc>>;
    async fn refresh(&self) -> CoreResult<bool> { Ok(false) }
    fn auth_type(&self) -> AuthType;
    fn describe(&self) -> String;
}

pub enum AuthType {
    ApiKey,        // Ozon: Client-Id + Api-Key headers
    BearerToken,   // Wildberries: Authorization header (БЕЗ префикса Bearer)
    OAuth2,        // будущие маркетплейсы
}
```

**`Report`** — абстракция отчёта:

```rust
#[async_trait]
pub trait Report: Send + Sync {
    fn type_id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn category(&self) -> ReportCategory;
    fn downloader_kind(&self) -> DownloaderKind;
    fn parameters(&self) -> &[ReportParameter];

    async fn download(
        &self,
        auth: &dyn Authenticator,
        params: &ReportParams,
        progress: Arc<dyn ProgressCallback>,
        cancel: CancellationToken,
    ) -> CoreResult<Vec<DownloadedFile>>;
}

pub enum DownloaderKind {
    Api,           // Прямой HTTP-запрос
    ApiAsyncPoll,  // create → poll → download
}
```

#### 2.3.3 Реестр провайдеров

```rust
pub struct ProviderRegistry {
    providers: RwLock<HashMap<&'static str, Arc<dyn MarketplaceProvider>>>,
}

pub async fn register_all_providers(registry: &ProviderRegistry) -> CoreResult<()> {
    #[cfg(feature = "provider-ozon")]
    registry.register(Arc::new(crate::providers::ozon::OzonProvider::new()?)).await?;

    #[cfg(feature = "provider-wildberries")]
    registry.register(Arc::new(crate::providers::wildberries::WildberriesProvider::new()?)).await?;

    Ok(())
}
```

### 2.4 Структура проекта (глава 03)

```
mdwf/                              # корень workspace
├── Cargo.toml                      # workspace manifest
├── rustfmt.toml
├── clippy.toml
├── deny.toml
├── .github/workflows/
│   ├── ci.yml
│   └── release.yml
├── docs/
│   ├── adr/                        # Architecture Decision Records
│   │   ├── ADR-001-rust-tokio.md
│   │   ├── ADR-002-gtk4-gui.md
│   │   └── ...
│   ├── architecture.md
│   └── plugin-authoring.md
├── crates/
│   ├── core/                       # mdwf-core: трейты, типы, реестр
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── provider.rs         # trait MarketplaceProvider
│   │       ├── auth.rs             # trait Authenticator, AuthType
│   │       ├── report.rs           # trait Report, ReportCategory
│   │       ├── downloader.rs       # trait Downloader, DownloaderKind
│   │       ├── capabilities.rs     # struct Capabilities
│   │       ├── registry.rs         # ProviderRegistry
│   │       ├── profile.rs          # struct Profile
│   │       ├── params.rs           # ReportParams, ReportParameter
│   │       ├── progress.rs         # trait ProgressCallback
│   │       ├── error.rs            # CoreError (thiserror)
│   │       ├── health.rs           # HealthStatus
│   │       └── pagination.rs       # enum Pagination
│   ├── storage/                    # mdwf-storage: файловое хранилище + SQLite
│   │   └── src/
│   │       ├── file_store.rs
│   │       ├── naming.rs           # детерминированные имена файлов
│   │       ├── catalog.rs          # SqliteCatalog
│   │       ├── schema.sql
│   │       ├── migrations/
│   │       └── dedup.rs            # проверка дубликатов по хэшу
│   ├── secrets/                    # mdwf-secrets: OS keychain wrapper
│   │   └── src/
│   │       ├── keychain.rs         # trait SecretStore
│   │       ├── os_keychain.rs      # реализация через keyring crate
│   │       └── memory.rs           # in-memory mock для тестов
│   ├── scheduler/                  # mdwf-scheduler: cron + очередь
│   │   └── src/
│   │       ├── job.rs
│   │       ├── runner.rs
│   │       ├── cron.rs
│   │       └── persistence.rs
│   ├── providers/
│   │   ├── ozon/                   # mdwf-providers-ozon
│   │   │   └── src/
│   │   │       ├── lib.rs          # OzonProvider
│   │   │       ├── auth.rs         # OzonAuthenticator
│   │   │       ├── client.rs       # OzonHttpClient: rate limit, retry
│   │   │       ├── date_format.rs  # 3 форматтера дат
│   │   │       ├── pagination.rs   # 3 схемы пагинации
│   │   │       ├── reports/        # каждый отчёт — отдельный файл
│   │   │       │   ├── realization.rs
│   │   │       │   ├── buyout.rs
│   │   │       │   ├── b2b_sales.rs
│   │   │       │   ├── mutual_settlement.rs
│   │   │       │   ├── compensation.rs
│   │   │       │   └── ... (20 отчётов)
│   │   │       ├── capabilities.rs
│   │   │       └── error.rs
│   │   └── wildberries/            # mdwf-providers-wildberries
│   │       └── src/
│   │           ├── lib.rs          # WildberriesProvider
│   │           ├── auth.rs         # WbAuthenticator (Authorization: <token> БЕЗ Bearer)
│   │           ├── client.rs       # WbHttpClient: 4 типа токенов, rate limit
│   │           ├── date_format.rs  # RFC3339 Moscow (UTC+3)
│   │           ├── pagination.rs   # RrdidCursor, DateCursor, OffsetLimit, TaskId
│   │           ├── subclients/
│   │           │   ├── finance.rs     # finance-api.wildberries.ru
│   │           │   ├── documents.rs   # documents-api.wildberries.ru
│   │           │   ├── statistics.rs  # statistics-api.wildberries.ru
│   │           │   ├── analytics.rs   # seller-analytics-api.wildberries.ru
│   │           │   └── returns.rs     # returns-api.wildberries.ru
│   │           ├── reports/
│   │           │   ├── balance.rs
│   │           │   ├── sales_reports_detailed.rs
│   │           │   ├── documents.rs   # УПД, УКД, акты через documents API
│   │           │   └── ...
│   │           ├── capabilities.rs
│   │           └── error.rs
│   ├── cli/                        # mdwf-cli: CLI binary
│   │   └── src/
│   │       ├── main.rs
│   │       ├── commands/
│   │       │   ├── list_providers.rs
│   │       │   ├── download.rs
│   │       │   ├── schedule.rs
│   │       │   ├── profile.rs
│   │       │   ├── out_of_scope.rs # список out-of-scope документов
│   │       │   └── doctor.rs
│   │       └── output.rs
│   ├── gui/                        # mdwf-gui: GUI binary (GTK4 + libadwaita)
│   │   └── src/
│   │       ├── main.rs             # gtk::Application + libadwaita::init
│   │       ├── app.rs              # MdwfApp: GObject
│   │       ├── views/
│   │       │   ├── profiles.rs
│   │       │   ├── profile_edit.rs # динамическая форма из Capabilities
│   │       │   ├── reports.rs
│   │       │   ├── download.rs
│   │       │   ├── scheduler.rs
│   │       │   ├── settings.rs
│   │       │   ├── logs.rs
│   │       │   └── about.rs
│   │       ├── widgets/
│   │       │   ├── progress_bar.rs # GtkProgressBar обёртка
│   │       │   ├── dynamic_form.rs # GtkGrid форма из ReportParameter[]
│   │       │   └── file_tree.rs    # GtkColumnView / GtkListView
│   │       └── theme.rs            # CSS-провайдеры, Adwaita
│   └── api/                        # mdwf-api: опциональный REST API
│       └── src/
│           ├── routes.rs           # axum роуты
│           └── auth.rs
├── tests/                          # интеграционные тесты
│   ├── ozon_e2e.rs
│   ├── wb_e2e.rs
│   └── scheduler_e2e.rs
├── examples/
│   ├── minimal_provider.rs
│   └── headless_download.rs
└── benches/
    └── pagination.rs
```

#### 2.4.1 Cargo workspace

```toml
# Cargo.toml (корневой)
[workspace]
resolver = "2"
members = [
    "crates/core",
    "crates/storage",
    "crates/secrets",
    "crates/scheduler",
    "crates/providers/ozon",
    "crates/providers/wildberries",
    "crates/cli",
    "crates/gui",
    "crates/api",
]
default-members = ["crates/cli", "crates/gui"]

[workspace.dependencies]
tokio = { version = "1.38", features = ["full"] }
async-trait = "0.1"
thiserror = "1.0"
anyhow = "1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
toml = "0.8"
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json", "stream"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
chrono = { version = "0.4", features = ["serde"] }
chrono-tz = "0.9"
parking_lot = "0.12"
arc-swap = "1.7"
sha2 = "0.10"
rusqlite = { version = "0.31", features = ["bundled"] }
keyring = "2.3"
clap = { version = "4.5", features = ["derive", "env"] }
# GUI (GTK4 + libadwaita)
gtk4 = "0.9"
libadwaita = { version = "0.7", features = ["v1_5"] }
glib = "0.19"
gio = "0.19"
# internal
mdwf-core = { path = "crates/core", version = "1.0.0" }
mdwf-storage = { path = "crates/storage", version = "1.0.0" }
mdwf-secrets = { path = "crates/secrets", version = "1.0.0" }
mdwf-scheduler = { path = "crates/scheduler", version = "1.0.0" }
mdwf-providers-ozon = { path = "crates/providers/ozon", version = "1.0.0" }
mdwf-providers-wildberries = { path = "crates/providers/wildberries", version = "1.0.0" }
```

### 2.5 GUI (глава 04)

#### 2.5.1 Технологический выбор: GTK4 + libadwaita

GUI реализован на **GTK4 + libadwaita** через крейты `gtk4` и `libadwaita` (экосистема gtk-rs). Решение зафиксировано в ADR-002.

Ключевые факторы выбора:
- **Профессиональный нативный внешний вид** — дизайн GNOME Adwaita, стандарт для корпоративных Linux-приложений.
- **Нативные виджеты** — GtkEntry, GtkButton, GtkComboBox, GtkListView, GtkColumnView, AdwLeaflet.
- **Доступность (a11y)** — ATK: Orca/NVDA/VoiceOver, навигация с клавиатуры, high-contrast.
- **Retained-mode** — перерисовка только изменённых областей; эффективен для длительной работы в фоне.
- **Зрелость** — GTK с 1998 года, GTK4 с 2020; стабильный ABI.
- **Cross-platform** — Linux (нативно), Windows/macOS через bundled GTK runtime.
- **CSS-стилизация** — фирменные цвета MDWF поверх Adwaita через CSS-провайдер.

#### 2.5.2 Архитектура GUI

GUI строго следует принципу «никакой бизнес-логики в UI». Окна и виджеты только отображают состояние и отправляют команды в доменный слой. Асинхронные задачи tokio communiцируют с GTK через `glib::MainContext`.

```rust
// crates/gui/src/app.rs — скелет приложения на GTK4 + libadwaita
use gtk4::{prelude::*, Application, ApplicationWindow, gtk::Builder};
use gtk4::gio;
use libadwaita as adw;
use tokio::sync::mpsc;
use std::sync::Arc;
use parking_lot::RwLock;

pub struct MdwfApp {
    gtk_app: adw::Application,
    state: Arc<RwLock<UiState>>,
    cmd_tx: mpsc::UnboundedSender<UiCommand>,
    event_rx: Arc<RwLock<Option<mpsc::UnboundedReceiver<UiEvent>>>>,
}

impl MdwfApp {
    pub fn new(cmd_tx: mpsc::UnboundedSender<UiCommand>) -> Self {
        let gtk_app = adw::Application::new(
            Some("dev.mdwf.MDWF"),
            gio::ApplicationFlags::FLAGS_NONE,
        );

        let app = Self {
            gtk_app,
            state: Arc::new(RwLock::new(UiState::default())),
            cmd_tx,
            event_rx: Arc::new(RwLock::new(None)),
        };

        let state = app.state.clone();
        let cmd_tx = app.cmd_tx.clone();
        app.gtk_app.connect_activate(move |gtk_app| {
            Self::build_main_window(gtk_app, &state, &cmd_tx);
        });

        app
    }

    pub fn run(&self) -> std::process::ExitCode {
        self.gtk_app.run()
    }

    fn build_main_window(
        app: &adw::Application,
        state: &Arc<RwLock<UiState>>,
        cmd_tx: &mpsc::UnboundedSender<UiCommand>,
    ) {
        let builder = Builder::from_string(include_str!("ui/main_window.ui"));
        let window: adw::ApplicationWindow = builder.object("main_window")
            .expect("main_window not found");
        window.set_application(Some(app));

        let stack: gtk4::Stack = builder.object("content_stack").unwrap();

        // Навигация через кнопки → переключение страниц Stack
        for (name, view_id) in [
            ("nav_profiles",  ViewId::Profiles),
            ("nav_reports",   ViewId::Reports),
            ("nav_download",  ViewId::Download),
            ("nav_scheduler", ViewId::Scheduler),
            ("nav_settings",  ViewId::Settings),
            ("nav_logs",      ViewId::Logs),
            ("nav_about",     ViewId::About),
        ] {
            let btn: gtk4::Button = builder.object(name).unwrap();
            let stack_clone = stack.clone();
            btn.connect_clicked(move |_| {
                stack_clone.set_visible_child_name(view_id.as_str());
            });
        }

        window.present();
    }
}
```

#### 2.5.3 Динамическая форма из Capabilities

```rust
// crates/gui/src/views/profile_edit.rs — динамическая форма на GTK4
use gtk4::{prelude::*, Grid, Label, Entry, ComboBoxText, SpinButton};
use mdwf_core::capabilities::{AuthField, AuthFieldKind};

pub fn build_auth_fields(
    fields: &[AuthField],
) -> (Grid, std::collections::HashMap<String, gtk4::Widget>) {
    let grid = Grid::new();
    grid.set_column_spacing(12);
    grid.set_row_spacing(8);
    grid.set_margin_top(16);
    grid.set_margin_bottom(16);
    grid.set_margin_start(16);
    grid.set_margin_end(16);

    let mut widgets = std::collections::HashMap::new();

    for (row, field) in fields.iter().enumerate() {
        let label_text = if field.required {
            format!("{} *", field.label)
        } else {
            field.label.to_string()
        };
        let label = Label::new(Some(&label_text));
        label.set_halign(gtk4::Align::End);
        grid.attach(&label, 0, row as i32, 1, 1);

        let widget: gtk4::Widget = match field.kind {
            AuthFieldKind::Text => {
                let entry = Entry::new();
                if let Some(p) = field.placeholder { entry.set_placeholder_text(Some(p)); }
                entry.upcast::<gtk4::Widget>()
            }
            AuthFieldKind::Password => {
                let entry = Entry::new();
                entry.set_visibility(false);
                entry.set_input_purpose(gtk4::InputPurpose::Password);
                entry.upcast::<gtk4::Widget>()
            }
            AuthFieldKind::Number => {
                let spin = SpinButton::with_range(0.0, f64::MAX, 1.0);
                spin.set_digits(0);
                spin.upcast::<gtk4::Widget>()
            }
            AuthFieldKind::Select(options) => {
                let combo = ComboBoxText::new();
                for opt in &options { combo.append_text(opt); }
                combo.set_active(0);
                combo.upcast::<gtk4::Widget>()
            }
        };

        grid.attach(&widget, 1, row as i32, 1, 1);
        widgets.insert(field.id.to_string(), widget);
    }

    (grid, widgets)
}
```

#### 2.5.4 Тема оформления через CSS

```rust
// crates/gui/src/theme.rs — AdwStyleManager + GTK CSS
use gtk4::gdk::Display;
use gtk4::CssProvider;
use libadwaita as adw;

pub fn apply_brand_css(app: &adw::Application) {
    let css = r#"
        @define-color mdwf_primary #1a365d;
        @define-color mdwf_accent  #2b6cb0;
        @define-color mdwf_success #16a34a;
        @define-color mdwf_warning #d97706;
        @define-color mdwf_error   #dc2626;

        button.suggested-action {
            background-color: @mdwf_accent;
        }
        progressbar > trough > progress {
            background-color: @mdwf_accent;
        }
        .status-ok    { color: @mdwf_success; }
        .status-warn  { color: @mdwf_warning; }
        .status-error { color: @mdwf_error;   }
        .dim-label {
            color: alpha(@theme_fg_color, 0.55);
            font-size: 0.9em;
        }
    "#;

    let provider = CssProvider::new();
    provider.load_from_data(css.as_bytes());

    if let Some(display) = Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

pub fn set_color_scheme(scheme: ColorScheme) {
    let manager = adw::StyleManager::default();
    let adw_scheme = match scheme {
        ColorScheme::System => adw::ColorScheme::Default,
        ColorScheme::Light  => adw::ColorScheme::ForceLight,
        ColorScheme::Dark   => adw::ColorScheme::ForceDark,
    };
    manager.set_color_scheme(adw_scheme);
}

pub enum ColorScheme { System, Light, Dark }
```

### 2.6 CLI (глава 05)

#### 2.6.1 Подкоманды

```bash
# Управление провайдерами
mdwf providers list
mdwf providers info <provider_id>

# Управление профилями
mdwf profiles list
mdwf profiles add --provider ozon --name "Ozon-1" --client-id 1234567 --api-key "secret"
mdwf profiles delete <name> [--yes]
mdwf profiles check <name>

# Список отчётов
mdwf reports list --provider ozon
mdwf reports info <provider_id> <report_type_id>

# Выгрузка
mdwf download \
  --profile "Ozon-1" \
  --report ozon.realization \
  --report ozon.mutual_settlement \
  --period 2026-06 \
  --output-dir /mnt/shared/ozon/june-2026

# Расписание
mdwf schedule list
mdwf schedule add --name "monthly" --profile "Ozon-1" \
  --report ozon.realization --cron "0 2 1 * *"
mdwf schedule run <name>

# Out-of-scope документы (недоступны через API)
mdwf out-of-scope [--provider <id>]

# Диагностика
mdwf doctor
```

#### 2.6.2 Коды возврата

| Код | Имя | Значение |
|-----|-----|----------|
| 0 | SUCCESS | Успех |
| 1 | GENERIC_ERROR | Общая ошибка |
| 2 | USAGE_ERROR | Неверные аргументы |
| 3 | CONFIG_ERROR | Ошибка конфигурации |
| 4 | AUTH_ERROR | Ошибка авторизации |
| 5 | NETWORK_ERROR | Сетевая ошибка |
| 6 | RATE_LIMIT | Превышен rate limit |
| 7 | API_ERROR | API вернул ошибку |
| 8 | STORAGE_ERROR | Ошибка ФС/БД |
| 9 | NOT_FOUND | Ресурс не найден |
| 11 | DEPRECATED_METHOD | Метод устарел и отключён |
| 12 | PARTIAL_SUCCESS | Часть операций успешна, часть — нет |
| 13 | CANCELLED | Отменено пользователем |
| 64 | OUT_OF_SCOPE | Документ недоступен через API |

### 2.7 Настройки (глава 06)

#### 2.7.1 config.toml

```toml
schema_version = 2

[app]
ui_scale = 100
theme = "system"
language = "ru"
start_minimized = false
confirm_exit_during_download = true

[storage]
output_dir = "~/Documents/MDWF/downloads"
file_name_template = "{provider}_{profile}_{report}_{period}.{ext}"
folder_structure = "by_provider_month"
compute_hash = true

[security]
use_keychain = true
lock_timeout_minutes = 0
log_retention_days = 30

[network]
request_timeout_seconds = 30
max_concurrency_per_provider = 3
use_system_proxy = true
max_retries = 5
retry_base_delay_ms = 500
retry_max_delay_ms = 30000

[scheduler]
enabled_on_start = true
autostart_with_os = false
max_parallel_jobs = 3

[logging]
level = "info"
dir = "~/.mdwf/logs"
format = "text"
rotation = "daily"
max_files = 30

[providers.ozon]
base_url = "https://api-seller.ozon.ru"
use_deprecated_transaction_list = false

[providers.wildberries]
documents_batch_size = 50
```

#### 2.7.2 Схема SQLite

```sql
CREATE TABLE profiles (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL UNIQUE,
    provider_id     TEXT NOT NULL,
    description     TEXT,
    auth_metadata   TEXT,
    keychain_id     TEXT,
    created_at      TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at      TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_check_at   TIMESTAMP,
    last_check_ok   BOOLEAN
);

CREATE TABLE downloads (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id      INTEGER NOT NULL,
    report_type     TEXT NOT NULL,
    period          TEXT,
    params          TEXT,
    file_path       TEXT NOT NULL,
    file_size       INTEGER NOT NULL,
    file_hash       TEXT,
    file_format     TEXT NOT NULL,
    rows_count      INTEGER,
    downloader_kind TEXT NOT NULL,
    source_url      TEXT,
    downloaded_at   TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(profile_id, report_type, period, file_hash)
);

CREATE TABLE schedules (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL UNIQUE,
    profile_id      INTEGER NOT NULL,
    reports         TEXT NOT NULL,
    cron_expr       TEXT NOT NULL,
    period_offset   INTEGER NOT NULL DEFAULT 0,
    params          TEXT,
    enabled         BOOLEAN NOT NULL DEFAULT 1,
    next_run_at     TIMESTAMP,
    last_run_at     TIMESTAMP,
    last_run_status TEXT
);
```

### 2.8 Планировщик (глава 08)

#### 2.8.1 Cron-выражения

| Шаблон | Cron | Описание |
|--------|------|----------|
| 1-е число месяца в 02:00 | `0 2 1 * *` | Ежемесячная выгрузка |
| Каждый понедельник в 09:00 | `0 9 * * 1` | Еженедельная выгрузка |
| Каждый день в 09:00 | `0 9 * * *` | Ежедневная выгрузка |
| Каждый квартал | `0 2 1 1,4,7,10 *` | Квартальные акты сверки |

#### 2.8.2 Retry policy

| Попытка | Задержка (база) | Jitter |
|---------|-----------------|--------|
| 2 | 500 мс | ±250 мс |
| 3 | 1000 мс | ±500 мс |
| 4 | 2000 мс | ±1000 мс |
| 5 | 4000 мс | ±2000 мс |
| 6 | 8000 мс | ±4000 мс |

Не ретраятся: 400, 401, 403, 404, 422. Ретраятся: 429, 5xx, сетевые ошибки.

### 2.9 Реализация провайдера Ozon (глава 11)

#### 2.9.1 Авторизация

```rust
// crates/providers/ozon/src/auth.rs
pub struct OzonAuthenticator {
    client_id: i64,
    api_key: mdwf_core::secret::SecretString,
    key_created_at: Option<DateTime<Utc>>,
}

impl Authenticator for OzonAuthenticator {
    fn apply(&self, req: RequestBuilder) -> RequestBuilder {
        req.header("Client-Id", self.client_id.to_string())
           .header("Api-Key", self.api_key.expose_secret())
    }

    fn expires_at(&self) -> Option<DateTime<Utc>> {
        self.key_created_at.map(|t| t + Duration::days(180))
    }

    fn auth_type(&self) -> AuthType { AuthType::ApiKey }
}
```

**TTL API-ключа:** 180 дней. MDWF предупреждает за 14 дней до истечения.

#### 2.9.2 Форматирование дат

Ozon API использует три формата дат:

```rust
// ISO 8601 UTC с миллисекундами и Z (v3 endpoints)
// "2026-07-03T00:00:00.000Z"
pub fn format_iso8601_ms_z(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

// Только месяц (realization)
// "2026-06"
pub fn format_year_month(year: i32, month: u32) -> String {
    format!("{:04}-{:02}", year, month)
}

// Только дата (compensation, decompensation, accrual/by-day)
// "2026-07-03"
pub fn format_date_only(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}
```

#### 2.9.3 Пагинация

Три схемы пагинации:

| Схема | Параметры | Endpoints |
|-------|-----------|-----------|
| Pages | `page` (1-based), `page_size` (max 1000) | `/v3/finance/transaction/list`, `/v1/finance/compensation` |
| Cursor | `last_id`, `limit` (max 1000) | `/v3/posting/fbs/list`, `/v2/posting/fbo/list` |
| Offset | `limit` + `offset` | `/v1/finance/document-b2b-sales`, `/v1/finance/mutual-settlement` |

#### 2.9.4 Health check

```rust
pub async fn health_check(auth: &dyn Authenticator) -> CoreResult<HealthStatus> {
    let body = serde_json::json!({});
    match self.post("/v1/finance/balance", &body, auth, &Default::default()).await {
        Ok(_) => {
            if let Some(expires_at) = auth.expires_at() {
                let days_left = (expires_at - chrono::Utc::now()).num_days();
                if days_left < 3 {
                    return Ok(HealthStatus::down(format!("API key expires in {} days", days_left)));
                }
                if days_left < 14 {
                    return Ok(HealthStatus::degraded(format!("API key expires in {} days", days_left)));
                }
            }
            Ok(HealthStatus::ok())
        }
        Err(CoreError::Network(e)) if e.is_timeout() => {
            Ok(HealthStatus::down("network timeout".into()))
        }
        Err(_) => Ok(HealthStatus::down("auth failed".into())),
    }
}
```

### 2.10 Реализация провайдера Wildberries (глава 12)

#### 2.10.1 Авторизация

```rust
// crates/providers/wildberries/src/auth.rs
pub struct WbAuthenticator {
    token: mdwf_core::secret::SecretString,
    token_type: WbTokenType,
}

#[derive(Debug, Clone, Copy)]
pub enum WbTokenType {
    Personal,  // Основной тип для продавцов
    Service,   // Облачные сервисы из каталога WB
    Base,      // Ограниченный доступ, низкие rate limits
    Test,      // Sandbox
}

impl Authenticator for WbAuthenticator {
    fn apply(&self, req: RequestBuilder) -> RequestBuilder {
        // КРИТИЧЕСКИ: БЕЗ префикса "Bearer "!
        req.header("Authorization", self.token.expose_secret())
    }

    fn expires_at(&self) -> Option<DateTime<Utc>> {
        Some(self.key_created_at + chrono::Duration::days(180))
    }

    fn auth_type(&self) -> AuthType { AuthType::BearerToken }
}
```

#### 2.10.2 Под-клиенты по доменам

| Под-клиент | Домен | Назначение | Rate limit (Personal) |
|------------|-------|------------|----------------------|
| WbFinanceClient | `finance-api.wildberries.ru` | Баланс, отчёты реализации, эквайринг | 1 RPM burst 1 |
| WbDocumentsClient | `documents-api.wildberries.ru` | Список/скачивание документов (УПД, акты) | 1 req/10s burst 5 |
| WbStatisticsClient | `statistics-api.wildberries.ru` | Заказы, продажи, поставки | 1 RPM burst 10 |
| WbAnalyticsClient | `seller-analytics-api.wildberries.ru` | Штрафы, замеры, антифрод | 1 RPM burst 1 |
| WbReturnsClient | `returns-api.wildberries.ru` | Возвраты | 1 RPM burst 1 |

#### 2.10.3 Documents API — скачивание УПД, актов

Трёхшаговый паттерн:

```rust
pub async fn download_documents_by_category(
    &self,
    auth: &dyn Authenticator,
    category: &str,           // "upd", "upd-purchase-from-legal", "sale-to-le-signed", ...
    date_from: chrono::NaiveDate,
    date_to: chrono::NaiveDate,
    extensions: &[&str],      // ["xml", "pdf", "xlsx", "zip"]
) -> CoreResult<Vec<DownloadedFile>> {
    // Шаг 1: проверить, что категория поддерживается WB
    let supported = self.documents_client.list_categories(auth).await?;
    if !supported.iter().any(|c| c.name == category) {
        return Err(CoreError::InvalidParameter(format!(
            "WB documents API не возвращает категорию '{}'", category
        )));
    }

    // Шаг 2: получить список документов
    let documents = self.documents_client.list_documents(auth, ListDocumentsParams {
        category: Some(category.to_string()),
        begin_time: Some(date_from),
        end_time: Some(date_to),
        sort: Some("date".to_string()),
        order: Some("desc".to_string()),
        limit: 1000,
        offset: 0,
        locale: "ru".to_string(),
    }).await?;

    // Шаг 3: скачать батчами по 50 через D4
    let mut files = Vec::new();
    for chunk in documents.chunks(50) {
        let batch: Vec<(String, String)> = chunk.iter()
            .filter_map(|doc| {
                let ext = extensions.iter()
                    .find(|e| doc.extensions.iter().any(|de| de == **e))
                    .or_else(|| doc.extensions.first())
                    .copied()?;
                Some((doc.service_name.clone(), ext.to_string()))
            })
            .collect();
        if batch.is_empty() { continue; }
        let downloaded = self.documents_client.download_batch(auth, &batch).await?;
        files.extend(downloaded);
    }
    Ok(files)
}
```

**Реестр категорий WB:**

| category | Назначение | Верификация | Форматы |
|----------|------------|-------------|---------|
| `upd` | УПД | forum/1602 | xml, zip |
| `upd-purchase-from-legal` | УПД (покупка у юрлица) | forum/1602 | xml, zip |
| `sale-to-le-signed` | УКД (продажа юрлицу) | forum/1602 | xml, zip |
| `redeem-notification` | Уведомление о выкупе | spec example | xlsx, zip |
| `act-income-mp` | Акт за МП-услуги | spec example | xlsx, zip |

### 2.11 Обработка ошибок (глава 14)

#### 2.11.1 Иерархия ошибок

```rust
// crates/core/src/error.rs (thiserror)
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("provider not registered: {0}")]
    ProviderNotFound(String),

    #[error("profile not found: {0}")]
    ProfileNotFound(String),

    #[error("report type not supported: {0}")]
    ReportTypeNotSupported(String),

    #[error("invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("secret not found in keychain: {0}")]
    SecretNotFound(String),

    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("operation cancelled")]
    Cancelled,

    #[error("internal error: {0}")]
    Internal(String),
}

pub type CoreResult<T> = Result<T, CoreError>;
```

#### 2.11.2 Circuit breaker

```rust
pub struct CircuitBreaker {
    failure_threshold: u32,        // 5 ошибок подряд → открыть
    failure_count: AtomicU32,
    is_open: AtomicBool,
    cooldown: Duration,            // 5 минут
}

impl CircuitBreaker {
    pub fn check(&self) -> Result<(), CircuitBreakerError> {
        if self.is_open.load(Ordering::Relaxed) {
            let opened = self.opened_at.lock();
            if let Some(t) = *opened {
                if t.elapsed() < self.cooldown {
                    return Err(CircuitBreakerError::Open {
                        remaining: self.cooldown - t.elapsed(),
                    });
                }
            }
            self.is_open.store(false, Ordering::Relaxed);
            self.failure_count.store(0, Ordering::Relaxed);
        }
        Ok(())
    }

    pub fn on_success(&self) {
        self.failure_count.store(0, Ordering::Relaxed);
    }

    pub fn on_failure(&self) {
        let count = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
        if count >= self.failure_threshold {
            self.is_open.store(true, Ordering::Relaxed);
        }
    }
}
```

### 2.12 Тестирование (глава 15)

#### 2.12.1 Пирамида тестирования

| Уровень | Что проверяет | Количество | Время |
|---------|---------------|------------|-------|
| Unit | Отдельные функции | ≥ 200 | < 5 сек |
| Integration | Взаимодействие модулей, mock-сервер | ≥ 50 | < 30 сек |
| E2E | Полные сценарии через CLI | ≥ 20 | < 5 мин |
| Smoke | Критические пути | 5–10 | < 1 мин |

#### 2.12.2 Тестирование GTK4-виджетов

```rust
// crates/gui/tests/widget_test.rs
use gtk4::{prelude::*, test::*};
use mdwf_gui::views::profile_edit::build_auth_fields;
use mdwf_core::capabilities::{AuthField, AuthFieldKind};

#[gtk_test]
fn test_auth_form_builds_all_fields() {
    let fields = vec![
        AuthField { id: "client_id", label: "Client-Id", kind: AuthFieldKind::Number,
                    required: true, placeholder: None, help_text: None, secret: false },
        AuthField { id: "api_key", label: "Api-Key", kind: AuthFieldKind::Password,
                    required: true, placeholder: None, help_text: None, secret: true },
    ];

    let (grid, widgets) = build_auth_fields(&fields);

    assert_eq!(widgets.len(), 2);
    assert!(widgets.contains_key("client_id"));
    assert!(widgets.contains_key("api_key"));
}

#[gtk_test]
fn test_password_field_hides_input() {
    let fields = vec![
        AuthField { id: "api_key", label: "Api-Key", kind: AuthFieldKind::Password,
                    required: true, placeholder: None, help_text: None, secret: true },
    ];
    let (_grid, widgets) = build_auth_fields(&fields);
    let widget = widgets.get("api_key").unwrap();
    let entry: &gtk4::Entry = widget.downcast_ref().unwrap();

    assert!(!entry.visibility());
    assert_eq!(entry.input_purpose(), gtk4::InputPurpose::Password);
}
```

### 2.13 Стандарты кодирования (глава 16)

#### 2.13.1 Запрещённые паттерны

| Паттерн | Замена | Обоснование |
|---------|--------|-------------|
| `.unwrap()` | `?` или match | Паника в продакшене |
| `.expect("msg")` | `.context("msg")?` | То же |
| `panic!()` | Возврат `Err(...)` | Паника ломает приложение |
| `unreachable!()` | `Err(InternalError::Unreachable)` | «Невозможное» может случиться |
| `.clone()` без необходимости | Ссылка или `Arc` | Лишние аллокации |
| `unsafe` вне аудита | Безопасная обёртка | Изоляция unsafe |
| `println!()` в lib | `tracing::info!()` | Структурированное логирование |
| Глобальный `mut static` | `OnceLock` / `ArcSwap` | Thread safety |

#### 2.13.2 Code review чек-лист

1. ☐ `cargo fmt --check` проходит
2. ☐ `cargo clippy -- -D warnings` без предупреждений
3. ☐ Все тесты проходят
4. ☐ Покрытие не снизилось
5. ☐ Публичный API задокументирован
6. ☐ Нет `unwrap()`/`expect()`/`panic!()` вне тестов
7. ☐ Имена соответствуют конвенциям
8. ☐ Ошибки типизированы и возвращают контекст
9. ☐ Секреты не попадают в логи
10. ☐ CHANGELOG.md обновлён

### 2.14 Порядок разработки (глава 17)

| № | Этап | Результат | Критерий завершения |
|---|------|-----------|---------------------|
| 1 | Утверждение Спецификации | Прочтение глав 00–18 | Оператор пишет «Спецификация утверждена» |
| 2 | Скелет workspace | Cargo workspace + пустые крейты + CI | `cargo build` succeeds |
| 3 | Core-трейты | mdwf-core с трейтами | Все трейты определены |
| 4 | Storage + Secrets | mdwf-storage + mdwf-secrets | Миграции применяются |
| 5 | Mock-провайдер | TestProvider | GUI/CLI показывают mock-отчёты |
| 6 | OzonProvider | 20 отчётов | Все отчёты работают |
| 7 | GUI | GTK4 + libadwaita приложение | Можно создать профиль, выгрузить отчёт |
| 8 | CLI | Все подкоманды | CI пример работает |
| 9 | Scheduler | Планировщик + автозапуск ОС | Cron-правило срабатывает |
| 10 | Out-of-scope документация | Команда `mdwf out-of-scope` | Список выведен корректно |
| 11 | WildberriesProvider | Все отчёты из таблицы 12.3 | `git diff src/core/` пуст |
| 12 | Тестирование | Покрытие ≥ 80% core | CI green |
| 13 | Документация | cargo doc + user guide | Новый провайдер за 5 дней |
| 14 | Релиз v1.4 | Binary artifacts | Smoke-тесты прошли |

---

## 3. Counterarguments / Limitations

### 3.1 Ограничение: не все документы доступны через API

**Проблема:** Акты сверки, счета на оплату, договоры WB; УПД с доп. услугами, обеспечительные платежи Ozon — не имеют API-эндпоинтов.

**Решение MDWF:** Эти документы помечены как out-of-scope. Пользователь получает их вручную через личный кабинет. Команда `mdwf out-of-scope` выводит список с инструкциями.

**Компромисс:** Полнота автоматизации ограничена в пользу нулевого риска блокировки аккаунта. Это сознательное решение, принятое из-за запрета автоматизации личного кабинета в ToS маркетплейсов.

### 3.2 Ограничение: GTK4 требует bundled runtime на Windows/macOS

**Проблема:** На Linux GTK4 + libadwaita — системные пакеты. На Windows и macOS требуется bundled GTK runtime (~30 МБ к размеру бинарника).

**Решение:** Использовать gvsbuild (Windows) и Homebrew (macOS) для сборки bundled GTK. Размер дистрибутива: 60–80 МБ (Windows), 50–70 МБ (macOS), 20–30 МБ (Linux). Для Linux предпочтителен Flatpak (изоляция + bundle GTK).

**Компромисс:** Увеличение размера бинарника на ~30 МБ приемлемо для desktop-приложения. Взамен — нативный профессиональный интерфейс.

### 3.3 Ограничение: сложность GTK4 для разработчиков

**Проблема:** GTK4 имеет крутую кривую обучения: GObject-система, сигналы, свойства, GtkBuilder XML, composite templates.

**Решение:** Команда должна быть знакома с концепциями GObject до начала реализации. Документация содержит подробные примеры кода на gtk-rs. Для долгоживущего продукта сложность GTK4 оправдана: GTK-приложения легче поддерживать, расширять и локализовать.

### 3.4 Ограничение: rate limits WB documents API

**Проблема:** Эндпоинт `/api/v1/documents/download/all` (D4) имеет лимит 1 req/5 min, burst 5 — максимум 600 документов в час. Для продавцов с тысячами УПД backfill займёт дни.

**Решение:** MDWF реализует очередь с приоритетами: свежие документы выгружаются первыми, исторические — фоновым процессом. Прогресс отображается в GUI.

### 3.5 Риск: устаревание API маркетплейсов

**Проблема:** Ozon и WB активно обновляют API, выводя из эксплуатации старые методы.

**Текущие критические даты:**
- Ozon `/v3/finance/transaction/list` — отключение 6 июля 2026 (замена: `/v1/finance/accrual/postings`, `/v1/finance/accrual/by-day`, `/v1/finance/compensation`, `/v1/finance/decompensation`)
- WB `/api/v5/supplier/reportDetailByPeriod` — отключение 15 июля 2026 (замена: `POST /api/finance/v1/sales-reports/detailed`)
- WB `/api/v1/supplier/stocks` — уже отключён 23 июня 2026

**Решение:** MDWF реализует feature-флаги для переключения между устаревшими и новыми методами. Автоматический мониторинг дат отключения. Предупреждения пользователю за 30 дней до отключения.

---

## 4. Conclusion & Implications

### 4.1 Резюме

Marketplace Downloader Framework v1.4 — кросс-платформенное desktop-приложение на Rust для автоматизированной выгрузки финансовых документов с маркетплейсов через их официальные API. Архитектура построена по принципу «Framework First»: ядро не зависит от конкретных маркетплейсов, добавление нового провайдера требует только создания крейта в `crates/providers/<name>/` и одной строки регистрации.

Ключевые характеристики:
- **Только официальное API** — cabinet scraper полностью исключён (ToS маркетплейсов).
- **GTK4 + libadwaita** — профессиональный нативный интерфейс для корпоративных пользователей.
- **Расширяемость** — добавление Wildberries не требует изменения ядра (`git diff src/core/` пуст).
- **Идемпотентность** — дедупликация по SHA-256; повторная выгрузка не создаёт дубликатов.
- **Безопасность** — секреты в OS keychain; маскирование в логах; нулевой риск блокировки аккаунта.

### 4.2 Покрытие документов

**Ozon:** 20 отчётов через API (отчёты реализации, выкупы, УПД по выкупленным товарам, B2B продажи, взаиморасчёты, компенсации, декомпенсации, начисления, баланс, ДДС, акт о расхождениях, аналитика). 5 документов out-of-scope (УПД с доп. услугами, отчёты партнёров, обеспечительные платежи, счета, акты сверки).

**Wildberries:** 24 отчёта через API (баланс, реализация, эквайринг, УПД/УКД через documents API, штрафы, антифрод, возвраты). 3 документа out-of-scope (акты сверки, счета, договоры).

### 4.3 Будущие направления

1. **Мониторинг расширения API** — регулярная проверка появления новых категорий в `GET /api/v1/documents/categories` (WB) и новых эндпоинтов Ozon.
2. **Импорт вручную скачанных файлов** — команда `mdwf import` для загрузки PDF/XML, которые пользователь скачал вручную.
3. **Уведомления о готовности квартальных документов** — планировщик напоминает о необходимости скачать акты сверки вручную.
4. **Лоббирование расширения API** — через сообщество продавцов запрашивать добавление API-эндпоинтов для актов сверки и счетов.
5. **Server mode (v2.0)** — REST API + web-frontend для команд.
6. **Динамические плагины (v2.2)** — runtime-плагины через libloading или WebAssembly.
7. **International marketplaces (v3.0)** — Amazon SP-API, eBay, Etsy.

### 4.4 Призыв к действию

Для начала разработки MDWF выполните следующие шаги:

1. Ознакомьтесь с настоящей спецификацией целиком (главы 00–18).
2. Создайте Cargo workspace по структуре из главы 03.
3. Реализуйте core-трейты (глава 09).
4. Создайте TestProvider для проверки архитектуры (этап 5).
5. Реализуйте OzonProvider (глава 11, этап 6).
6. Соблюдайте принцип поэтапного утверждения (глава 17): ни один этап не начинается без подтверждения оператора.

Все шаги воспроизводимы и не требуют дополнительных уточнений. Документ самодостаточен.

---

## 5. References

### 5.1 Ozon Seller API

1. Официальная PDF-документация Ozon Seller API v2.1 (665 страниц) — предоставлена пользователем.
2. Ozon for dev: `https://dev.ozon.ru/`
3. News/699 — новые методы для финансовых отчётов: `https://dev.ozon.ru/news/699`
4. News/584 — новые лимиты Seller API (50 RPS): `https://dev.ozon.ru/news/584`
5. News/649 — обновление правил API-ключей (TTL 180 дней): `https://dev.ozon.ru/news/649`
6. Community/1261 — требование Admin role для b2b-sales и mutual-settlement: `https://dev.ozon.ru/community/1261`
7. Community/1809 — API для фактических выкупов: `https://dev.ozon.ru/community/1809`

### 5.2 Wildberries API

8. Официальная OpenAPI-спецификация (раздел «Документы»): `https://dev.wildberries.ru/docs/openapi/financial-reports-and-accounting#tag=Dokumenty`
9. WB API Authorization System: `https://dev.wildberries.ru/en/knowledge-base/articles/019d49a1-0d73-71e9-be3e-b2c44567470c/wb-api-authorization-system`
10. WB API Rate Limits: `https://dev.wildberries.ru/en/knowledge-base/articles/019d49a1-28ca-7735-f2f-98210695abc7/wb-api-rate-limits`
11. WB API Error Codes: `https://dev.wildberries.ru/knowledge-base/articles/019d49a1-2cb0-781d-8921-deaf4a014a58/rasshifrovka-kodov-oshibok-wb-api`
12. Release notes id=188 — deprecation reportDetailByPeriod: `https://dev.wildberries.ru/en/release-notes?id=188`
13. Release notes id=194 — stocks отключён 23 июня 2026: `https://dev.wildberries.ru/en/release-notes?id=194`
14. News/148 — обновление системы токенов (4 типа): `https://dev.wildberries.ru/news/148`
15. Forum/1602 — реальный ответ /categories (категории upd, sale-to-le-signed): `https://dev.wildberries.ru/forum/1602`
16. Forum/2141 — X-Ratelimit-Retry значения 600+ секунд: `https://dev.wildberries.ru/forum/2141`
17. Зеркало OpenAPI spec: `https://github.com/eslazarev/wildberries-sdk` (specs/13-finances.yaml)

### 5.3 Технологии

18. GTK4 documentation: `https://docs.gtk.org/gtk4/`
19. libadwaita documentation: `https://gnome.pages.gitlab.gnome.org/libadwaita/`
20. gtk-rs (Rust bindings): `https://gtk-rs.org/`
21. Tokio (async runtime): `https://tokio.rs/`
22. tracing (structured logging): `https://docs.rs/tracing/`
23. thiserror (error handling): `https://docs.rs/thiserror/`
24. reqwest (HTTP client): `https://docs.rs/reqwest/`
25. reportlab (PDF generation для предыдущих версий): `https://docs.reportlab.com/`

### 5.4 Дополнительные материалы

26. Conventional Commits: `https://www.conventionalcommits.org/`
27. cargo-deny: `https://embarkstudios.github.io/cargo-deny/`
28. WCAG 2.1 AA: `https://www.w3.org/TR/WCAG21/`
29. GNOME Human Interface Guidelines: `https://developer.gnome.org/hig/`
30. Flatpak: `https://flatpak.org/`

---

*Document version: v1.4 | Date: 2026-07-10 | Format: Markdown*
