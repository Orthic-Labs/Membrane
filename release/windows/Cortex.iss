; D18: per-user Cortex installer (Inno Setup).
; Places files under %LOCALAPPDATA%\Orthic\Cortex, adds the user PATH entry,
; records uninstall metadata, and optionally installs the user background task.

#define MyAppName "Cortex"
#define MyAppVersion "0.2.0"
#define MyAppId "io.orthic.cortex"

[Setup]
AppId={{8C5E9C51-9D4B-4E6A-9E2C-{MyAppVersion}}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher=Orthic Labs
DefaultDirName={localappdata}\Orthic\Cortex
PrivilegesRequired=lowest
OutputDir=.
OutputBaseFilename=Cortex-{#MyAppVersion}-Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Files]
Source: "..\..\staged\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs

[Registry]
Root: HKCU; Subkey: "Software\Orthic\Cortex"; ValueType: string; ValueName: "InstallDir"; ValueData: "{app}"; Flags: uninsdeletekey

[Tasks]
Name: "usertask"; Description: "Start the Cortex watcher at login (per-user background task)"; Flags: unchecked

[Run]
Filename: "{cmd}"; Parameters: "/c schtasks /Create /F /TN OrthicCortex /XML ""{app}\cortex-task.xml"""; Tasks: usertask; Flags: runhidden

[UninstallRun]
Filename: "{cmd}"; Parameters: "/c schtasks /Delete /F /TN OrthicCortex"; Flags: runhidden

[Code]
procedure CurStepChanged(CurStep: TSetupStep);
var
  UserPath: string;
begin
  if CurStep = ssPostInstall then
  begin
    if RegQueryStringValue(HKCU, 'Environment', 'Path', UserPath) then
    begin
      if Pos(ExpandConstant('{app}'), UserPath) = 0 then
        RegWriteExpandStringValue(HKCU, 'Environment', 'Path', UserPath + ';' + ExpandConstant('{app}'));
    end
    else
      RegWriteExpandStringValue(HKCU, 'Environment', 'Path', ExpandConstant('{app}'));
  end;
end;
