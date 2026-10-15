<#
.SYNOPSIS
  Install, upgrade, verify and uninstall a built EXE in an isolated synthetic test folder.
  Refuses to replace an existing installation. Never reads real WordCraft credentials.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)] [string] $Installer,
    [string] $TestDirectory = [IO.Path]::GetTempPath(),
    [switch] $AllUsers
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Installer = (Resolve-Path -LiteralPath $Installer).Path
$RegistryRoot = if ($AllUsers) { 'HKLM:' } else { 'HKCU:' }
$UninstallSubkey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{FE5755E2-9030-4A51-B3BA-68AC943313A8}_is1'
$UninstallKey = "$RegistryRoot\$UninstallSubkey"
foreach ($scope in @('HKCU:', 'HKLM:')) {
    if (Test-Path -LiteralPath "$scope\$UninstallSubkey") { throw 'The fork is already installed. Run this check in a clean Windows test account/machine.' }
}
if ($AllUsers) {
    $principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw '-AllUsers requires an elevated PowerShell in a clean test machine.' }
}
$CapabilitiesKey = "$RegistryRoot\Software\WordCraftChatGPT\Capabilities"
$ProgidKey = "$RegistryRoot\Software\Classes\WordCraft.ChatGPT.Document"
if ((Test-Path -LiteralPath $CapabilitiesKey) -or (Test-Path -LiteralPath $ProgidKey)) { throw 'Existing fork association keys found; refusing to modify them.' }
# Observe Windows' existing choice without changing it or exposing it in test output.
$choiceKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.docx\UserChoice'
$choiceBefore = if (Test-Path -LiteralPath $choiceKey) { (Get-Item -LiteralPath $choiceKey).GetValue('ProgId') } else { $null }
$extensionKey = "$RegistryRoot\Software\Classes\.docx"
$defaultBefore = if (Test-Path -LiteralPath $extensionKey) { (Get-Item -LiteralPath $extensionKey).GetValue('') } else { $null }
if (-not (Test-Path -LiteralPath "$Installer.payload.json")) { throw 'Installer payload inventory is missing' }
$inventory = Get-Content -LiteralPath "$Installer.payload.json" -Raw | ConvertFrom-Json
$checksum = ((Get-Content -LiteralPath "$Installer.sha256" -Raw) -split '\s+')[0]
if ((Get-FileHash -LiteralPath $Installer -Algorithm SHA256).Hash -ne $checksum) { throw 'Installer checksum mismatch' }
$TestRoot = Join-Path ([IO.Path]::GetFullPath($TestDirectory)) "wordcraft-installer-$([Guid]::NewGuid().ToString('N'))"
$InstallDir = Join-Path $TestRoot 'Other drive style path\WordCraft ChatGPT'
$ProfileDir = Join-Path $TestRoot 'synthetic-profile'
New-Item -ItemType Directory -Force -Path "$ProfileDir\WordCraft\chatgpt-direct", $InstallDir | Out-Null
$sentinel = Join-Path $ProfileDir 'WordCraft\chatgpt-direct\synthetic-profile.txt'
[IO.File]::WriteAllText($sentinel, 'Synthetic test data. Not an authentication file.')
$oldAppData = $env:APPDATA
$oldLocalAppData = $env:LOCALAPPDATA
$oldProfileDir = $env:WORDCRAFT_PROFILE_DIR
$env:APPDATA = $ProfileDir
$env:LOCALAPPDATA = $ProfileDir
$env:WORDCRAFT_PROFILE_DIR = Join-Path $ProfileDir 'WordCraft'
$installed = $false
function Run-Setup([string] $Suffix, [string] $Destination = $InstallDir, [switch] $ExpectRefusal) {
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOICONS', '/SP-', "/DIR=`"$Destination`"", '/TYPE=full', '/TASKS=fileassoc', "/LOG=`"$TestRoot\setup-$Suffix.log`"")
    if (-not $AllUsers) { $arguments += '/CURRENTUSER' }
    $process = Start-Process -FilePath $Installer -ArgumentList $arguments -PassThru -Wait
    if ($ExpectRefusal) {
        if ($process.ExitCode -eq 0) { throw 'Setup overwrote an unowned WordCraft directory' }
    } elseif ($process.ExitCode -ne 0) { throw "Setup $Suffix failed: $($process.ExitCode)" }
}
try {
    Run-Setup 'install'
    $installed = $true
    if (-not (Test-Path -LiteralPath "$InstallDir\unins000.exe")) { throw 'Uninstaller is missing' }
    foreach ($item in $inventory) {
        $relative = $item.path
        if ($relative -match '^licenses/') { $destination = Join-Path $InstallDir $relative }
        elseif ($relative -in @('README.md', 'chatgpt.md', 'windows-installer.md')) { $destination = Join-Path "$InstallDir\docs" $relative }
        elseif ($relative -in @('wordcraft.exe', 'wordcraft-cli.exe', 'wordcraft-chatgpt-login.exe')) { $destination = Join-Path $InstallDir $relative }
        else { continue } # Setup-only license/introduction/artwork are not installed files.
        if ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -ne $item.sha256) { throw "Installed file differs: $relative" }
    }
    $payload = @(Get-ChildItem -LiteralPath $InstallDir -Recurse -File)
    if ($payload | Where-Object { $_.Name -match '\.(json|env|pfx|key)$' }) { throw 'Unexpected private/configuration file in installed payload' }
    foreach ($tool in @(@('wordcraft-cli.exe', '--version'), @('wordcraft-chatgpt-login.exe', '--help'), @('wordcraft.exe', '--help'))) {
        & (Join-Path $InstallDir $tool[0]) $tool[1]
        if ($LASTEXITCODE -ne 0) { throw "Startup failed: $($tool[0])" }
    }
    $registered = Get-Item -LiteralPath "$RegistryRoot\Software\RegisteredApplications"
    if ($registered.GetValue('WordCraft with ChatGPT') -ne 'Software\WordCraftChatGPT\Capabilities') { throw 'Default Apps registration missing' }
    if ((Get-Item -LiteralPath "$CapabilitiesKey\FileAssociations").GetValue('.docx') -ne 'WordCraft.ChatGPT.Document') { throw 'DOCX capability missing' }
    $command = (Get-Item -LiteralPath "$ProgidKey\shell\open\command").GetValue('')
    if ($command -ne "`"$InstallDir\wordcraft.exe`" `"%1`"") { throw 'DOCX open command is incorrect' }
    if ((Get-Item -LiteralPath "$RegistryRoot\Software\Classes\.docx\OpenWithProgids").GetValue('WordCraft.ChatGPT.Document', $null) -ne '') { throw 'DOCX Open with entry missing' }
    $unowned = Join-Path $TestRoot 'Other WordCraft installation'
    New-Item -ItemType Directory -Force -Path $unowned | Out-Null
    [IO.File]::WriteAllText("$unowned\wordcraft.exe", 'Synthetic upstream executable sentinel')
    Run-Setup 'unowned-directory' $unowned -ExpectRefusal
    if ([IO.File]::ReadAllText("$unowned\wordcraft.exe") -ne 'Synthetic upstream executable sentinel') { throw 'Another installation was modified' }
    $document = Join-Path $InstallDir 'synthetic-user-document.txt'
    [IO.File]::WriteAllText($document, 'Synthetic document must survive upgrade and uninstall.')
    Run-Setup 'upgrade'
    if (-not (Test-Path -LiteralPath $sentinel) -or -not (Test-Path -LiteralPath $document)) { throw 'Upgrade removed user data' }
    $process = Start-Process -FilePath "$InstallDir\unins000.exe" -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=`"$TestRoot\uninstall.log`"") -PassThru -Wait
    if ($process.ExitCode -ne 0) { throw "Uninstall failed: $($process.ExitCode)" }
    $installed = $false
    if ((Test-Path -LiteralPath "$InstallDir\wordcraft.exe") -or (Test-Path -LiteralPath $UninstallKey)) { throw 'Uninstall left application files or registration' }
    if (-not (Test-Path -LiteralPath $sentinel) -or -not (Test-Path -LiteralPath $document)) { throw 'Uninstall removed user data' }
    if ((Test-Path -LiteralPath $CapabilitiesKey) -or (Test-Path -LiteralPath $ProgidKey)) { throw 'Uninstall left fork capabilities or ProgID' }
    if ((Get-Item -LiteralPath "$RegistryRoot\Software\Classes\.docx\OpenWithProgids").GetValue('WordCraft.ChatGPT.Document', $null) -ne $null) { throw 'Uninstall left DOCX association' }
    $choiceAfter = if (Test-Path -LiteralPath $choiceKey) { (Get-Item -LiteralPath $choiceKey).GetValue('ProgId') } else { $null }
    $defaultAfter = if (Test-Path -LiteralPath $extensionKey) { (Get-Item -LiteralPath $extensionKey).GetValue('') } else { $null }
    if ($choiceBefore -ne $choiceAfter -or $defaultBefore -ne $defaultAfter) { throw 'Setup changed an existing DOCX default' }
    Write-Host 'PASS: custom path, payload hashes, startup, default-app registration, no DOCX default takeover, foreign-install guard, reinstall and uninstall; profile/document preserved.'
} finally {
    # Do not remove the test directory if an installed package still references it.
    # A failure keeps logs for diagnosis; never touch another installation or real profile.
    $env:APPDATA = $oldAppData
    $env:LOCALAPPDATA = $oldLocalAppData
    $env:WORDCRAFT_PROFILE_DIR = $oldProfileDir
    if ($installed) { Write-Warning "Test installation remains at $InstallDir. Uninstall it before rerunning. Logs: $TestRoot" }
    else { Write-Host "Synthetic test files and logs: $TestRoot" }
}
