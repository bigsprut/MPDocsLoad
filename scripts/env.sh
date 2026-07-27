#!/usr/bin/env bash
# Подготавливает окружение для сборки/запуска MDWF на Windows (Git Bash).
# Источник: scripts/env.sh
#
# Использование:
#   source scripts/env.sh        # экспортирует переменные в текущий шелл
#   ./scripts/env.sh cargo build # или запускает команду с нужным окружением
#
# Что делает:
#   1. Добавляет D:/msys64/mingw64/bin в PATH (gcc, pkg-config, gtk DLL).
#   2. Устанавливает PKG_CONFIG_PATH на pkgconfig из MSYS2.
#   3. Пинит Rust-тулчейн на stable-x86_64-pc-windows-gnu (см. rust-toolchain.toml).

set -euo pipefail

MDWF_MSYS2_MINGW_BIN="${MDWF_MSYS2_MINGW_BIN:-D:/msys64/mingw64/bin}"

if [ ! -d "$MDWF_MSYS2_MINGW_BIN" ]; then
  echo "[env] MSYS2 mingw64 не найден в $MDWF_MSYS2_MINGW_BIN" >&2
  echo "[env] Установите MSYS2 + пакеты gtk4/libadwaita или задайте MDWF_MSYS2_MINGW_BIN" >&2
  exit 1
fi

export PATH="$MDWF_MSYS2_MINGW_BIN:$PATH"
export PKG_CONFIG_PATH="${MDWF_MSYS2_MINGW_BIN%/bin}/lib/pkgconfig"
# gnu-тулчейн нужен для линковки с MinGW-сборкой GTK.
export RUSTUP_TOOLCHAIN="stable-x86_64-pc-windows-gnu"

if [ "$#" -gt 0 ]; then
  exec "$@"
fi
