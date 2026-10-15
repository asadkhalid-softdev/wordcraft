; Native desktop installer for the ChatGPT fork. Built by package-exe.ps1.
; Keep this AppId distinct from upstream's MSI UpgradeCode.
#ifndef AppVersion
  #error AppVersion must be supplied by package-exe.ps1
#endif

[Setup]
AppId={{FE5755E2-9030-4A51-B3BA-68AC943313A8}
AppName=WordCraft with ChatGPT
AppVersion={#AppVersion}
AppVerName=WordCraft {#AppVersion} with ChatGPT
AppPublisher=asadkhalid-softdev and the WordCraft contributors
AppPublisherURL=https://github.com/asadkhalid-softdev/wordcraft
AppSupportURL=https://github.com/asadkhalid-softdev/wordcraft/issues
AppUpdatesURL=https://github.com/asadkhalid-softdev/wordcraft/releases
VersionInfoVersion={#FileVersion}
VersionInfoDescription=WordCraft with ChatGPT Setup
DefaultDirName={commonpf}\WordCraft
DefaultGroupName=WordCraft (ChatGPT fork)
AllowNoIcons=yes
DisableWelcomePage=no
DisableDirPage=no
DisableProgramGroupPage=no
DisableReadyPage=no
PrivilegesRequired=admin
; Current-user override is for isolated verification or explicit custom-folder installs.
PrivilegesRequiredOverridesAllowed=commandline
ArchitecturesAllowed={#SetupArch}
ArchitecturesInstallIn64BitMode={#SetupArch}
MinVersion=10.0
WizardStyle=modern
WizardSizePercent=115
SetupIconFile={#IconPath}
WizardImageFile={#StageDir}\wizard.bmp
WizardSmallImageFile={#StageDir}\wizard-small.bmp
LicenseFile={#StageDir}\license.txt
InfoBeforeFile={#StageDir}\welcome.txt
UninstallDisplayIcon={app}\wordcraft.exe
UninstallDisplayName=WordCraft (ChatGPT fork)
OutputDir={#OutputDir}
OutputBaseFilename=wordcraft-{#AppVersion}-windows-{#BuildArch}-setup
Compression=lzma2
SolidCompression=yes
CloseApplications=yes
CloseApplicationsFilter=wordcraft.exe,wordcraft-cli.exe,wordcraft-chatgpt-login.exe
RestartApplications=no
UninstallLogMode=append
ChangesAssociations=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Types]
Name: "full"; Description: "Standard installation"
Name: "compact"; Description: "WordCraft application only"
Name: "custom"; Description: "Custom installation"; Flags: iscustom

[Components]
Name: "app"; Description: "WordCraft with built-in ChatGPT integration"; Types: full compact custom; Flags: fixed
Name: "tools"; Description: "Command-line tools and optional ChatGPT sign-in helper"; Types: full

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked
Name: "fileassoc"; Description: "Register WordCraft for &Open with and Default apps (keeps your current defaults)"; GroupDescription: "File integration:"

[Files]
; Explicit payload: never source a developer profile, directory tree or credential file.
Source: "{#StageDir}\wordcraft.exe"; DestDir: "{app}"; Components: app; Flags: ignoreversion
Source: "{#StageDir}\wordcraft-cli.exe"; DestDir: "{app}"; Components: tools; Flags: ignoreversion
Source: "{#StageDir}\wordcraft-chatgpt-login.exe"; DestDir: "{app}"; Components: tools; Flags: ignoreversion
Source: "{#StageDir}\README.md"; DestDir: "{app}\docs"; Flags: ignoreversion
Source: "{#StageDir}\chatgpt.md"; DestDir: "{app}\docs"; Flags: ignoreversion
Source: "{#StageDir}\windows-installer.md"; DestDir: "{app}\docs"; Flags: ignoreversion
Source: "{#StageDir}\licenses\*"; DestDir: "{app}\licenses"; Flags: ignoreversion

[Icons]
Name: "{group}\WordCraft (ChatGPT)"; Filename: "{app}\wordcraft.exe"; WorkingDir: "{app}"
Name: "{group}\ChatGPT setup guide"; Filename: "{app}\docs\chatgpt.md"
Name: "{group}\Uninstall WordCraft (ChatGPT fork)"; Filename: "{uninstallexe}"
Name: "{autodesktop}\WordCraft (ChatGPT)"; Filename: "{app}\wordcraft.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Registry]
; HKA resolves to HKLM for the normal all-users install; no per-user writes while elevated.
Root: HKA; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "WordCraft with ChatGPT"; ValueData: "Software\WordCraftChatGPT\Capabilities"; Tasks: fileassoc; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\WordCraftChatGPT\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "WordCraft with ChatGPT"; Tasks: fileassoc; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\WordCraftChatGPT\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "Write and edit documents with optional ChatGPT assistance."; Tasks: fileassoc
Root: HKA; Subkey: "Software\WordCraftChatGPT\Capabilities"; ValueType: string; ValueName: "ApplicationIcon"; ValueData: """{app}\wordcraft.exe"",0"; Tasks: fileassoc

Root: HKA; Subkey: "Software\Classes\WordCraft.ChatGPT.Document"; ValueType: string; ValueData: "WordCraft document"; Tasks: fileassoc; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\WordCraft.ChatGPT.Document"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "WordCraft with ChatGPT"; Tasks: fileassoc
Root: HKA; Subkey: "Software\Classes\WordCraft.ChatGPT.Document\DefaultIcon"; ValueType: string; ValueData: """{app}\wordcraft.exe"",0"; Tasks: fileassoc
Root: HKA; Subkey: "Software\Classes\WordCraft.ChatGPT.Document\shell\open\command"; ValueType: string; ValueData: """{app}\wordcraft.exe"" ""%1"""; Tasks: fileassoc
Root: HKA; Subkey: "Software\Classes\.docx\OpenWithProgids"; ValueType: string; ValueName: "WordCraft.ChatGPT.Document"; ValueData: ""; Tasks: fileassoc; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.docm\OpenWithProgids"; ValueType: string; ValueName: "WordCraft.ChatGPT.Document"; ValueData: ""; Tasks: fileassoc; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.dotx\OpenWithProgids"; ValueType: string; ValueName: "WordCraft.ChatGPT.Document"; ValueData: ""; Tasks: fileassoc; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.dotm\OpenWithProgids"; ValueType: string; ValueName: "WordCraft.ChatGPT.Document"; ValueData: ""; Tasks: fileassoc; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.doc\OpenWithProgids"; ValueType: string; ValueName: "WordCraft.ChatGPT.Document"; ValueData: ""; Tasks: fileassoc; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.odt\OpenWithProgids"; ValueType: string; ValueName: "WordCraft.ChatGPT.Document"; ValueData: ""; Tasks: fileassoc; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.rtf\OpenWithProgids"; ValueType: string; ValueName: "WordCraft.ChatGPT.Document"; ValueData: ""; Tasks: fileassoc; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.md\OpenWithProgids"; ValueType: string; ValueName: "WordCraft.ChatGPT.Document"; ValueData: ""; Tasks: fileassoc; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.txt\OpenWithProgids"; ValueType: string; ValueName: "WordCraft.ChatGPT.Document"; ValueData: ""; Tasks: fileassoc; Flags: uninsdeletevalue

Root: HKA; Subkey: "Software\WordCraftChatGPT\Capabilities\FileAssociations"; ValueType: string; ValueName: ".docx"; ValueData: "WordCraft.ChatGPT.Document"; Tasks: fileassoc
Root: HKA; Subkey: "Software\WordCraftChatGPT\Capabilities\FileAssociations"; ValueType: string; ValueName: ".docm"; ValueData: "WordCraft.ChatGPT.Document"; Tasks: fileassoc
Root: HKA; Subkey: "Software\WordCraftChatGPT\Capabilities\FileAssociations"; ValueType: string; ValueName: ".dotx"; ValueData: "WordCraft.ChatGPT.Document"; Tasks: fileassoc
Root: HKA; Subkey: "Software\WordCraftChatGPT\Capabilities\FileAssociations"; ValueType: string; ValueName: ".dotm"; ValueData: "WordCraft.ChatGPT.Document"; Tasks: fileassoc
Root: HKA; Subkey: "Software\WordCraftChatGPT\Capabilities\FileAssociations"; ValueType: string; ValueName: ".doc"; ValueData: "WordCraft.ChatGPT.Document"; Tasks: fileassoc
Root: HKA; Subkey: "Software\WordCraftChatGPT\Capabilities\FileAssociations"; ValueType: string; ValueName: ".odt"; ValueData: "WordCraft.ChatGPT.Document"; Tasks: fileassoc
Root: HKA; Subkey: "Software\WordCraftChatGPT\Capabilities\FileAssociations"; ValueType: string; ValueName: ".rtf"; ValueData: "WordCraft.ChatGPT.Document"; Tasks: fileassoc
Root: HKA; Subkey: "Software\WordCraftChatGPT\Capabilities\FileAssociations"; ValueType: string; ValueName: ".md"; ValueData: "WordCraft.ChatGPT.Document"; Tasks: fileassoc
Root: HKA; Subkey: "Software\WordCraftChatGPT\Capabilities\FileAssociations"; ValueType: string; ValueName: ".txt"; ValueData: "WordCraft.ChatGPT.Document"; Tasks: fileassoc

[Run]
Filename: "{app}\wordcraft.exe"; Description: "Launch WordCraft"; Flags: nowait postinstall skipifsilent runasoriginaluser

Filename: "{code:DefaultAppsUri}"; Description: "Choose default apps for documents"; Tasks: fileassoc; Flags: shellexec postinstall skipifsilent unchecked runasoriginaluser

; No profile writes, credential imports or profile deletion.
; Uninstall removes installed files and integration only; the user's writing and login remain.

[Code]
function DefaultAppsUri(Param: String): String;
begin
  if IsAdminInstallMode then
    Result := 'ms-settings:defaultapps?registeredAppMachine=WordCraft%20with%20ChatGPT'
  else
    Result := 'ms-settings:defaultapps?registeredAppUser=WordCraft%20with%20ChatGPT';
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  RootKey: Integer;
  PreviousDir: String;
begin
  Result := '';
  if not FileExists(ExpandConstant('{app}\wordcraft.exe')) then
    Exit;
  if IsAdminInstallMode then RootKey := HKLM else RootKey := HKCU;
  if RegQueryStringValue(RootKey,
      'Software\Microsoft\Windows\CurrentVersion\Uninstall\{FE5755E2-9030-4A51-B3BA-68AC943313A8}_is1',
      'InstallLocation', PreviousDir) then
    if (CompareText(AddBackslash(PreviousDir), AddBackslash(ExpandConstant('{app}'))) = 0) and
        FileExists(ExpandConstant('{app}\unins000.exe')) then
      Exit;
  Result := 'This folder already contains another WordCraft installation. Uninstall that edition first, or choose a different folder. Your documents and ChatGPT profile are stored separately.';
end;
