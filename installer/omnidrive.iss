#ifndef AppVersion
  #define AppVersion "0.1.14"
#endif

#ifndef PayloadDir
  #define PayloadDir "..\dist\installer\payload"
#endif

#ifndef OutputDir
  #define OutputDir "..\dist\installer\output"
#endif

#define AppName "OmniDrive"
#define AppPublisher "OmniDrive"
#define AppExeName "angeld.exe"
#define CliExeName "omnidrive.exe"
#define AutostartLauncherName "angeld-autostart.vbs"
#define AppAssocName "OmniDrive"
#define AppId "{{B5F0E7D0-5B7C-4A4A-9F93-1C0C6C0B5A27}"
#define RunKeyPath "Software\Microsoft\Windows\CurrentVersion\Run"
#define RunValueName "OmniDriveAngeld"

[Setup]
AppId={#AppId}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={localappdata}\Programs\OmniDrive
DefaultGroupName=OmniDrive
DisableProgramGroupPage=no
UninstallDisplayIcon={app}\icons\omnidrive.ico
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=OmniDrive-Setup-{#AppVersion}
SetupIconFile={#PayloadDir}\icons\omnidrive.ico
ChangesEnvironment=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "addtopath"; Description: "Add OmniDrive installation directory to the system PATH"; Flags: unchecked

[Dirs]
Name: "{localappdata}\OmniDrive"; Flags: uninsneveruninstall

[Files]
Source: "{#PayloadDir}\angeld.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\omnidrive.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\static\*"; DestDir: "{app}\static"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#PayloadDir}\icons\*"; DestDir: "{app}\icons"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#SourcePath}\{#AutostartLauncherName}"; DestDir: "{app}"; Flags: ignoreversion

[Registry]
Root: HKCU; Subkey: "{#RunKeyPath}"; ValueType: string; ValueName: "{#RunValueName}"; ValueData: """{sys}\wscript.exe"" //B ""{app}\{#AutostartLauncherName}"""; Flags: uninsdeletevalue

[Icons]
Name: "{group}\OmniDrive Daemon"; Filename: "{app}\{#AppExeName}"
Name: "{group}\OmniDrive CLI"; Filename: "{app}\{#CliExeName}"

[Run]
Filename: "{sys}\wscript.exe"; Parameters: "//B ""{app}\{#AutostartLauncherName}"""; Description: "Start OmniDrive after installation"; Flags: nowait postinstall skipifsilent unchecked

[Code]
const
  CFAPI_MIN_BUILD = 16299;
  UserEnvironmentKey = 'Environment';

function IsCloudFilesSupported(): Boolean;
var
  Version: TWindowsVersion;
begin
  GetWindowsVersionEx(Version);
  Result :=
    Version.NTPlatform and
    (
      (Version.Major > 10) or
      ((Version.Major = 10) and (Version.Build >= CFAPI_MIN_BUILD))
    );
end;

function InitializeSetup(): Boolean;
begin
  if not IsCloudFilesSupported() then
  begin
    MsgBox(
      'OmniDrive requires Windows 10 Fall Creators Update (build 16299) or newer because Smart Sync depends on the Cloud Files API.',
      mbCriticalError,
      MB_OK
    );
    Result := False;
    exit;
  end;

  Result := True;
end;

function PathContainsEntry(const ExistingPath, Entry: string): Boolean;
var
  SearchPath: string;
begin
  SearchPath := ';' + Lowercase(ExistingPath) + ';';
  Result := Pos(';' + Lowercase(Entry) + ';', SearchPath) > 0;
end;

procedure AddInstallDirToUserPath();
var
  ExistingPath: string;
  ExpandedPath: string;
begin
  if not WizardIsTaskSelected('addtopath') then
    exit;

  if not RegQueryStringValue(HKCU, UserEnvironmentKey, 'Path', ExistingPath) then
    ExistingPath := '';

  ExpandedPath := ExpandConstant('{app}');
  if PathContainsEntry(ExistingPath, ExpandedPath) then
    exit;

  if (ExistingPath <> '') and (ExistingPath[Length(ExistingPath)] <> ';') then
    ExistingPath := ExistingPath + ';';
  ExistingPath := ExistingPath + ExpandedPath;

  if not RegWriteExpandStringValue(HKCU, UserEnvironmentKey, 'Path', ExistingPath) then
    MsgBox('Failed to update the user PATH for OmniDrive.', mbError, MB_OK);
end;

function RemovePathEntry(const ExistingPath, Entry: string): string;
var
  SearchPath: string;
begin
  SearchPath := ';' + ExistingPath + ';';
  StringChangeEx(SearchPath, ';' + Entry + ';', ';', True);

  while Pos(';;', SearchPath) > 0 do
    StringChangeEx(SearchPath, ';;', ';', True);

  if (Length(SearchPath) > 0) and (SearchPath[1] = ';') then
    Delete(SearchPath, 1, 1);
  if (Length(SearchPath) > 0) and (SearchPath[Length(SearchPath)] = ';') then
    Delete(SearchPath, Length(SearchPath), 1);

  Result := SearchPath;
end;

procedure RemoveInstallDirFromUserPath();
var
  ExistingPath: string;
  UpdatedPath: string;
begin
  if not RegQueryStringValue(HKCU, UserEnvironmentKey, 'Path', ExistingPath) then
    exit;

  UpdatedPath := RemovePathEntry(ExistingPath, ExpandConstant('{app}'));
  if UpdatedPath = ExistingPath then
    exit;

  RegWriteExpandStringValue(HKCU, UserEnvironmentKey, 'Path', UpdatedPath);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    AddInstallDirToUserPath();
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    RemoveInstallDirFromUserPath();
end;
