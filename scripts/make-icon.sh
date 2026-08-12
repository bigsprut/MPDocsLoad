#!/usr/bin/env bash
# Сборка app-icon.ico из SVG (PNG-in-ICO, работает на Windows 10/11).
#
# Почему так: ни ImageMagick (`convert` — это Windows-FAT-утилита), ни Pillow,
# ни icotool в окружении нет. ICO-формат простой — собираем вручную: заголовок
# (6 байт) + директория (16 байт/изображение) + PNG-блобы.
#
# Запуск: bash scripts/make-icon.sh
# Источник: crates/gui/resources/app-icon.svg → crates/gui/resources/app-icon.ico

set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SVG="$REPO_ROOT/crates/gui/resources/app-icon.svg"
OUT="$REPO_ROOT/crates/gui/resources/app-icon.ico"
SIZES=(16 24 32 48 64 128 256)
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# 1. Рендер SVG → PNG каждого размера.
pngs=()
for s in "${SIZES[@]}"; do
    p="$TMP/$s.png"
    rsvg-convert -w "$s" -h "$s" "$SVG" -o "$p"
    pngs+=("$p")
done

# Хелперы little-endian.
le2() { printf '\\x%02x\\x%02x' "$(( $1 & 255 ))" "$(( ($1 >> 8) & 255 ))"; }
le4() { printf '\\x%02x\\x%02x\\x%02x\\x%02x' "$(( $1 & 255 ))" "$(( ($1 >> 8) & 255 ))" "$(( ($1 >> 16) & 255 ))" "$(( ($1 >> 24) & 255 ))"; }

# 2. Заголовок + директория (сначала в тексте с escape-последовательностями).
n=${#pngs[@]}
data_offset=$((6 + 16 * n))
entries=""
cur_offset=$data_offset
for i in "${!pngs[@]}"; do
    p="${pngs[$i]}"
    s="${SIZES[$i]}"
    sz=$(stat -c %s "$p")
    wb=$s; [[ $s -eq 256 ]] && wb=0
    # width, height, colorCount(0), reserved(0), planes(1), bpp(32), size, offset
    entries+="\\x$(printf '%02x' $wb)\\x$(printf '%02x' $wb)\\x00\\x00\\x01\\x00\\x20\\x00$(le4 "$sz")$(le4 "$cur_offset")"
    cur_offset=$((cur_offset + sz))
done

# 3. Сборка: заголовок + директория, затем PNG-блоби.
{
    printf '\x00\x00\x01\x00'        # reserved=0, type=1 (ICO)
    printf "$(le2 "$n")"             # count
    printf "%b" "$entries"
    for p in "${pngs[@]}"; do cat "$p"; done
} > "$OUT"

echo "Создан $OUT ($(stat -c %s "$OUT") байт, $n размеров: ${SIZES[*]})"
