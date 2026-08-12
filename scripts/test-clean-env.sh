#!/usr/bin/env bash
# Симуляция «чистой Windows»: запуск бандла со скрабленным окружением.
#
# Цель: проверить, что dist/mdwf/mdwf-gui.exe запускается БЕЗ MSYS2 в PATH и
# БЕЗ глобальных GTK-переменных — т.е. в условиях, максимально близких к чистой
# Windows, куда установлен инсталлятор. Дев-машина «欺骗но» работает, потому что
# GTK4 стоит глобально в D:\msys64\mingw64; чистая машина этого не имеет.
#
# Что скрабится:
#   - PATH: ОСТАВЛЯЕМ ТОЛЬКО системные Windows-директории (System32 и т.п.),
#     ВЫРЕЗАЕМ всё, что содержит msys64 / mingw / Git / usr/bin.
#   - GTK_* / GDK_* / GSETTINGS_* / FONTCONFIG_* / XDG_* / GLIB_* → unset.
#   - APPDATA/USERPROFILE/LOCALAPPDATA → temp-директория (чистый конфиг, как на
#     новой машине: нет mdwf.db/config.toml).
# Дев-машина «видит» бандл как чужую установку.
#
# Использование: ./scripts/test-clean-env.sh
# Выход: код 124 (timeout убил) = ОК (приложение запустилось и работало до kill).
#        любой другой = проблема (краш / не стартануло). Лог — в $LOG.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="$REPO_ROOT/dist/mdwf/mdwf-gui.exe"
TMP="$REPO_ROOT/dist/_clean_test"
LOG="$TMP/run.log"

[[ -f "$APP" ]] || { echo "Бандл не найден: $APP (сначала scripts/build-release.sh)"; exit 2; }

# ⚠️ КРИТИЧНО: убить любой ранее оставленный mdwf-gui.exe ДО запуска. GNU timeout
# в MSYS НЕ убивает native Windows GUI-процесс (SIGTERM не доходит), и «зомби»
# остаётся жить как primary-instance gtk::Application (фиксированный APP_ID).
# Следующий запуск увидит живой primary → форвард activate → НЕМЕДЛЕННО выйдет
# (exit 0, без окна) — ложный «краш». taskkill /F — единственный надёжный уборщик.
taskkill //IM mdwf-gui.exe //F 2>/dev/null && echo "(убит ранее оставленный процесс)" || true

rm -rf "$TMP"
mkdir -p "$TMP/AppData/Roaming" "$TMP/AppData/Local" "$TMP/Home"

# Скраб PATH: оставляем только Windows-системные пути. Разделитель в Git Bash
# PATH — «:», но пути выглядят как /c/Windows/... Нужно отфильтровать.
SYSROOT_UNIX="$(cygpath -u "$SYSTEMROOT" 2>/dev/null || echo /c/Windows)"
CLEAN_PATH="$SYSROOT_UNIX/System32:$SYSROOT_UNIX:$SYSROOT_UNIX/System32/Wbem:$SYSROOT_UNIX/System32/WindowsPowerShell/v1.0"

echo "=== Симуляция чистой Windows ==="
echo "exe:      $APP"
echo "скраб PATH: $CLEAN_PATH"
echo "temp дом: $TMP/Home"
echo ""

# Запуск со скрабленным env. env -u снимает переменные; переопределяем нужные.
# timeout 8 — GUI стартует, открывает окно, мы его убиваем. 124 = успех.
timeout 8 env \
  -i \
  PATH="$CLEAN_PATH" \
  SYSTEMROOT="$SYSTEMROOT" \
  TEMP="$TMP" \
  TMP="$TMP" \
  APPDATA="$TMP/AppData/Roaming" \
  LOCALAPPDATA="$TMP/AppData/Local" \
  USERPROFILE="$TMP/Home" \
  HOMEDRIVE="C:" \
  HOMEPATH="$TMP/Home" \
  USERNAME="mdwf_test" \
  PATHEXT=".COM;.EXE;.BAT;.CMD" \
  NUMBER_OF_PROCESSORS="$NUMBER_OF_PROCESSORS" \
  PROCESSOR_ARCHITECTURE="$PROCESSOR_ARCHITECTURE" \
  RUST_LOG="debug" \
  "$APP" >"$LOG" 2>&1
CODE=$?

echo "=== Результат ==="
echo "exit code: $CODE"
if [[ $CODE -eq 124 ]]; then
  echo "✅ ОК — приложение запустилось и работало до kill по timeout (как чистая Windows)"
else
  echo "❌ ПРОБЛЕМА — приложение не запустилось или упало (код $CODE)"
fi
echo ""
echo "=== Лог (первые 80 строк) ==="
head -80 "$LOG" 2>/dev/null
echo ""
echo "=== Лог: строки с ошибками/предупреждениями GTK ==="
grep -iE "error|fail|not found|cannot|missing|warning|critical|GLib-GObject|GTK|No such|unable|segmentation|exception" "$LOG" 2>/dev/null | head -40 || echo "(grep ничего не нашёл)"
echo ""
echo "Полный лог: $LOG"
