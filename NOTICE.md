# NOTICE — сторонние компоненты

MDWF распространяется по MIT OR Apache-2.0 (см. `LICENSE-MIT`,
`LICENSE-Apache-2.0`). Дистрибутив (бандл `dist/mdwf/`, инсталлер
`MDWFSetup-*.exe`) включает следующие сторонние компоненты:

## Графические ассеты

| Компонент | Лицензия | Источник | Использование |
|---|---|---|---|
| vscode-icons | MIT | github.com/vscode-icons/vscode-icons | PNG-иконки типов файлов (Excel/PDF/JSON/XML/ZIP/TXT) в GUI |
| Adwaita Icon Theme | LGPL-3.0-or-later / CC-BY-SA (icons) | gnome.org (пакет MSYS2 `mingw-w64-x86_64-adwaita-icon-theme`) | symbolic-иконки кнопок GUI |
| hicolor Icon Theme | CC-BY-SA / GPL-2.0+ (cache-утилита) | freedesktop.org (пакет MSYS2) | базовая тема иконок бандла |

Требования permissive-лицензий (сохранение уведомления о копирайте)
удовлетворяются настоящим NOTICE.

## GTK-рантайм (DLL в бандле, пакеты MSYS2 mingw-w64-x86_64-*)

| Компонент | Лицензия |
|---|---|
| GTK 4 | LGPL-2.1-or-later |
| libadwaita | LGPL-2.1-or-later |
| GLib / GDK-PixBuf / Pango / HarfBuzz | LGPL-2.1-or-later |
| Cairo | LGPL-2.1-or-later / MPL-1.1 |
| FreeType | FreeType License (BSD-style) |
| Fontconfig | MIT-style |

LGPL-компоненты линкуются динамически (отдельные DLL в бандле) — исходные
тексты доступны у их проектов; замена DLL на модифицированные разрешена
условиями LGPL.

## Инструменты сборки дистрибутива

| Компонент | Лицензия | Использование |
|---|---|---|
| Inno Setup 7 | Inno Setup License (свободная) | компиляция инсталлера (`installer/mdwf.iss`) |
| rsvg-convert (librsvg) | LGPL-2.1-or-later | конвертация SVG→PNG иконок при сборке |
| winres/windres (GNU binutils) | GPL-3.0 (инструмент сборки, в дистрибутив не входит) | встраивание .ico/.rsrc в exe |

## Rust-зависимости (статическая линковка)

Все крейты-зависимости (`Cargo.lock`) — permissive: MIT, Apache-2.0 (или
двойная MIT/Apache-2.0), BSD, ISC, MPL-2.0 (например, `webpki-roots`).
Полный список с версиями — `Cargo.lock`; лицензии каждого крейта — поле
`license` на crates.io.

## Документация API маркетплейсов

`docs/ozon-seller-api-reference.md` — локальная копия официальной
документации Ozon (docs.ozon.ru), используется для сверы эндпоинтов;
авторские права Ozon. Спецификации Wildberries сверяются с зеркалом
`github.com/eslazarev/wildberries-sdk` (OpenAPI, генерируется из
dev.wildberries.ru).
