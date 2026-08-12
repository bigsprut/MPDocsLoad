; Inno Setup-скрипт для MDWF (спец. §3.2 — распространение).
;
; Сборка: сначала собрать бандл — ./scripts/build-release.sh (создаст dist/mdwf/),
; затем скомпилировать инсталлятор:
;   "C:\Program Files (x86)\Inno Setup 6\ISCC.exe" installer\mdwf.iss
; Результат: installer\Output\MDWFSetup-<version>.exe
;
; Что делает инсталлятор:
;   - Копирует relocatable-бандл в {autopf}\MDWF (Program Files).
;   - ПОСЛЕ копирования запускает postinstall.bat — пересобирает loaders.cache
;     и gsettings-схемы под абсолютный путь установки (бандл relocatable).
;   - Ярлык в меню Пуск (и опционально на рабочем столе).
;   - Деинсталлятор удаляет {app} (данные пользователя в %APPDATA%\mdwf остаются).

#define MyAppName      "MDWF"
#define MyAppVersion   "1.4.0"
#define MyAppPublisher "MDWF"
#define MyAppExeName   "mdwf-gui.exe"
#define BuildDir       "..\dist\mdwf"

[Setup]
AppId={{MDWF-1A2B-4C5D-9E0F-MDWF0001}}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
DisableWelcomePage=no
ArchitecturesInstallIn64BitMode=x64compatible
ArchitecturesAllowed=x64compatible
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}
OutputDir=Output
OutputBaseFilename=MDWFSetup-{#MyAppVersion}
; Файлы берём из собранного бандла (build-release.sh → dist/mdwf/).
PrivilegesRequiredOverridesAllowed=dialog

[Languages]
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Создать ярлык на рабочем столе"; GroupDescription: "Дополнительные ярлыки:"; Flags: unchecked

[Files]
; Весь бандл (exe, DLL, share/, lib/, инструменты, postinstall.bat).
Source: "{#BuildDir}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"
Name: "{group}\Деинсталлировать {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
; 1) Пересборка loaders.cache + gsettings-схем под путь установки (relocatable).
Filename: "{app}\postinstall.bat"; WorkingDir: "{app}"; Flags: runhidden; StatusMsg: "Настройка графических компонентов…"
; 2) Запустить приложение после установки (кроме тихой установки).
Filename: "{app}\{#MyAppExeName}"; Description: "Запустить {#MyAppName}"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Удаляем весь каталог установки (Inno сам удаляет установленные файлы; это
; подчищает сгенерированные loaders.cache и пр.). Данные в %APPDATA%\mdwf не трогаем.
Type: filesandordirs; Name: "{app}"
