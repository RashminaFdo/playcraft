; PlayCraft Windows Installer Script (Inno Setup 6)
; Installs ONLY the Windows PC Launcher (Android APK stays separate on GitHub Releases).
; Author: Rashmina Fernando (https://www.rashmina.dev)

#define MyAppName "PlayCraft"
#define MyAppVersion "1.0.0"
#define MyAppPublisher "Rashmina Fernando (www.rashmina.dev)"
#define MyAppURL "https://www.rashmina.dev"
#define MyAppReleasesURL "https://github.com/RashminaFdo/playcraft/releases"
#define MyAppExeName "PlayCraft.exe"

[Setup]
AppId={{B48C59B4-8E2D-491E-9C30-7A1E6B7D19A2}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} v{#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppReleasesURL}
AppUpdatesURL={#MyAppReleasesURL}
DefaultDirName={localappdata}\Programs\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
UsePreviousAppDir=yes
UsePreviousTasks=yes
PrivilegesRequired=lowest
LicenseFile=..\LICENSE
OutputDir=..\dist-bundle\PlayCraft-by-Rashmina-Fernando
OutputBaseFilename=PlayCraft-Setup-v{#MyAppVersion}
SetupIconFile=..\src-tauri\icons\icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
CloseApplications=force
CloseApplicationsFilter=*.exe
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\dist-bundle\PlayCraft-by-Rashmina-Fernando\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\ViGEmBus_1.22.0_x64_x86_arm64.exe"; DestDir: "{tmp}"; Flags: deleteafterinstall; Check: ViGEmBusMissing

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{tmp}\ViGEmBus_1.22.0_x64_x86_arm64.exe"; Description: "Install ViGEmBus Controller Driver (Required for Forza Gyro & Xbox Vibration)"; Check: ViGEmBusMissing; Flags: postinstall shellexec waituntilterminated skipifsilent
Filename: "{app}\{#MyAppExeName}"; Description: "Launch {#MyAppName}"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent

[Code]
procedure CloseRunningPlayCraft();
var
  ResultCode: Integer;
  TaskkillPath: String;
begin
  TaskkillPath := ExpandConstant('{sys}\taskkill.exe');
  Exec(TaskkillPath, '/F /IM PlayCraft.exe', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Exec(TaskkillPath, '/F /IM playcraft.exe', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Exec(TaskkillPath, '/F /IM PlayCraft-by-Rashmina-Fernando.exe', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Sleep(600);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  CloseRunningPlayCraft();
  Result := '';
end;

function InitializeUninstall(): Boolean;
begin
  CloseRunningPlayCraft();
  Result := True;
end;

function ViGEmBusMissing(): Boolean;
begin
  Result := not FileExists(ExpandConstant('{sys}\drivers\ViGEmBus.sys'));
end;

