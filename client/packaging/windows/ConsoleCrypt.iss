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
; Vaults are outside {app}; uninstall never deletes the user's data.
