; The Windows installer, built with Inno Setup 6.3 or later from a release
; binary (the release workflow does this on every tag):
;
;   iscc /DVersion=0.9.1 /DArch=x86_64 /DBinary=...\tidalfast.exe ^
;        /DOutputDir=dist packaging\windows\tidalfast.iss
;
; Arch is x86_64 or aarch64, as in the Rust target triple, so the installer
; is named like the zip next to it. It needs no administrator rights: the
; program goes to the user's own Programs folder with a Start menu entry,
; and a running copy is closed before an update replaces it.

#ifndef Version
  #error Version must be defined on the ISCC command line
#endif
#ifndef Arch
  #error Arch must be defined on the ISCC command line (x86_64 or aarch64)
#endif
#ifndef Binary
  #error Binary must be defined on the ISCC command line
#endif
#ifndef OutputDir
  #error OutputDir must be defined on the ISCC command line
#endif
#if Arch == "aarch64"
  #define InnoArch "arm64"
#else
  #define InnoArch "x64compatible"
#endif

#define AppName "Tidalfast"
#define AppExeName "tidalfast.exe"
#define AppIdentity "Tidalfast"

[Setup]
; Never change: this is how Windows tells an update from a new program.
AppId={{2BC3C20F-7CC4-4BD0-A51C-0A5415B0189B}
AppName={#AppName}
AppVersion={#Version}
AppVerName={#AppName} {#Version}
AppPublisher=Tidalfast contributors
AppCopyright=© 2026 Tidalfast contributors
AppPublisherURL=https://cioudguy.github.io/tidalfast
AppSupportURL=https://github.com/CIoudGuy/tidalfast/issues
AppUpdatesURL=https://cioudguy.github.io/tidalfast/download/
DefaultDirName={localappdata}\Programs\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed={#InnoArch}
ArchitecturesInstallIn64BitMode={#InnoArch}
MinVersion=10.0
LicenseFile=..\..\LICENSE
OutputDir={#OutputDir}
OutputBaseFilename=tidalfast-v{#Version}-{#Arch}-pc-windows-msvc-setup
SetupIconFile=tidalfast.ico
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
UninstallDisplayIcon={app}\{#AppExeName}
; The file version has to be numbers: a release candidate's -rc1 comes off.
#define Dash Pos("-", Version)
#if Dash > 0
  #define NumericVersion Copy(Version, 1, Dash - 1)
#else
  #define NumericVersion Version
#endif
VersionInfoVersion={#NumericVersion}.0

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "{#Binary}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "tidalfast-installer.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Registry]
; TIDAL links (tidal:track:…) open in Tidalfast. Registered for this
; user only, like the program itself. The official client registers the same
; scheme when it is installed; whichever was set up last has the links, and
; Settings > Apps > Default apps can hand them to the other, where Tidalfast
; is listed through the capabilities below. The tidaluna: scheme (TidaLuna's
; own, which plugins wait on for a sign-in redirect) goes
; to the same program the same way.
Root: HKCU; Subkey: "Software\Classes\tidal"; ValueType: string; ValueName: ""; ValueData: "URL:TIDAL link"
Root: HKCU; Subkey: "Software\Classes\tidal"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\tidal\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExeName}"",0"
Root: HKCU; Subkey: "Software\Classes\tidal\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExeName}"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\tidaluna"; ValueType: string; ValueName: ""; ValueData: "URL:TidaLuna link"
Root: HKCU; Subkey: "Software\Classes\tidaluna"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\tidaluna\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExeName}"",0"
Root: HKCU; Subkey: "Software\Classes\tidaluna\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExeName}"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\Tidalfast.tidal"; ValueType: string; ValueName: ""; ValueData: "URL:TIDAL link"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Tidalfast.tidal"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Tidalfast.tidal\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExeName}"",0"
Root: HKCU; Subkey: "Software\Classes\Tidalfast.tidal\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExeName}"" ""%1"""
Root: HKCU; Subkey: "Software\{#AppIdentity}\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "{#AppName}"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\{#AppIdentity}\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "A native TIDAL client"
Root: HKCU; Subkey: "Software\{#AppIdentity}\Capabilities\URLAssociations"; ValueType: string; ValueName: "tidal"; ValueData: "Tidalfast.tidal"
Root: HKCU; Subkey: "Software\{#AppIdentity}\Capabilities\URLAssociations"; ValueType: string; ValueName: "tidaluna"; ValueData: "Tidalfast.tidal"
Root: HKCU; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "{#AppIdentity}"; ValueData: "Software\{#AppIdentity}\Capabilities"; Flags: uninsdeletevalue

[Run]
Filename: "{app}\{#AppExeName}"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent

[Code]
#if Arch == "x86_64"
// x64compatible lets this installer run on an ARM PC under emulation, and a
// copy installed from it keeps updating to x64. The ARM installer has
// everything this one has and runs natively, so point to it. A silent
// install (an update) goes ahead without asking.
function InitializeSetup(): Boolean;
begin
  Result := True;
  if IsArm64 and not WizardSilent then
    Result := MsgBox('This PC has an ARM processor. The ARM installer ' +
      '(aarch64) runs natively, faster and on less battery.' + #13#10#13#10 +
      'Install the version for Intel and AMD processors anyway?',
      mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES;
end;
#endif

procedure CloseInstalledTidalfast;
var
  Locator, Services, Processes, Process, Path: Variant;
  Index: Integer;
  Exe: String;
begin
  Exe := ExpandConstant('{app}\{#AppExeName}');
  try
    Locator := CreateOleObject('WbemScripting.SWbemLocator');
    Services := Locator.ConnectServer('', 'root\CIMV2');
    Processes := Services.ExecQuery('SELECT * FROM Win32_Process WHERE Name = ''{#AppExeName}''');
    for Index := 0 to Processes.Count - 1 do begin
      Process := Processes.ItemIndex(Index);
      Path := Process.ExecutablePath;
      // A portable copy can have the same name. Never close it by name alone.
      if not VarIsNull(Path) then
        if CompareText(Path, Exe) = 0 then
          Log('Closing installed Tidalfast: result ' + IntToStr(Process.Terminate(0)));
    end;
  except
    Log('Could not close installed Tidalfast: ' + GetExceptionMessage);
  end;
end;

// CloseApplications covers Setup, not Uninstall. Check the installed file
// before removing anything, including the uninstaller needed to try again.
function InitializeUninstall: Boolean;
var
  Exe: String;
  Probe: TFileStream;
begin
  Result := False;
  Exe := ExpandConstant('{app}\{#AppExeName}');
  while FileExists(Exe) do begin
    try
      // Windows denies write access to a running executable. Do not write
      // anything: opening it also catches other locks on this exact copy.
      Probe := TFileStream.Create(Exe, fmOpenReadWrite or fmShareExclusive);
      Probe.Free;
      Result := True;
      Exit;
    except
      Log('Cannot open the installed executable for uninstall: ' + GetExceptionMessage);
    end;
    if UninstallSilent then
      Exit;
    case SuppressibleTaskDialogMsgBox('Tidalfast cannot be removed yet.',
      'Its program file is in use or inaccessible. Quit Tidalfast and retry, or close it here to continue.' + #13#10#13#10 +
      'Close Tidalfast ends the process immediately. Unsaved changes may be lost.',
      mbError, MB_YESNOCANCEL, ['Retry', 'Close Tidalfast'], 0, IDCANCEL) of
      IDYES: ;
      IDNO: begin
        CloseInstalledTidalfast;
        Sleep(200);
      end;
    else
      Exit;
    end;
  end;
  Result := True;
end;

// The tidal: and tidaluna: scheme keys are shared with whatever else opens
// the links, so uninstalling takes each away only while it still names this
// program.
procedure RemoveSchemeKeyIfOurs(Key: String);
var
  Command: String;
  Exe: String;
begin
  if not RegQueryStringValue(HKCU, Key + '\shell\open\command', '', Command) then
    Exit;
  Exe := Lowercase(ExpandConstant('{app}\{#AppExeName}'));
  if Pos(Exe, Lowercase(Command)) > 0 then
    RegDeleteKeyIncludingSubkeys(HKCU, Key);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep <> usUninstall then
    Exit;
  RemoveSchemeKeyIfOurs('Software\Classes\tidal');
  RemoveSchemeKeyIfOurs('Software\Classes\tidaluna');
end;
