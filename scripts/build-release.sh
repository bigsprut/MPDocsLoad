#!/usr/bin/env bash
# Сборка release-дистрибутива MDWF для Windows (спец. §3.2, ЭТАП 14).
#
# Что делает:
#   1. Подготавливает окружение (MSYS2/gnu).
#   2. Собирает GUI и CLI в release-профиле.
#   3. Копирует GTK-рантайм DLL рядом с .exe (~70 MB bundle).
#   4. Формирует dist/mdwf/ с готовым к запуску приложением.
#
# Использование:
#   ./scripts/build-release.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MSYS2_MINGW_BIN="${MSYS2_MINGW_BIN:-D:/msys64/mingw64/bin}"
DIST="$REPO_ROOT/dist"
DIST_APP="$DIST/mdwf"

echo "=== [1/5] Подготовка окружения ==="
export PATH="$MSYS2_MINGW_BIN:$PATH"
export PKG_CONFIG_PATH="${MSYS2_MINGW_BIN%/bin}/lib/pkgconfig"
export RUSTUP_TOOLCHAIN="stable-x86_64-pc-windows-gnu"

pkg-config --modversion gtk4 || { echo "gtk4 не найден через pkg-config"; exit 1; }

echo "=== [2/5] Release-сборка GUI и CLI ==="
cd "$REPO_ROOT"
cargo build --release -p mdwf-gui -p mdwf-cli

echo "=== [3/5] Подготовка dist/ ==="
rm -rf "$DIST"
mkdir -p "$DIST_APP"

cp "$REPO_ROOT/target/x86_64-pc-windows-gnu/release/mdwf-gui.exe" "$DIST_APP/"
cp "$REPO_ROOT/target/x86_64-pc-windows-gnu/release/mdwf.exe" "$DIST_APP/"
cp "$REPO_ROOT/README.md" "$DIST_APP/"

echo "=== [4/5] Копирование GTK-рантайма (DLL, schemas, pixbuf loaders) ==="
# Минимальный набор DLL для запуска GTK4/libadwaita-приложения.
# Список может расширяться; используем копирование по маске нужных библиотек.
GTK_DLLS=(
    libgtk-4-1.dll
    libadwaita-1-0.dll
    libglib-2.0-0.dll
    libgobject-2.0-0.dll
    libgio-2.0-0.dll
    libpango-1.0-0.dll
    libcairo-2.dll
    libgdk_pixbuf-2.0-0.dll
    libharfbuzz-0.dll
    libgraphene-1.0-0.dll
    libpangocairo-1.0-0.dll
    libffi-8.dll
    libintl-8.dll
    libpcre2-8-0.dll
    libpng16-16.dll
    libfreetype-6.dll
    libfontconfig-1.dll
    libepoxy-0.dll
    libzstd.dll
    libbz2-1.dll
    libexpat-1.dll
    libgcc_s_seh-1.dll
    libstdc++-6.dll
    libwinpthread-1.dll
    libxml2-2.dll
    zlib1.dll
)

copied=0
for dll in "${GTK_DLLS[@]}"; do
    src="$MSYS2_MINGW_BIN/$dll"
    if [[ -f "$src" ]]; then
        cp "$src" "$DIST_APP/"
        copied=$((copied + 1))
    fi
done
echo "  скопировано DLL: $copied"

# GTK schemas + gdk-pixbuf loaders (для корректного рендеринга).
mkdir -p "$DIST_APP/share/glib-2.0/schemas"
cp -r "$MSYS2_MINGW_BIN/../share/glib-2.0/schemas/." "$DIST_APP/share/glib-2.0/schemas/" 2>/dev/null || true
mkdir -p "$DIST_APP/lib/gdk-pixbuf-2.0"
cp -r "$MSYS2_MINGW_BIN/../lib/gdk-pixbuf-2.0/." "$DIST_APP/lib/gdk-pixbuf-2.0/" 2>/dev/null || true

# Компилируем схемы в bundle.
glib-compile-schemas "$DIST_APP/share/glib-2.0/schemas/" 2>/dev/null || true

echo "=== [5/5] Готово ==="
du -sh "$DIST_APP" 2>/dev/null || true
ls "$DIST_APP"/*.exe
echo ""
echo "Распространяемая папка: $DIST_APP"
echo "Запуск: $DIST_APP/mdwf-gui.exe"
