; Inno Setup script for Amalith-Setup.exe — the Windows download.
; CI step: package-windows.ps1 compiles it with ISCC after staging the two
; binaries; it isn't meant to be run by hand.
;
; Installs Amalith.exe (the app) and Amalith.com (the console front door, see
; crates/amalith-console) side by side, adds Start menu / desktop shortcuts,
; associates .amalith files, and optionally puts the install folder on PATH so
; `Amalith script foo.jsx` works from any terminal. Settings are not here:
; the app keeps them in %APPDATA%\Amalith, which survives upgrades/uninstalls.
;
; Defaults to Program Files (asks for admin once); the privileges dialog also
; offers a no-admin "just for me" install in %LOCALAPPDATA%\Programs.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#define RepoRoot AddBackslash(SourcePath) + ".."
#define StageDir RepoRoot + "\target\package\windows"

[Setup]
; Never change AppId: it's how an upgrade finds the existing install.
AppId={{27CC85E5-7B09-46AA-B2EE-99B67C07F8D8}
AppName=Amalith
AppVersion={#AppVersion}
AppVerName=Amalith {#AppVersion}
AppPublisher=Amalith
AppPublisherURL=https://www.amalith.app/
AppSupportURL=https://github.com/tonykastaneda/Amalith/issues
VersionInfoVersion={#AppVersion}
DefaultDirName={autopf}\Amalith
; Upgrades reuse the existing folder without showing the folder page, so a
; new version always lands on top of the old one instead of beside it.
DisableDirPage=auto
DisableProgramGroupPage=yes
PrivilegesRequired=admin
PrivilegesRequiredOverridesAllowed=dialog commandline
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesEnvironment=yes
ChangesAssociations=yes
SetupIconFile={#RepoRoot}\crates\amalith-shell\assets\amalith.ico
UninstallDisplayIcon={app}\Amalith.exe
UninstallDisplayName=Amalith
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
OutputDir={#RepoRoot}\target\package
; No version in the name so the website can always match "-setup.exe".
OutputBaseFilename=Amalith-Setup

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "addtopath"; Description: "Add Amalith to PATH (run ""Amalith script"" from any terminal)"; GroupDescription: "Command line:"

[Files]
Source: "{#StageDir}\Amalith.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\Amalith.com"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Amalith"; Filename: "{app}\Amalith.exe"
Name: "{autodesktop}\Amalith"; Filename: "{app}\Amalith.exe"; Tasks: desktopicon

[Registry]
Root: HKA; Subkey: "Software\Classes\.amalith"; ValueType: string; ValueName: ""; ValueData: "Amalith.Document"; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.amalith\OpenWithProgids"; ValueType: string; ValueName: "Amalith.Document"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Amalith.Document"; ValueType: string; ValueName: ""; ValueData: "Amalith Document"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Amalith.Document\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\Amalith.exe,0"
Root: HKA; Subkey: "Software\Classes\Amalith.Document\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\Amalith.exe"" ""%1"""

[Run]
Filename: "{app}\Amalith.exe"; Description: "{cm:LaunchProgram,Amalith}"; Flags: nowait postinstall skipifsilent

[Code]
// PATH lives in the machine environment for an all-users install and in the
// user's for a "just for me" one — matching where the files went.
function EnvRoot: Integer;
begin
  if IsAdminInstallMode then
    Result := HKEY_LOCAL_MACHINE
  else
    Result := HKEY_CURRENT_USER;
end;

function EnvKey: String;
begin
  if IsAdminInstallMode then
    Result := 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment'
  else
    Result := 'Environment';
end;

procedure AddToPath(const Dir: String);
var
  Paths: String;
begin
  if not RegQueryStringValue(EnvRoot, EnvKey, 'Path', Paths) then
    Paths := '';
  if Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Paths) + ';') > 0 then
    exit;
  if (Paths <> '') and (Paths[Length(Paths)] <> ';') then
    Paths := Paths + ';';
  RegWriteExpandStringValue(EnvRoot, EnvKey, 'Path', Paths + Dir);
end;

// Removes exactly our entry and leaves every other PATH entry untouched.
procedure RemoveFromPath(const Dir: String);
var
  Paths: String;
  P: Integer;
begin
  if not RegQueryStringValue(EnvRoot, EnvKey, 'Path', Paths) then
    exit;
  Paths := ';' + Paths + ';';
  P := Pos(';' + Uppercase(Dir) + ';', Uppercase(Paths));
  if P = 0 then
    exit;
  Delete(Paths, P, Length(Dir) + 1);
  Paths := Copy(Paths, 2, Length(Paths) - 2);
  RegWriteExpandStringValue(EnvRoot, EnvKey, 'Path', Paths);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    // Re-running the installer with the box unticked takes it back off.
    if WizardIsTaskSelected('addtopath') then
      AddToPath(ExpandConstant('{app}'))
    else
      RemoveFromPath(ExpandConstant('{app}'));
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemoveFromPath(ExpandConstant('{app}'));
end;
