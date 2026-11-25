; The PlayLibre installer for Windows (Inno Setup 6). The release workflow
; builds it with:
;
;   iscc /DVersion=<version> /DNumericVersion=<a.b.c.d> /DSource=<folder with playlibre.exe> /DOutput=<file name without .exe> packaging\windows\playlibre.iss
;
; It's the usual setup wizard, with the PlayLibre icon on it and on
; everything it makes.
; It installs for the current user only, so it needs no administrator:
; the app goes in %LOCALAPPDATA%\Programs\PlayLibre, with a Start menu entry
; (and a desktop one if you tick it). It adds that folder to your PATH, so
; `playlibre` in a new terminal opens the app, and registers playlibre://
; links, so Play on the website opens it. Uninstalling takes all of that
; out again and keeps your games and settings.

#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef Arch
  ; x64compatible for the x86-64 build, arm64 for the ARM one.
  #define Arch "x64compatible"
#endif
#ifndef Source
  #define Source "..\..\target\x86_64-pc-windows-msvc\dist"
#endif
; Windows wants a plain a.b.c.d version in the installer's file details.
#ifndef NumericVersion
  #define NumericVersion "0.0.0.0"
#endif
#ifndef Output
  #define Output "playlibre-setup"
#endif

[Setup]
AppId={{6B0C2B9E-3F4A-4E7B-9C1D-7A2F5E8D4C31}
AppName=PlayLibre
AppVersion={#Version}
AppPublisher=PlayLibre
AppPublisherURL=https://playlibre.net
DefaultDirName={localappdata}\Programs\PlayLibre
DefaultGroupName=PlayLibre
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ChangesEnvironment=yes
ChangesAssociations=yes
OutputDir=.
OutputBaseFilename={#Output}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupIconFile=..\icons\playlibre.ico
WizardImageFile=wizard-large.bmp
WizardSmallImageFile=wizard-small.bmp
AppSupportURL=https://playlibre.net
VersionInfoVersion={#NumericVersion}
VersionInfoProductName=PlayLibre
CloseApplications=yes
ArchitecturesAllowed={#Arch}
ArchitecturesInstallIn64BitMode={#Arch}
UninstallDisplayIcon={app}\playlibre.ico
UninstallDisplayName=PlayLibre

[Tasks]
Name: "desktopicon"; Description: "Put PlayLibre on the desktop"; GroupDescription: "Shortcuts:"; Flags: checkedonce

[InstallDelete]
Type: files; Name: "{app}\libroxel.exe"
Type: files; Name: "{app}\libroxel.ico"
Type: files; Name: "{autoprograms}\Libroxel.lnk"
Type: files; Name: "{autodesktop}\Libroxel.lnk"

[Files]
Source: "{#Source}\playlibre.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\icons\playlibre.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\README.md"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#Source}\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#Source}\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist

[Icons]
Name: "{autoprograms}\PlayLibre"; Filename: "{app}\playlibre.exe"; IconFilename: "{app}\playlibre.ico"; Comment: "Make and play 3D games with friends"
Name: "{autodesktop}\PlayLibre"; Filename: "{app}\playlibre.exe"; IconFilename: "{app}\playlibre.ico"; Tasks: desktopicon

[Registry]
; playlibre:// links open the app.
Root: HKCU; Subkey: "Software\Classes\playlibre"; ValueType: string; ValueName: ""; ValueData: "URL:PlayLibre"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\playlibre"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\playlibre\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\playlibre.ico"
Root: HKCU; Subkey: "Software\Classes\playlibre\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\playlibre.exe"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\libroxel"; ValueType: string; ValueName: ""; ValueData: "URL:PlayLibre"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\libroxel"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\libroxel\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\playlibre.ico"
Root: HKCU; Subkey: "Software\Classes\libroxel\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\playlibre.exe"" ""%1"""

[Run]
Filename: "{app}\playlibre.exe"; Description: "Open PlayLibre now"; Flags: nowait postinstall skipifsilent
Filename: "{app}\playlibre.exe"; Flags: nowait; Check: WizardSilent

[Code]
const
  EnvKey = 'Environment';

{ Whether Dir is one of the folders in the user's PATH. }
function OnPath(Paths, Dir: string): Boolean;
begin
  Result := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Paths) + ';') > 0;
end;

procedure AddToPath(Dir: string);
var
  Paths: string;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Paths) then
    Paths := '';
  if OnPath(Paths, Dir) then
    exit;
  if (Paths <> '') and (Copy(Paths, Length(Paths), 1) <> ';') then
    Paths := Paths + ';';
  RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Paths + Dir);
end;

procedure RemoveFromPath(Dir: string);
var
  Paths: string;
  P: Integer;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Paths) then
    exit;
  Paths := ';' + Paths + ';';
  P := Pos(';' + Uppercase(Dir) + ';', Uppercase(Paths));
  if P = 0 then
    exit;
  Delete(Paths, P, Length(Dir) + 1);
  { Trim the separators added above. }
  if Copy(Paths, 1, 1) = ';' then
    Delete(Paths, 1, 1);
  if Copy(Paths, Length(Paths), 1) = ';' then
    Delete(Paths, Length(Paths), 1);
  RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Paths);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    AddToPath(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemoveFromPath(ExpandConstant('{app}'));
end;
