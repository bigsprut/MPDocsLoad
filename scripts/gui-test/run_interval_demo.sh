#!/usr/bin/env bash
# Демо/шаблон GUI клик-теста: «Загрузка» → «📅 Интервал» → вкладка «Месяц» →
# «Март» → проверка, что date_from/date_to заполнились (OCR экрана + SQLite).
#
# Инструментарий: shot.ps1 (скриншот, DPI-aware), ocr.ps1 (Windows OCR, UTF-8,
# прямоугольники слов), focus.ps1 (надёжный SetForegroundWindow), click.ps1
# (SendInput). Все координаты — физические пиксели экрана, согласованы между
# скриптами (SetProcessDPIAware в каждом).
#
# Запуск: bash scripts/gui-test/run_interval_demo.sh [путь-к-exe]
set -u
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
GT="$ROOT/dist/_gt"
EXE="${1:-$ROOT/target/x86_64-pc-windows-gnu/debug/mdwf-gui.exe}"
mkdir -p "$GT"

# ⚠️ Тест КЛИКАЕТ по РЕАЛЬНОМУ рабочему столу и переводит окно приложения на
# передний план. НЕ запускайте, пока работаете за машиной (клики могут попасть
# в ваши окна). Запускайте, когда рабочий стол свободен.
trap 'taskkill //IM mdwf-gui.exe //F >/dev/null 2>&1' EXIT

PSC="powershell.exe -NoProfile -ExecutionPolicy Bypass -File"
PSCMD="powershell.exe -NoProfile -ExecutionPolicy Bypass -Command"
SHOT="$(cygpath -w "$ROOT/scripts/gui-test/shot.ps1")"
OCRW="$(cygpath -w "$ROOT/scripts/gui-test/ocr.ps1")"
CLK="$(cygpath -w "$ROOT/scripts/gui-test/click.ps1")"
FOCUS="$(cygpath -w "$ROOT/scripts/gui-test/focus.ps1")"
GTMARK="GUI-TEST"

# Найти слово в OCR-выводе → "X Y" (центр). Использование: fw Слово файл
fw() { awk -F'|' -v w="$1" '$1==w {printf "%d %d", $2+$4/2, $3+$5/2; exit}' "$2"; }

click_word() { # click_word Слово файл-ocr [описание]
    local c; c="$(fw "$1" "$2")"
    if [ -z "$c" ]; then echo "$GTMARK FAIL: слово «$1» не найдено ($3)"; return 1; fi
    echo "$GTMARK click «$1» @ ${c% *} , ${c#* }"
    $PSC "$CLK" -X "${c% *}" -Y "${c#* }" -Hwnd "$HWND"
}

shot_ocr() { # shot_ocr имя → пишет имя.png + имя.txt (слова)
    $PSC "$FOCUS" -Hwnd "$HWND" | grep -o "FOREGROUND=True" || echo "$GTMARK WARN: окно не на переднем плане"
    $PSC "$SHOT" -Out "$(cygpath -w "$GT/$1.png")" >/dev/null
    $PSC "$OCRW" "$(cygpath -w "$GT/$1.png")" -Words > "$GT/$1.txt"
}

# --- 0. Запуск приложения ---
taskkill //IM mdwf-gui.exe //F >/dev/null 2>&1; sleep 1
( timeout 300 "$EXE" > "$GT/demo_run.log" 2>&1 ) &
sleep 7
HWND=$($PSCMD "(Get-Process mdwf-gui -ErrorAction SilentlyContinue).MainWindowHandle" | tr -d '\r' | tail -1)
[ -n "$HWND" ] && [ "$HWND" != "0" ] || { echo "$GTMARK FAIL: приложение не запустилось"; exit 1; }
echo "$GTMARK HWND=$HWND"

# --- 1. Вкладка «Загрузка» ---
shot_ocr step1
click_word "Загрузка" "$GT/step1.txt" "сайдбар" || exit 1
sleep 1.5

# --- 2. Кнопка «📅 Интервал» ---
shot_ocr step2
click_word "Интервал" "$GT/step2.txt" "кнопка интервала" || exit 1
sleep 1.5

# --- 3. Вкладка «Месяц» в пикере ---
shot_ocr step3
grep -E '^(Неделя|Месяц|Квартал|Год)\|' "$GT/step3.txt" | head -4
click_word "Месяц" "$GT/step3.txt" "вкладка пикера" || exit 1
sleep 1.2

# --- 4. Кнопка «Март» ---
shot_ocr step4
click_word "Март" "$GT/step4.txt" "месяц в сетке" || exit 1
sleep 1.5

# --- 5. Верификация: даты на экране ---
$PSC "$FOCUS" -Hwnd "$HWND" >/dev/null
$PSC "$SHOT" -Out "$(cygpath -w "$GT/final.png")" >/dev/null
$PSC "$OCRW" "$(cygpath -w "$GT/final.png")" > "$GT/final.txt"
echo "$GTMARK даты на экране: $(grep -oE '202[0-9]-[0-9]{2}-[0-9]{2}' "$GT/final.txt" | sort -u | tr '\n' ' ')"

# --- 6. Верификация: ui_state в SQLite ---
DB="$APPDATA/mdwf/mdwf.db"
[ -f "$DB" ] || DB="C:/Users/$USERNAME/AppData/Roaming/mdwf/mdwf.db"
if [ -f "$DB" ]; then
    python3 - "$DB" <<'PYEOF'
import json, sqlite3, sys
db = sqlite3.connect(sys.argv[1])
row = db.execute("select value from ui_state where key='download_screen'").fetchone()
st = json.loads(row[0]) if row else {}
print("GUI-TEST ui_state date_from=%r date_to=%r" % (st.get("date_from"), st.get("date_to")))
PYEOF
else
    echo "$GTMARK WARN: БД не найдена"
fi

taskkill //IM mdwf-gui.exe //F >/dev/null 2>&1
echo "$GTMARK DONE (скриншоты/OCR: $GT)"
