#!/usr/bin/env bash
# Сборка app-icon.ico из SVG.
#
# ICO: малые размеры (16-128) как BMP/DIB, 256 как PNG-blob. Энкодер —
# scripts/ico_encode.py (чистый stdlib: zlib+struct). ПОЧЕМУ не all-PNG:
# Windows для малых значков (16/32/48 — таскбар/список/проводник) ждёт BMP/DIB;
# all-PNG ICO → Windows показывает ДЕФОЛТНУЮ иконку (не бренд), а .NET
# Icon-лоадер на ней падает. См. урок №43. ImageMagick/icotool/Pillow нет.
#
# Также: disk-hicolor PNG для GtkWindow::set_default_icon_name (build-release.sh).
#
# Запуск: bash scripts/make-icon.sh
# Источник: crates/gui/resources/app-icon.svg

set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SVG="$REPO_ROOT/crates/gui/resources/app-icon.svg"
OUT="$REPO_ROOT/crates/gui/resources/app-icon.ico"
SIZES=(16 24 32 48 64 128 256)
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# 1. Рендер SVG → PNG каждого размера.
for s in "${SIZES[@]}"; do
    rsvg-convert -w "$s" -h "$s" "$SVG" -o "$TMP/$s.png"
done

# 2. Кодирование ICO (малые = BMP/DIB, 256 = PNG) через Python-энкодер.
ICO_ARGS=()
for s in "${SIZES[@]}"; do ICO_ARGS+=("$TMP/$s.png"); done
python3 "$REPO_ROOT/scripts/ico_encode.py" "$OUT" "${ICO_ARGS[@]}"

# 3. Disk-hicolor PNG для GtkWindow::set_default_icon_name("mdwf").
# build-release.sh копирует их в share/icons/hicolor/<size>x<size>/apps/mdwf.png.
DISK_SIZES=(16 32 48 64 128 256)
DISK_BASE="$REPO_ROOT/crates/gui/resources/icons"
for s in "${DISK_SIZES[@]}"; do
    d="$DISK_BASE/${s}x${s}/apps"
    mkdir -p "$d"
    rsvg-convert -w "$s" -h "$s" "$SVG" -o "$d/mdwf.png"
done

echo "Готово: $OUT (DIB small + PNG 256) + disk-hicolor PNG (${DISK_SIZES[*]})"
echo "ВАЖНО: после смены — пересобрать бандл (gtk4-update-icon-cache --force) + GUI (winres)."
