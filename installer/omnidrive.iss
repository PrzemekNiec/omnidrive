#ifndef AppVersion
  #define AppVersion "0.3.31"
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
#define TrayExeName "omnidrive-tray.exe"
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
Source: "{#PayloadDir}\{#TrayExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\omnidrive.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\omnidrive_shell_ext.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\static\*"; DestDir: "{app}\static"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#PayloadDir}\icons\*"; DestDir: "{app}\icons"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#SourcePath}\{#AutostartLauncherName}"; DestDir: "{app}"; Flags: ignoreversion

[Registry]
Root: HKCU; Subkey: "{#RunKeyPath}"; ValueType: string; ValueName: "{#RunValueName}"; ValueData: """{sys}\wscript.exe"" //B ""{app}\{#AutostartLauncherName}"""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\CLSID\{{8D437341-B89B-4D14-9983-5A50529A88B4}"; ValueType: string; ValueName: ""; ValueData: "OmniDrive"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\CLSID\{{8D437341-B89B-4D14-9983-5A50529A88B4}\InprocServer32"; ValueType: string; ValueName: ""; ValueData: "{app}\omnidrive_shell_ext.dll"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\CLSID\{{8D437341-B89B-4D14-9983-5A50529A88B4}\InprocServer32"; ValueType: string; ValueName: "ThreadingModel"; ValueData: "Apartment"
Root: HKCU; Subkey: "Software\Classes\*\shellex\ContextMenuHandlers\OmniDrive"; ValueType: string; ValueName: ""; ValueData: "{{8D437341-B89B-4D14-9983-5A50529A88B4}"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Directory\shellex\ContextMenuHandlers\OmniDrive"; ValueType: string; ValueName: ""; ValueData: "{{8D437341-B89B-4D14-9983-5A50529A88B4}"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\*\shell\OmniDrive"; ValueType: none; Flags: deletekey
Root: HKCU; Subkey: "Software\Classes\Directory\shell\OmniDrive"; ValueType: none; Flags: deletekey

[Icons]
Name: "{group}\OmniDrive Daemon"; Filename: "{app}\{#AppExeName}"
Name: "{group}\OmniDrive CLI"; Filename: "{app}\{#CliExeName}"

[Run]
Filename: "{sys}\wscript.exe"; Parameters: "//B ""{app}\{#AutostartLauncherName}"""; Description: "Start OmniDrive after installation"; Flags: nowait postinstall skipifsilent

[InstallDelete]
; Clean stale files from previous versions if needed

[UninstallRun]
Filename: "taskkill"; Parameters: "/F /IM {#TrayExeName}"; Flags: runhidden waituntilterminated; RunOnceId: "KillTray"
Filename: "taskkill"; Parameters: "/F /IM {#AppExeName}"; Flags: runhidden waituntilterminated; RunOnceId: "KillDaemon"

[UninstallDelete]
Type: files; Name: "{app}\omnidrive_shell_ext.dll*.old"

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

procedure KillRunningProcesses();
var
  ResultCode: Integer;
begin
  Exec('taskkill', '/F /IM {#TrayExeName}', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Exec('taskkill', '/F /IM {#AppExeName}', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
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

  { Kill running processes so the installer can overwrite binaries }
  KillRunningProcesses();

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

procedure RenameLockedShellExtDll();
var
  DllPath, BackupPath: string;
begin
  { Explorer keeps the shell extension DLL loaded, so overwriting it in place would fail;
    Windows allows renaming a loaded DLL, so move it aside and let [Files] write a fresh one. }
  DllPath := ExpandConstant('{app}\omnidrive_shell_ext.dll');
  if FileExists(DllPath) then
  begin
    BackupPath := DllPath + '.' + GetDateTimeString('yyyymmddhhnnsszzz', '', '') + '.old';
    RenameFile(DllPath, BackupPath);
  end;
end;

procedure CleanupOldShellExtDlls();
var
  FindRec: TFindRec;
  Dir: string;
begin
  Dir := ExpandConstant('{app}');
  if FindFirst(Dir + '\omnidrive_shell_ext.dll.*.old', FindRec) then
  begin
    try
      repeat
        DeleteFile(Dir + '\' + FindRec.Name);
      until not FindNext(FindRec);
    finally
      FindClose(FindRec);
    end;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssInstall then
    RenameLockedShellExtDll();
  if CurStep = ssPostInstall then
  begin
    AddInstallDirToUserPath();
    CleanupOldShellExtDlls();
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    RemoveInstallDirFromUserPath();
end;
