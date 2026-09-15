#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif
#ifndef MyAppBuild
  #define MyAppBuild "0"
#endif
#ifndef FolioRoot
  #define FolioRoot "../.."
#endif

#define MyAppName "Folio"
#define MyAppExeName "Folio.exe"

[Setup]
AppId={{A3C8E91F-4B2D-4E7A-9C15-8F6D2B1A0E44}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher=xuyi
AppPublisherURL=https://github.com/08820048/folio
AppSupportURL=https://github.com/08820048/folio
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
LicenseFile={#FolioRoot}\LICENSE
OutputDir={#FolioRoot}\target\release-artifacts
OutputBaseFilename=Folio-{#MyAppVersion}-{#MyAppBuild}-windows-x64
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
UninstallDisplayIcon={app}\{#MyAppExeName}
SetupIconFile={#FolioRoot}\target\folio.ico

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "{#FolioRoot}\target\windows-stage\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#FolioRoot}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#FolioRoot}\NOTICE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch Folio"; Flags: nowait postinstall skipifsilent
