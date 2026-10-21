; Inno Setup 6; use client/scripts/build-windows.ps1 -Installer.
#ifndef AppVersion
  #error AppVersion must be passed by the build script
#endif
#ifndef AppSource
  #error AppSource must point to the complete Flutter release bundle
#endif
[Setup]
AppId={{07A40F0D-CB43-48CB-B1E1-EE0F23785475}
AppName=ConsoleCrypt
AppVersion={#AppVersion}
VersionInfoVersion={#FileVersion}
AppPublisher=Alexander Evsikov
AppPublisherURL=https://consolecrypt.dev
AppSupportURL=https://github.com/evsikovas/consolecrypt-client/issues
AppUpdatesURL=https://github.com/evsikovas/consolecrypt-client/releases
DefaultDirName={localappdata}\Programs\ConsoleCrypt
DefaultGroupName=ConsoleCrypt
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename={#OutputName}
SetupIconFile={#RepoRoot}\client\flutter\windows\runner\resources\app_icon.ico
UninstallDisplayIcon={app}\ConsoleCrypt.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
UninstallDisplayName=ConsoleCrypt

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"
[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
[Files]
Source: "{#AppSource}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
[Icons]
Name: "{autoprograms}\ConsoleCrypt"; Filename: "{app}\ConsoleCrypt.exe"
Name: "{autodesktop}\ConsoleCrypt"; Filename: "{app}\ConsoleCrypt.exe"; Tasks: desktopicon
[Run]
Filename: "{app}\ConsoleCrypt.exe"; Description: "{cm:LaunchProgram,ConsoleCrypt}"; Flags: nowait postinstall skipifsilent
; In-app update (/SILENT /RELAUNCH=1): start the updated app again.
Filename: "{app}\ConsoleCrypt.exe"; Flags: nowait; Check: RelaunchAfterUpdate
; Vaults are outside {app}; uninstall never deletes the user's data.

[Code]
{ In-app updates pass /WAITPID=<pid> /RELAUNCH=1 (lib/updates/update_service.dart).
  Setup waits for that ConsoleCrypt process to exit before files are replaced,
  so no file is in use and no Restart Manager prompt appears. }
const
  SYNCHRONIZE = $00100000;

function OpenProcess(DesiredAccess: Cardinal; InheritHandle: Cardinal; ProcessId: Cardinal): Cardinal;
  external 'OpenProcess@kernel32.dll stdcall';
function WaitForSingleObject(Target: Cardinal; Milliseconds: Cardinal): Cardinal;
  external 'WaitForSingleObject@kernel32.dll stdcall';
function CloseHandle(Target: Cardinal): Cardinal;
  external 'CloseHandle@kernel32.dll stdcall';

function InitializeSetup(): Boolean;
var
  ProcessId: Integer;
  ProcessHandle: Cardinal;
begin
  ProcessId := StrToIntDef(ExpandConstant('{param:waitpid|0}'), 0);
  if ProcessId > 0 then
  begin
    ProcessHandle := OpenProcess(SYNCHRONIZE, 0, ProcessId);
    if ProcessHandle <> 0 then
    begin
      WaitForSingleObject(ProcessHandle, 60000);
      CloseHandle(ProcessHandle);
    end;
  end;
  Result := True;
end;

function RelaunchAfterUpdate(): Boolean;
begin
  Result := WizardSilent() and (ExpandConstant('{param:relaunch|0}') = '1');
end;
