; The Libroxel installer for Windows (Inno Setup 6). The release workflow
; builds it with:
;
;   iscc /DVersion=<version> /DSource=<folder with libroxel.exe> /DOutput=<file name without .exe> packaging\windows\libroxel.iss
;
; It installs for the current user only, so it needs no administrator:
; the app goes in %LOCALAPPDATA%\Programs\Libroxel, with a Start menu entry
; (and a desktop one if you tick it). It adds that folder to your PATH, so
; `libroxel` in a new terminal opens the app, and registers libroxel://
; links, so Play on the website opens it. Uninstalling takes all of that
; out again and keeps your games and settings.

#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef Source
  #define Source "..\..\target\x86_64-pc-windows-msvc\dist"
#endif
#ifndef Output
  #define Output "libroxel-setup"
#endif

[Setup]
AppId={{6B0C2B9E-3F4A-4E7B-9C1D-7A2F5E8D4C31}
AppName=Libroxel
AppVersion={#Version}
AppPublisher=Libroxel
AppPublisherURL=https://libroxel.com
DefaultDirName={localappdata}\Programs\Libroxel
DefaultGroupName=Libroxel
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ChangesEnvironment=yes
ChangesAssociations=yes
OutputDir=.
OutputBaseFilename={#Output}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\libroxel.exe
UninstallDisplayName=Libroxel

[Tasks]
Name: "desktopicon"; Description: "Put Libroxel on the desktop"; GroupDescription: "Shortcuts:"

[Files]
Source: "{#Source}\libroxel.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\README.md"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#Source}\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#Source}\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist

[Icons]
Name: "{autoprograms}\Libroxel"; Filename: "{app}\libroxel.exe"
Name: "{autodesktop}\Libroxel"; Filename: "{app}\libroxel.exe"; Tasks: desktopicon

[Registry]
; libroxel:// links open the app.
Root: HKCU; Subkey: "Software\Classes\libroxel"; ValueType: string; ValueName: ""; ValueData: "URL:Libroxel"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\libroxel"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\libroxel\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\libroxel.exe"" ""%1"""

[Run]
Filename: "{app}\libroxel.exe"; Description: "Open Libroxel now"; Flags: nowait postinstall skipifsilent

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
