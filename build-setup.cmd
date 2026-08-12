@echo off
rem ===========================================================================
rem  build-setup.cmd - build the MDWF release installer in one click.
rem
rem  Run: double-click this file, or from cmd/PowerShell:  build-setup.cmd
rem
rem  What it does: finds bash (MSYS2 or Git Bash) and runs scripts/build-setup.sh,
rem  which builds the bundle (build-release.sh) and compiles the Inno Setup installer.
rem  Result: installer\Output\MDWFSetup-<version>.exe
rem
rem  NOTE: the project is built with the MSYS2/MinGW toolchain (GTK libraries,
rem  ntldd, glib-compile-schemas), so MSYS2 (D:\msys64) or Git Bash is required.
rem  This is Windows, not Linux - "bash" here is only the MSYS2 shell.
rem  Messages are in English on purpose: cmd reads .bat/.cmd in the OEM codepage,
rem  so Cyrillic in this file would garble. Russian output comes from the .sh.
rem ===========================================================================

setlocal enableextensions
cd /d "%~dp0"
chcp 65001 >nul

echo ============================================================
echo   MDWF: build release installer
echo ============================================================

rem --- find bash.exe (MSYS2 first, then Git Bash, then PATH) ---
set "BASH="
if exist "D:\msys64\usr\bin\bash.exe" set "BASH=D:\msys64\usr\bin\bash.exe"
if not defined BASH if exist "C:\msys64\usr\bin\bash.exe" set "BASH=C:\msys64\usr\bin\bash.exe"
if not defined BASH if exist "%ProgramFiles%\Git\bin\bash.exe" set "BASH=%ProgramFiles%\Git\bin\bash.exe"
if not defined BASH if exist "%ProgramFiles(x86)%\Git\bin\bash.exe" set "BASH=%ProgramFiles(x86)%\Git\bin\bash.exe"
if not defined BASH (
    where bash >nul 2>&1 && for /f "delims=" %%i in ('where bash') do if not defined BASH set "BASH=%%i"
)

if not defined BASH (
    echo.
    echo [ERROR] bash not found. Need MSYS2 ^(https://www.msys2.org/^) or Git for Windows.
    echo         MSYS2 expected at D:\msys64.
    echo.
    pause
    exit /b 1
)

echo bash: %BASH%
echo.

rem --- run the build ---
"%BASH%" scripts/build-setup.sh
set "RC=%ERRORLEVEL%"

echo.
if "%RC%"=="0" (
    echo [OK] Done. Installer: installer\Output\MDWFSetup-*.exe
) else (
    echo [ERROR] Build failed ^(code %RC%^). See output above.
)

echo.
pause
exit /b %RC%
