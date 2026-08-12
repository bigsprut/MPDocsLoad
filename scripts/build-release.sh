#!/usr/bin/env bash
# Сборка release-дистрибутива MDWF для Windows (спец. §3.2, ЭТАП 14).
#
# Что делает:
#   1. Подготавливает окружение (MSYS2/gnu).
#   2. Собирает GUI и CLI в release-профиле.
#   3. Копирует GTK-рантайм: ВСЕ DLL-зависимости через `ntldd -R` (рекурсивно,
#      а не хардкод-список), иконки (Adwaita/hicolor), gsettings-схемы,
#      gdk-pixbuf-лоадеры + инструменты для их пересборки инсталлятором.
#   4. Формирует dist/mdwf/ — готовый к запуску relocatable-бандл.
#
# ВАЖНО: приложение relocatable — main.rs сам настраивает env (XDG_DATA_DIRS,
# GDK_PIXBUF_MODULE_FILE) на соседние share/lib. Инсталлятор (installer/mdwf.iss)
# при установке пересобирает loaders.cache и схемы под путь установки.
#
# Использование: ./scripts/build-release.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MSYS2_MINGW_BIN="${MSYS2_MINGW_BIN:-D:/msys64/mingw64/bin}"
MSYS_ROOT="${MSYS2_MINGW_BIN%/bin}"
DIST="$REPO_ROOT/dist"
DIST_APP="$DIST/mdwf"

echo "=== [1/6] Подготовка окружения ==="
export PATH="$MSYS2_MINGW_BIN:$PATH"
export PKG_CONFIG_PATH="$MSYS_ROOT/lib/pkgconfig"
export RUSTUP_TOOLCHAIN="stable-x86_64-pc-windows-gnu"

pkg-config --modversion gtk4 || { echo "gtk4 не найден через pkg-config"; exit 1; }
command -v ntldd >/dev/null || { echo "ntldd не найден (нужен для сбора DLL)"; exit 1; }

echo "=== [2/6] Release-сборка GUI и CLI ==="
cd "$REPO_ROOT"
cargo build --release -p mdwf-gui -p mdwf-cli

echo "=== [3/6] Подготовка dist/ ==="
rm -rf "$DIST"
mkdir -p "$DIST_APP"
cp "$REPO_ROOT/target/x86_64-pc-windows-gnu/release/mdwf-gui.exe" "$DIST_APP/"
cp "$REPO_ROOT/target/x86_64-pc-windows-gnu/release/mdwf.exe" "$DIST_APP/"
cp "$REPO_ROOT/README.md" "$DIST_APP/" 2>/dev/null || true

echo "=== [4/6] Сбор DLL-зависимостей (ntldd -R, рекурсивно) ==="
# Собираем ВСЕ транзитивные зависимости обоих exe, фильтруем до mingw (отсекаем
# системные kernel32 и т.п.), копируем рядом с exe. Заменяет хардкод-список DLL.
{
    ntldd -R "$DIST_APP/mdwf-gui.exe" 2>/dev/null
    ntldd -R "$DIST_APP/mdwf.exe" 2>/dev/null
} | grep -i mingw \
  | sed -E 's|.*=>[[:space:]]*||; s|[[:space:]]*\(.*||' \
  | sort -u > "$DIST/_deps.txt"
dll_count=0
while IFS= read -r dll; do
    if [[ -f "$dll" ]]; then
        cp -n "$dll" "$DIST_APP/" && dll_count=$((dll_count + 1))
    fi
done < "$DIST/_deps.txt"
rm -f "$DIST/_deps.txt"
echo "  скопировано DLL: $dll_count"

echo "=== [5/6] Иконки (Adwaita/hicolor), gsettings-схемы, gdk-pixbuf-лоадеры ==="
# Иконки стандартной темы (стрелки/контролы libadwaita).
mkdir -p "$DIST_APP/share/icons"
cp -r "$MSYS_ROOT/share/icons/Adwaita" "$DIST_APP/share/icons/" 2>/dev/null \
    || echo "  WARN: Adwaita не найден"
cp -r "$MSYS_ROOT/share/icons/hicolor" "$DIST_APP/share/icons/" 2>/dev/null || true

# Иконка ПРИЛОЖЕНИЯ «mdwf» в on-disk теме hicolor — для GtkWindow::set_default_icon_name.
# PNG, отрендеренные из app-icon.svg (crates/gui/resources/icons/<size>x<size>/apps/).
# hicolor/index.theme уже объявляет <size>x<size>/apps, так что имя «mdwf» резолвится.
# (GTK4 add_resource_path + gresource на Windows не подхватывает иконку — только disk.)
for s in 16 32 48 64 128 256; do
    src="$REPO_ROOT/crates/gui/resources/icons/${s}x${s}/apps/mdwf.png"
    [[ -f "$src" ]] && cp "$src" \
        "$DIST_APP/share/icons/hicolor/${s}x${s}/apps/mdwf.png" 2>/dev/null || true
