; Instalador (Widgets-Setup.exe), gerado pelo Inno Setup 6 a partir de dist\Widgets
; (que o packaginguild.ps1 cria):
;   ISCC.exe /DAppVersion=3.0.0 packaging\Widgets.iss
; Instala para o usuário atual, sem pedir administrador, na mesma pasta que o install.ps1 usa.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6F1D2B7E-4C3A-4E59-9C2D-8A7B5E1F3D20}
AppName=Widgets
AppVersion={#AppVersion}
AppVerName=Widgets {#AppVersion}
AppPublisher=lucasschn1
AppPublisherURL=https://github.com/lucasschn1/Widgets
AppSupportURL=https://github.com/lucasschn1/Widgets/issues
AppUpdatesURL=https://github.com/lucasschn1/Widgets/releases
DefaultDirName={localappdata}\Programs\Widgets
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=yes
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=Widgets-Setup
SetupIconFile=..\res\widgets.ico
UninstallDisplayIcon={app}\Widgets.exe
UninstallDisplayName=Widgets
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=force
RestartApplications=no
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "ptbr"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[Tasks]
Name: "startup"; Description: "Iniciar com o Windows"
Name: "desktopicon"; Description: "Criar um atalho na área de trabalho"; Flags: unchecked

[InstallDelete]
; numa atualização, tira os arquivos da versão anterior (a 2.x, em Python, tinha a pasta _internal)
Type: filesandordirs; Name: "{app}\_internal"

[Files]
Source: "..\dist\Widgets\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{userprograms}\Widgets"; Filename: "{app}\Widgets.exe"; Comment: "Widgets da área de trabalho"
Name: "{userdesktop}\Widgets"; Filename: "{app}\Widgets.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "DesktopWidgets"; ValueData: """{app}\Widgets.exe"""; Tasks: startup

[Run]
Filename: "{app}\Widgets.exe"; Description: "Abrir os Widgets agora"; Flags: nowait postinstall

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM Widgets.exe"; Flags: runhidden; RunOnceId: "FecharWidgets"
Filename: "{sys}\reg.exe"; Parameters: "delete HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v DesktopWidgets /f"; Flags: runhidden; RunOnceId: "TirarInicializacao"

[Code]
// fecha os Widgets abertos antes de trocar os arquivos (numa atualização)
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM Widgets.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Sleep(500);
  Result := '';
end;
