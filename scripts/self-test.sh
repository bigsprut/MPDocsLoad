#!/usr/bin/env bash
# Headless event-level self-test GUI (--self-test <scenario.json>).
# Прогоняет сценарий через тот же app-loop, что и живое окно, БЕЗ GTK/скриншотов/
# кликов; отчёт — рядом со сценарием (*.report.json). Exit 0 = PASS.
#
# Использование: bash scripts/self-test.sh scripts/selftest/smoke.json
# Сценарии: scripts/selftest/*.json (smoke — TestProvider, без сети;
# live_* — реальные API-профили, isolated=false).
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCENARIO="${1:-$REPO_ROOT/scripts/selftest/smoke.json}"

# env.sh при наличии позиционных аргументов делает exec "$@" — чистим их,
# иначе он исполнит путь сценария как команду.
set --
source "$REPO_ROOT/scripts/env.sh"

EXE="$REPO_ROOT/target/x86_64-pc-windows-gnu/debug/mdwf-gui.exe"
[[ -f "$EXE" ]] || EXE="$REPO_ROOT/target/x86_64-pc-windows-gnu/release/mdwf-gui.exe"
[[ -f "$EXE" ]] || { echo "Бинарник не найден — сначала cargo build -p mdwf-gui"; exit 2; }

# Живой GUI не мешает: --self-test не захватывает single-instance mutex.
echo "=== self-test: $SCENARIO ==="
"$EXE" --self-test "$SCENARIO"
CODE=$?
echo "=== exit: $CODE ==="
exit $CODE