done
icon_added=$(find "$DIST_APP/share/icons/hicolor" -name mdwf.png 2>/dev/null | wc -l)
echo "  app-icon 'mdwf' sizes в hicolor: $icon_added"

# Перегенерировать icon-theme.cache темы hicolor. Исходный кэш скопирован из MSYS2
# и НЕ содержит «mdwf» → без этого GtkWindow::set_default_icon_name("mdwf") даёт
# has_icon=false (GTK доверяет кэшу и не сканит директории → брендовая иконка не
# ставится на окно). Регенерируем (включая mdwf); если тулза недоступна — удаляем
# кэш (GTK fallback на скан директорий, проверено: has_icon=true).
if gtk4-update-icon-cache --force "$DIST_APP/share/icons/hicolor/" >/dev/null 2>&1 \
    || gtk-update-icon-cache --force "$DIST_APP/share/icons/hicolor/" >/dev/null 2>&1; then
    echo "  icon-theme.cache hicolor перегенерирован --force (включает mdwf)"
else
    rm -f "$DIST_APP/share/icons/hicolor/icon-theme.cache"
    echo "  icon-theme.cache hicolor удалён (GTK fallback на скан директорий)"
fi

# gsettings-схемы (GTK/libadwaita настройки). Компилируем в бандл.
mkdir -p "$DIST_APP/share/glib-2.0/schemas"
cp "$MSYS_ROOT/share/glib-2.0/schemas/"*.xml "$DIST_APP/share/glib-2.0/schemas/" 2>/dev/null || true
glib-compile-schemas "$DIST_APP/share/glib-2.0/schemas/" 2>/dev/null || true

# gdk-pixbuf-лоадеры (PNG/JPEG/SVG/… для рендера изображений) + cache.
# Лоадеры лежат в поддиректории loaders/ (не в самом 2.10.0/). Cache генерируется
# под пути бандла; инсталлятор (postinstall.bat) пересоберёт под {app}.
mkdir -p "$DIST_APP/lib/gdk-pixbuf-2.0/2.10.0/loaders"
cp "$MSYS_ROOT/lib/gdk-pixbuf-2.0/2.10.0/loaders/"*.dll \
    "$DIST_APP/lib/gdk-pixbuf-2.0/2.10.0/loaders/" 2>/dev/null || true
gdk-pixbuf-query-loaders "$DIST_APP/lib/gdk-pixbuf-2.0/2.10.0/loaders/"*.dll \
    > "$DIST_APP/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache" 2>/dev/null || true

# Инструменты для пересборки loaders.cache/схем при установке (Inno [Run]).
cp "$MSYS2_MINGW_BIN/gdk-pixbuf-query-loaders.exe" "$DIST_APP/" 2>/dev/null || true
cp "$MSYS2_MINGW_BIN/glib-compile-schemas.exe" "$DIST_APP/" 2>/dev/null || true

# postinstall.bat: пересобирает loaders.cache и gsettings-схемы под АБСОЛЮТНЫЙ
# путь установки (%~dp0 = каталог этого .bat = {app}). Бандл relocatable, поэтому
# пути из build-time кэша (dist/) на прод-машине нерелевантны — пересобираем.
# Запускается инсталлятором (Inno [Run]) молча после копирования файлов.
cat > "$DIST_APP/postinstall.bat" <<'BAT'
@echo off
setlocal
set "APPDIR=%~dp0"
set "LDIR=%APPDIR%lib\gdk-pixbuf-2.0\2.10.0"
set "SDIR=%APPDIR%share\glib-2.0\schemas"
rem Сбор абсолютных путей лоадеров (cmd не раскрывает glob в аргументах exe).
rem Лоадеры в поддиректории loaders/.
set "ARGS="
for %%f in ("%LDIR%\loaders\*.dll") do call set "ARGS=%%ARGS%% "%%f""
if exist "%APPDIR%gdk-pixbuf-query-loaders.exe" (
    "%APPDIR%gdk-pixbuf-query-loaders.exe" %ARGS% > "%LDIR%\loaders.cache"
)
if exist "%APPDIR%glib-compile-schemas.exe" (
    "%APPDIR%glib-compile-schemas.exe" "%SDIR%"
)
endlocal
BAT
unix2dos "$DIST_APP/postinstall.bat" 2>/dev/null || sed -i 's/$/\r/' "$DIST_APP/postinstall.bat"

echo "=== [6/6] Готово ==="
echo "Размер бандла:"
du -sh "$DIST_APP" 2>/dev/null || true
echo "exe:"
ls "$DIST_APP"/*.exe
echo ""
echo "Распространяемая папка: $DIST_APP"
echo "Запуск:                 $DIST_APP/mdwf-gui.exe"
echo "Инсталлятор:            iscc installer/mdwf.iss  (нужен Inno Setup)"
