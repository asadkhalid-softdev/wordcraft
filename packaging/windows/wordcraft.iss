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
DefaultDirName={localappdata}\Programs\WordCraft ChatGPT
DefaultGroupName=WordCraft (ChatGPT fork)
AllowNoIcons=yes
DisableWelcomePage=no
DisableDirPage=no
DisableProgramGroupPage=no
DisableReadyPage=no
PrivilegesRequired=lowest
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
Name: "fileassoc"; Description: "Add WordCraft to &Open with for documents (keeps your current defaults)"; GroupDescription: "File integration:"; Flags: unchecked

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
Root: HKCU; Subkey: "Software\Classes\WordCraft.ChatGPT.Document"; ValueType: string; ValueData: "WordCraft document"; Tasks: fileassoc; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\WordCraft.ChatGPT.Document\DefaultIcon"; ValueType: string; ValueData: """{app}\wordcraft.exe"",0"; Tasks: fileassoc
Root: HKCU; Subkey: "Software\Classes\WordCraft.ChatGPT.Document\shell\open\command"; ValueType: string; ValueData: """{app}\wordcraft.exe"" ""%1"""; Tasks: fileassoc
Root: HKCU; Subkey: "Software\Classes\.docx\OpenWithProgids"; ValueType: string; ValueName: "WordCraft.ChatGPT.Document"; ValueData: ""; Tasks: fileassoc; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.docm\OpenWithProgids"; ValueType: string; ValueName: "WordCraft.ChatGPT.Document"; ValueData: ""; Tasks: fileassoc; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.dotx\OpenWithProgids"; ValueType: string; ValueName: "WordCraft.ChatGPT.Document"; ValueData: ""; Tasks: fileassoc; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.dotm\OpenWithProgids"; ValueType: string; ValueName: "WordCraft.ChatGPT.Document"; ValueData: ""; Tasks: fileassoc; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.doc\OpenWithProgids"; ValueType: string; ValueName: "WordCraft.ChatGPT.Document"; ValueData: ""; Tasks: fileassoc; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.odt\OpenWithProgids"; ValueType: string; ValueName: "WordCraft.ChatGPT.Document"; ValueData: ""; Tasks: fileassoc; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.rtf\OpenWithProgids"; ValueType: string; ValueName: "WordCraft.ChatGPT.Document"; ValueData: ""; Tasks: fileassoc; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.md\OpenWithProgids"; ValueType: string; ValueName: "WordCraft.ChatGPT.Document"; ValueData: ""; Tasks: fileassoc; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.txt\OpenWithProgids"; ValueType: string; ValueName: "WordCraft.ChatGPT.Document"; ValueData: ""; Tasks: fileassoc; Flags: uninsdeletevalue

[Run]
Filename: "{app}\wordcraft.exe"; Description: "Launch WordCraft"; Flags: nowait postinstall skipifsilent runasoriginaluser

; No profile writes, credential imports, network calls, shell commands, or profile deletion.
; Uninstall removes installed files and integration only; the user's writing and login remain.
