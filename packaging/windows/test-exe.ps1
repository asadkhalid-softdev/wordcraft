<#
.SYNOPSIS
  Install, upgrade, verify and uninstall a built EXE in an isolated synthetic test folder.
  Refuses to replace an existing installation. Never reads real WordCraft credentials.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)] [string] $Installer,
    [string] $TestDirectory = [IO.Path]::GetTempPath()
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Installer = (Resolve-Path -LiteralPath $Installer).Path
$UninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{FE5755E2-9030-4A51-B3BA-68AC943313A8}_is1'
if (Test-Path -LiteralPath $UninstallKey) { throw 'The fork is already installed for this user. Run this check in a clean Windows test account.' }
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
function Run-Setup([string] $Suffix) {
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOICONS', '/SP-', "/DIR=`"$InstallDir`"", '/TYPE=full', '/TASKS=""', "/LOG=`"$TestRoot\setup-$Suffix.log`"")
    $process = Start-Process -FilePath $Installer -ArgumentList $arguments -PassThru -Wait
    if ($process.ExitCode -ne 0) { throw "Setup $Suffix failed: $($process.ExitCode)" }
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
    $document = Join-Path $InstallDir 'synthetic-user-document.txt'
    [IO.File]::WriteAllText($document, 'Synthetic document must survive upgrade and uninstall.')
    Run-Setup 'upgrade'
    if (-not (Test-Path -LiteralPath $sentinel) -or -not (Test-Path -LiteralPath $document)) { throw 'Upgrade removed user data' }
    $process = Start-Process -FilePath "$InstallDir\unins000.exe" -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=`"$TestRoot\uninstall.log`"") -PassThru -Wait
    if ($process.ExitCode -ne 0) { throw "Uninstall failed: $($process.ExitCode)" }
    $installed = $false
    if ((Test-Path -LiteralPath "$InstallDir\wordcraft.exe") -or (Test-Path -LiteralPath $UninstallKey)) { throw 'Uninstall left application files or registration' }
    if (-not (Test-Path -LiteralPath $sentinel) -or -not (Test-Path -LiteralPath $document)) { throw 'Uninstall removed user data' }
    Write-Host 'PASS: custom path, installed hashes, clean payload, startup, upgrade and uninstall; user data preserved.'
} finally {
    # Do not remove the test directory if an installed package still references it.
    # A failure keeps logs for diagnosis; never touch another installation or real profile.
    $env:APPDATA = $oldAppData
    $env:LOCALAPPDATA = $oldLocalAppData
    $env:WORDCRAFT_PROFILE_DIR = $oldProfileDir
    if ($installed) { Write-Warning "Test installation remains at $InstallDir. Uninstall it before rerunning. Logs: $TestRoot" }
    else { Write-Host "Synthetic test files and logs: $TestRoot" }
}
