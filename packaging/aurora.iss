#define AppName "Aurora Browser"
#define AppVersion "0.1.0-alpha"
#define AppPublisher "Aurora"
#define AppExeName "Aurora.exe"
#define RepoRoot AddBackslash(SourcePath) + "..\"
#define ReleaseDir AddBackslash(GetEnv("AURORA_ALPHA_PAYLOAD"))

[Setup]
AppId={{F8D3D434-26DD-4D95-89BE-FC673DFD08EE}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={localappdata}\Programs\Aurora Browser
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile={#RepoRoot}rust-browser\assets\aurora.ico
UninstallDisplayIcon={app}\{#AppExeName}
OutputDir={#RepoRoot}dist\alpha
OutputBaseFilename=Aurora-0.1.0-alpha-Windows-x64-Setup
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
Uninstallable=yes
ChangesAssociations=no
VersionInfoVersion=0.1.0.0
VersionInfoProductVersion=0.1.0.0
VersionInfoTextVersion={#AppVersion}
VersionInfoDescription=Aurora Browser installer

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[CustomMessages]
english.WebViewMissing=Microsoft Edge WebView2 Runtime is required to run Aurora Browser. Setup will now open Microsoft's official download page. Install the Evergreen Runtime, then run this installer again.
russian.WebViewMissing=Для работы Aurora Browser требуется Microsoft Edge WebView2 Runtime. Сейчас откроется официальная страница Microsoft. Установите Evergreen Runtime, затем запустите установщик Aurora ещё раз.

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "{#ReleaseDir}{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}docs\alpha\README.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Aurora Browser"; Filename: "{app}\{#AppExeName}"
Name: "{autodesktop}\Aurora Browser"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Code]
const
  WebView2ClientId = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}';

function IsPositiveRuntimeVersion(Version: String): Boolean; forward;

function HasWebView2Runtime: Boolean;
var
  Version: String;
begin
  Result := False;
  if RegQueryStringValue(HKLM32,
       'Software\Microsoft\EdgeUpdate\Clients\' + WebView2ClientId,
       'pv', Version) and IsPositiveRuntimeVersion(Version) then
    Result := True;
  if not Result and RegQueryStringValue(HKCU32,
       'Software\Microsoft\EdgeUpdate\Clients\' + WebView2ClientId,
       'pv', Version) and IsPositiveRuntimeVersion(Version) then
    Result := True;
end;

function IsPositiveRuntimeVersion(Version: String): Boolean;
var
  I, PartStart, PartCount, PartValue: Integer;
  Part: String;
  NonZero: Boolean;
begin
  Result := False;
  NonZero := False;
  PartCount := 0;
  PartStart := 1;
  for I := 1 to Length(Version) + 1 do
  begin
    if I <= Length(Version) then
    begin
      if Version[I] <> '.' then
        Continue;
      Part := Copy(Version, PartStart, I - PartStart);
    end
    else
      Part := Copy(Version, PartStart, I - PartStart);
    if Part = '' then
      Exit;
    PartValue := StrToIntDef(Part, -1);
    if PartValue < 0 then
      Exit;
    if PartValue > 0 then
      NonZero := True;
    PartCount := PartCount + 1;
    PartStart := I + 1;
  end;
  Result := (PartCount = 4) and NonZero;
end;

function InitializeSetup: Boolean;
var
  ShellResult: Integer;
begin
  Result := True;
  if not HasWebView2Runtime then
  begin
    MsgBox(ExpandConstant('{cm:WebViewMissing}'), mbInformation, MB_OK);
    ShellExec('open', 'https://developer.microsoft.com/microsoft-edge/webview2/',
      '', '', SW_SHOWNORMAL, ewNoWait, ShellResult);
    Result := False;
  end;
end;
