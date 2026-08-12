#!/usr/bin/env bash
# Симуляция полного flow'а установки на чистой машине.
#
# Что делает РЕАЛЬНЫЙ инсталлятор (installer/mdwf.iss, [Files]+[Run]):
#   1. Копирует бандл dist/mdwf/* в {app} (напр. C:\Program Files\MDWF).
#   2. Запускает postinstall.bat → пересобирает loaders.cache (абсолютные пути
#      к лоадерам) и gsettings-схемы под путь установки.
#   3. Пользователь запускает mdwf-gui.exe (или ярлык).
#
# Этот скрипт воспроизводит все 3 шага в путях С ПРОБЕЛОМ (как Program Files),
# затем запускает exe со скрабленным env. Это самый близкий к реальности тест
# без фактической установки (не требует прав админа).
#
# Использование: ./scripts/test-installed.sh

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$REPO_ROOT/dist/mdwf"
# Путь с пробелом — имитация «Program Files». Используем Windows-temp.
INST_ROOT_WIN="C:\\Users\\$USERNAME\\AppData\\Local\\Temp\\MDWF Install Sim"
INST_ROOT_UNIX="$(cygpath -u "$INST_ROOT_WIN")"
LOG="$INST_ROOT_UNIX/../_install_test.log"

[[ -d "$SRC" ]] || { echo "Бандл не найден: $SRC (сначала scripts/build-release.sh)"; exit 2; }

# ⚠️ КРИТИЧНО (урок №36): taskkill любого mdwf-gui.exe ДО запуска. GNU timeout
# в MSYS не убивает native Windows GUI-процесс → «зомби» остаётся как primary-
# instance gtk::Application (APP_ID) → следующий запуск форвардит activate и
# немедленно выходит (exit 0) — ложный сбой. См. также scripts/test-clean-env.sh.
taskkill //IM mdwf-gui.exe //F 2>/dev/null && echo "(убит ранее оставленный процесс)" || true

echo "=== [1/4] Копирование бандла → путь с пробелом ==="
echo "  src:  $SRC"
echo "  dst:  $INST_ROOT_WIN"
rm -rf "$INST_ROOT_UNIX"
mkdir -p "$INST_ROOT_UNIX"
cp -r "$SRC"/* "$INST_ROOT_UNIX"/
echo "  скопировано: $(ls "$INST_ROOT_UNIX" | wc -l) элементов верхнего уровня"

echo ""
echo "=== [2/4] Проверка ДО postinstall: loaders.cache содержит build-time пути? ==="
head -6 "$INST_ROOT_UNIX/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache" | grep -E '\.dll"' || true
echo "  (выше — СТАРЫЕ пути D:/work/.../dist/mdwf/... — после установки они битые)"

echo ""
echo "=== [3/4] Запуск postinstall.bat (как делает Inno [Run]) ==="
cmd.exe //C "postinstall.bat" >/dev/null 2>&1
# postinstall.bat использует %~dp0 — должен запускаться из каталога установки.
# Команда выше запускается из $INST_ROOT_UNIX (cd сделан ниже). Сделаем явно:
( cd "$INST_ROOT_UNIX" && cmd.exe //C "postinstall.bat" ) >/dev/null 2>&1 && echo "  postinstall: exit 0" || echo "  postinstall: НЕ нулевой exit"

echo ""
echo "  loaders.cache ПОСЛЕ postinstall (должны быть НОВЫЕ пути с Install Sim):"
head -6 "$INST_ROOT_UNIX/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache" | grep -E '\.dll"' || echo "  (пути не обновились — ПРОБЛЕМА)"
NEW_PATHS=$(grep -c "Install Sim" "$INST_ROOT_UNIX/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache" || true)
echo "  строк с новым путём: $NEW_PATHS"

echo ""
echo "=== [4/4] Запуск mdwf-gui.exe со скрабленным env ==="
SYSROOT_UNIX="$(cygpath -u "$SYSTEMROOT" 2>/dev/null || echo /c/Windows)"
CLEAN_PATH="$SYSROOT_UNIX/System32:$SYSROOT_UNIX:$SYSROOT_UNIX/System32/Wbem"

timeout 8 env -i \
  PATH="$CLEAN_PATH" \
  SYSTEMROOT="$SYSTEMROOT" \
  TEMP="$INST_ROOT_UNIX" \
  TMP="$INST_ROOT_UNIX" \
  APPDATA="$INST_ROOT_UNIX" \
  RUST_LOG="debug" \
  "$INST_ROOT_UNIX/mdwf-gui.exe" >"$LOG" 2>&1
CODE=$?
echo "  exit code: $CODE"
if [[ $CODE -eq 124 ]]; then
  echo "  ✅ ОК — установленное приложение запустилось в пути с пробелом без MSYS2"
else
  echo "  ❌ ПРОБЛЕМА — код $CODE"
fi

echo ""
echo "=== Лог: предупреждения/ошибки ==="
grep -iE "error|fail|not found|cannot|missing|critical|GLib-GObject|GTK|No such|unable|segfault" "$LOG" 2>/dev/null | head -30 || echo "(чисто)"
echo ""
echo "Полный лог: $LOG"
