; Per-user Windows installer: CLI and GUI launcher stay in the same directory.
#define AppName "MDE"
#define AppVersion "0.1.0"

[Setup]
AppId={{09631609-791D-552E-88AD-EFDFD1FA06A0}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=jarymor-ux
AppPublisherURL=https://github.com/jarymor-ux/mde
AppSupportURL=https://github.com/jarymor-ux/mde/issues
DefaultDirName={localappdata}\Programs\MDE
DefaultGroupName=MDE
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\mde-desktop.exe
OutputDir=..\dist
OutputBaseFilename=mde-windows-x86_64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: checkedonce

[Files]
Source: "..\target\release\mde.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\mde-desktop.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\THIRD_PARTY_NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\examples\*"; DestDir: "{app}\examples"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\MDE"; Filename: "{app}\mde-desktop.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\MDE"; Filename: "{app}\mde-desktop.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\mde-desktop.exe"; Description: "{cm:LaunchProgram,MDE}"; Flags: nowait postinstall skipifsilent
