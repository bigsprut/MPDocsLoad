#!/usr/bin/env bash
# Полная сборка релизного инсталлятора MDWF одним запуском:
#   1) бандл (./scripts/build-release.sh → dist/mdwf/, ~100 МБ);
#   2) компиляция Inno Setup-скрипта (installer/mdwf.iss → installer/Output/MDWFSetup-<ver>.exe).
#
# Запуск из Git Bash / MSYS2 в корне проекта:
#   bash scripts/build-setup.sh
#
# Требования: Rust + MSYS2/GTK (для бандла) и Inno Setup 6/7 (для .iss).
# Если Inno Setup не найден — скрипт подскажет, откуда скачать.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "============================================================"
echo "  Сборка релизного инсталлятора MDWF"
echo "============================================================"

echo ""
echo "=== [1/3] Бандл (build-release.sh) ==="
bash "$REPO_ROOT/scripts/build-release.sh"

echo ""
echo "=== [2/3] Поиск Inno Setup (ISCC.exe) ==="
ISCC=""
# Кандидаты во всех типичных расположениях (per-user и machine, Inno 6/7).
candidates=(
    "$LOCALAPPDATA/Programs/Inno Setup 7/ISCC.exe"
    "$LOCALAPPDATA/Programs/Inno Setup 6/ISCC.exe"
    "$USERPROFILE/AppData/Local/Programs/Inno Setup 7/ISCC.exe"
    "$USERPROFILE/AppData/Local/Programs/Inno Setup 6/ISCC.exe"
    "/c/Program Files/Inno Setup 7/ISCC.exe"
    "/c/Program Files (x86)/Inno Setup 7/ISCC.exe"
    "/c/Program Files/Inno Setup 6/ISCC.exe"
    "/c/Program Files (x86)/Inno Setup 6/ISCC.exe"
)
for c in "${candidates[@]}"; do
    if [[ -n "$c" && -f "$c" ]]; then
        ISCC="$c"
        break
    fi
done
# Запас: вдруг iscc в PATH.
if [[ -z "$ISCC" ]] && command -v iscc >/dev/null 2>&1; then
    ISCC="$(command -v iscc)"
fi

if [[ -z "$ISCC" ]]; then
    echo "ОШИБКА: Inno Setup (ISCC.exe) не найден."
    echo ""
    echo "Установи Inno Setup 6 или 7 (бесплатно):"
    echo "  https://jrsoftware.org/isdl.php"
    echo "Затем перезапусти этот скрипт."
    exit 1
fi
echo "  найден: $ISCC"

echo ""
echo "=== [3/3] Компиляция инсталлятора ==="
# Версия — единственный источник: [workspace.package] Cargo.toml.
# ВАЖНО: строка там С ОТСТУПОМ (секция) — прежде искали ^version без отступа
# (старый отдельный [package]); после унификации 3b6c462 grep молча давал
# пустоту → ISCC падал «more than one script filename».
APP_VERSION="$(grep -m1 '^[[:space:]]*version = "' "$REPO_ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')"
if [[ -z "$APP_VERSION" ]]; then
    echo "  ОШИБКА: версия не найдена в [workspace.package] Cargo.toml" >&2
    exit 1
fi
echo "  версия: $APP_VERSION"
# //D (не /D): MSYS/Git-Bash конвертирует аргументы вида /Dxxx в
# "C:/Program Files/Git/Dxxx" → ISCC видит второе «имя скрипта». Двойной
# слэш — штатный MSYS-эскейп (как cmd //c), до exe доходит /DMyAppVersion=….
"$ISCC" "//DMyAppVersion=$APP_VERSION" "$REPO_ROOT/installer/mdwf.iss"

echo ""
echo "============================================================"
echo "  ✅ Готово. Инсталлятор:"
echo "============================================================"
ls -lh "$REPO_ROOT/installer/Output/"*.exe 2>/dev/null | sed 's/^/  /'
echo ""
echo "Перенеси MDWFSetup-*.exe на целевую машину и запусти."
