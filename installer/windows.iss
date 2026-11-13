; Lodestone Windows installer (Inno Setup 6).
; Per-user install: no admin rights needed. Installs to
; %LOCALAPPDATA%\Programs\Lodestone, adds a Start Menu entry (Windows Search)
; and an uninstaller in Settings > Apps. User data is never touched.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6F3B2C1E-7A4D-4E8B-9C2F-1D5A7B3E9F40}
AppName=Lodestone
AppVersion={#AppVersion}
AppVerName=Lodestone {#AppVersion}
AppPublisher=p1e2r
AppPublisherURL=https://github.com/p1e2r/lodestone-launcher
DefaultDirName={userpf}\Lodestone
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=Lodestone-windows-x64-setup
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\Lodestone.exe
UninstallDisplayName=Lodestone
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "..\dist\Lodestone-windows-x64.exe"; DestDir: "{app}"; DestName: "Lodestone.exe"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\Lodestone"; Filename: "{app}\Lodestone.exe"
Name: "{userdesktop}\Lodestone"; Filename: "{app}\Lodestone.exe"; Tasks: desktopicon

[Registry]
; Remove the entry written when Lodestone moved itself here (avoids a duplicate in Apps).
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lodestone"; Flags: deletekey

[Run]
Filename: "{app}\Lodestone.exe"; Description: "Launch Lodestone"; Flags: nowait postinstall skipifsilent
