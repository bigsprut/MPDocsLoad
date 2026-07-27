# Подготавливает окружение для сборки/запуска MDWF на Windows (PowerShell).
# См. scripts/env.sh для эквивалента в Git Bash.
#
# Использование:
#   . .\scripts\env.ps1           # экспортирует переменные в сессию
#   . .\scripts\env.ps1; cargo build

$ErrorActionPreference = "Stop"

$MingwBin = if ($env:MDWF_MSYS2_MINGW_BIN) { $env:MDWF_MSYS2_MINGW_BIN } else { "D:\msys64\mingw64\bin" }

if (-not (Test-Path $MingwBin)) {
    Write-Error "[env] MSYS2 mingw64 не найден в $MingwBin. Установите MSYS2 + пакеты gtk4/libadwaita или задайте MDWF_MSYS2_MINGW_BIN."
}

$env:PATH = "$MingwBin;" + $env:PATH
$env:PKG_CONFIG_PATH = Join-Path (Split-Path $MingwBin -Parent) "lib\pkgconfig"
$env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-gnu"

Write-Host "[env] PATH/PKG_CONFIG_PATH/RUSTUP_TOOLCHAIN настроены для MDWF (MSYS2 mingw64 + gnu)."
