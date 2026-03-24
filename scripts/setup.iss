#define MyAppName "RustTube"
#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif
#define MyAppPublisher "Jonas Grimm"
#define MyAppURL "https://github.com/beqare/RustTube"
#define MySponsorURL "https://github.com/sponsors/beqare?frequency=one-time"
#define MyAppExeName "RustTube.exe"
#define MyAppDistDir "..\\dist\\RustTube"
#define MyAppIconFile "..\\assets\\icon.ico"

[Setup]
AppId={{9B1C5BB1-1A8F-4D9D-BB7D-B74A5601E9A1}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
DefaultDirName={userappdata}\JonasGrimm\RustTube
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
DisableDirPage=yes
LicenseFile=
PrivilegesRequired=lowest
OutputDir=..\dist\installer
OutputBaseFilename=RustTube-Setup
SetupIconFile={#MyAppIconFile}
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes
CloseApplicationsFilter={#MyAppExeName}
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"

[Files]
Source: "{#MyAppDistDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[UninstallDelete]
Type: filesandordirs; Name: "{app}"

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch {#MyAppName}"; Flags: nowait postinstall skipifsilent
Filename: "{#MyAppURL}"; Description: "Open GitHub page"; Flags: postinstall shellexec skipifsilent unchecked
Filename: "{#MySponsorURL}"; Description: "Open Sponsor page"; Flags: postinstall shellexec skipifsilent unchecked
