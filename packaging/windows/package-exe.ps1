<#
.SYNOPSIS
  Build the fork's native Windows setup wizard (Inno Setup 6.7 or newer).
.EXAMPLE
  pwsh -File packaging/windows/package-exe.ps1
  pwsh -File packaging/windows/package-exe.ps1 -SkipBuild -IsccPath 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe'
.NOTES
  Native MSVC Rust and Windows SDK required for building, not for installing.
  Install Inno Setup from https://jrsoftware.org/isdl.php (build-time dependency only).
  Payloads come from an explicit allowlist, never the repository or user profile as a tree.
#>
[CmdletBinding()]
param(
    [ValidateSet('x64', 'arm64')] [string] $Arch = 'x64',
    [switch] $SkipBuild,
    [string] $IsccPath
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$Target = if ($Arch -eq 'x64') { 'x86_64-pc-windows-msvc' } else { 'aarch64-pc-windows-msvc' }
$SetupArch = if ($Arch -eq 'x64') { 'x64compatible' } else { 'arm64' }

function Invoke-Native([string] $What, [scriptblock] $Block) {
    Write-Host "==> $What"
    & $Block
    if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

if (-not $IsccPath) {
    $candidate = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($candidate) { $IsccPath = $candidate.Source }
    foreach ($folder in @(${env:ProgramFiles(x86)}, $env:ProgramFiles, "$env:LOCALAPPDATA\Programs")) {
        foreach ($release in @('Inno Setup 7', 'Inno Setup 6')) {
            $path = Join-Path $folder "$release\ISCC.exe"
            if (-not $IsccPath -and (Test-Path -LiteralPath $path)) { $IsccPath = $path }
        }
    }
}
if (-not $IsccPath -or -not (Test-Path -LiteralPath $IsccPath)) {
    throw 'Inno Setup compiler not found. Install Inno Setup 6.7+ from https://jrsoftware.org/isdl.php or pass -IsccPath.'
}
$IsccPath = (Resolve-Path -LiteralPath $IsccPath).Path
$Version = $env:WORDCRAFT_VERSION
if (-not $Version) {
    $inPackage = $false
    foreach ($line in Get-Content -LiteralPath (Join-Path $Root 'Cargo.toml')) {
        if ($line -match '^\s*\[') { $inPackage = $line.Trim() -eq '[workspace.package]'; continue }
        if ($inPackage -and $line -match '^version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
    }
}
if (-not $Version -or $Version -notmatch '^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$') { throw 'Invalid package version' }
$FileVersion = (($Version -split '-')[0]) + '.0'
$TargetDir = if ($env:CARGO_TARGET_DIR) { [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR) } else { Join-Path $Root 'target' }
$Dist = if ($env:DIST) { [IO.Path]::GetFullPath($env:DIST) } else { Join-Path $Root 'dist\release' }
$Stage = Join-Path $TargetDir "windows-exe-package\$Arch"
$HostTriple = ((& rustc -vV) | Where-Object { $_ -match '^host:' }) -replace '^host:\s*', ''
if ($LASTEXITCODE -ne 0) { throw 'rustc -vV failed' }
# Host builds reuse target/release; cross-builds use the target-specific directory.
$Bin = if ($HostTriple -eq $Target) { Join-Path $TargetDir 'release' } else { Join-Path $TargetDir "$Target\release" }
$oldFlags = $env:RUSTFLAGS
$oldRequireResources = $env:WORDCRAFT_REQUIRE_WINRES
Push-Location $Root
try {
    if (-not $SkipBuild) {
        $env:RUSTFLAGS = '-C target-feature=+crt-static'
        $env:WORDCRAFT_REQUIRE_WINRES = '1'
        $buildArgs = @('build', '--release', '--locked', '-p', 'wordcraft', '-p', 'wordcraft-cli', '-p', 'wordcraft-ui-egui', '--bin', 'wordcraft', '--bin', 'wordcraft-cli', '--bin', 'wordcraft-chatgpt-login')
        if ($HostTriple -ne $Target) { $buildArgs += @('--target', $Target) }
        Invoke-Native 'Build release binaries with static CRT' { & cargo @buildArgs }
    }
    # Only clear this script's private staging directory, not the build output or profile.
    if (Test-Path -LiteralPath $Stage) { Remove-Item -LiteralPath $Stage -Recurse -Force }
    New-Item -ItemType Directory -Force -Path $Stage, $Dist, "$Stage\licenses" | Out-Null
    $Machine = if ($Arch -eq 'x64') { 0x8664 } else { 0xAA64 }
    $Binaries = @('wordcraft.exe', 'wordcraft-cli.exe', 'wordcraft-chatgpt-login.exe')
    foreach ($name in $Binaries) {
        $path = Join-Path $Bin $name
        $bytes = [IO.File]::ReadAllBytes($path)
        if ($bytes.Length -lt 256) { throw "Invalid PE: $name" }
        $pe = [BitConverter]::ToInt32($bytes, 0x3c)
        if ($pe -lt 0 -or $pe -gt $bytes.Length - 96 -or [BitConverter]::ToUInt32($bytes, $pe) -ne 0x4550) { throw "Invalid PE: $name" }
        if ([BitConverter]::ToUInt16($bytes, $pe + 4) -ne $Machine) { throw "Wrong architecture: $name" }
        $subsystem = if ($name -eq 'wordcraft.exe') { 2 } else { 3 }
        if ([BitConverter]::ToUInt16($bytes, $pe + 0x5c) -ne $subsystem) { throw "Wrong PE subsystem: $name" }
        Copy-Item -LiteralPath $path -Destination $Stage
    }
    & (Join-Path $PSScriptRoot 'sign.ps1') @($Binaries | ForEach-Object { Join-Path $Stage $_ })
    foreach ($name in @('README.md', 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'ATTRIBUTION.md')) {
        $destination = if ($name -eq 'README.md') { $Stage } else { "$Stage\licenses" }
        Copy-Item -LiteralPath (Join-Path $Root $name) -Destination $destination
    }
    foreach ($name in @('chatgpt.md', 'windows-installer.md')) {
        Copy-Item -LiteralPath (Join-Path $Root "docs\$name") -Destination $Stage
    }
    foreach ($file in Get-ChildItem -LiteralPath (Join-Path $Root 'assets\fonts') -Filter 'OFL-*.txt' -File) {
        Copy-Item -LiteralPath $file.FullName -Destination "$Stage\licenses"
    }
    if ($env:CRAFT_FONTS_DIR) {
        foreach ($file in Get-ChildItem -Path (Join-Path $env:CRAFT_FONTS_DIR 'fonts\*\OFL.txt') -File) {
            Copy-Item -LiteralPath $file.FullName -Destination "$Stage\licenses\OFL-$($file.Directory.Name).txt"
        }
    }
    # Include copyright/permission notices from the exact locked dependency sources.
    # Target-filtered Cargo metadata may also include build/test dependencies: extra notices
    # are harmless. It never includes tokens, local paths, manifests or source code in output.
    $metadataJson = & cargo metadata --locked --offline --format-version 1 --filter-platform $Target
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed' }
    $metadata = ($metadataJson -join "`n") | ConvertFrom-Json
    $overrides = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'license-overrides.json') -Raw | ConvertFrom-Json
    $notices = [Text.StringBuilder]::new()
    foreach ($package in ($metadata.packages | Sort-Object name, version)) {
        if ($package.id -in $metadata.workspace_members) { continue }
        [void] $notices.AppendLine("=== $($package.name) $($package.version) ===").AppendLine("License: $($package.license)").AppendLine("Source: $($package.repository)")
        $directory = Split-Path -Parent $package.manifest_path
        $files = @(Get-ChildItem -LiteralPath $directory -File -Recurse -Depth 4 | Where-Object { $_.Name -match '^(LICENSE|LICENCE|COPYING|COPYRIGHT|NOTICE)([._-]|$)|^(OFL|UFL)\.txt$' })
        if ($package.license_file) { $files += Get-Item -LiteralPath (Join-Path $directory $package.license_file) }
        # Git workspace crates may keep their licenses at the checkout root.
        if ($files.Count -eq 0 -and $package.source -like 'git+*') {
            $ancestor = $directory
            for ($level = 0; $level -lt 4 -and $ancestor; $level++) {
                $files += @(Get-ChildItem -LiteralPath $ancestor -File | Where-Object { $_.Name -match '^(LICENSE|LICENCE|COPYING|COPYRIGHT|NOTICE)([._-]|$)' })
                if ($files.Count -gt 0) { break }
                $ancestor = Split-Path -Parent $ancestor
            }
        }
        # Some published crates omit workspace-root license files. Reviewed copies from
        # their exact source revisions are kept in notices/, with provenance and hashes.
        $override = $overrides.PSObject.Properties["$($package.name) $($package.version)"]
        if ($override) {
            $noticeFile = Join-Path "$PSScriptRoot\notices" $override.Value.file
            $noticeText = [IO.File]::ReadAllText($noticeFile).Replace("`r`n", "`n")
            $hasher = [Security.Cryptography.SHA256]::Create()
            try { $digest = [BitConverter]::ToString($hasher.ComputeHash([Text.Encoding]::UTF8.GetBytes($noticeText))).Replace('-', '').ToLowerInvariant() }
            finally { $hasher.Dispose() }
            if ($digest -ne $override.Value.sha256) { throw "Notice integrity check failed: $($package.name)" }
            [void] $notices.AppendLine($noticeText)
        }
        if ($files.Count -eq 0 -and -not $override) { throw "No license notices found for $($package.name) $($package.version); add its notices before distribution." }
        foreach ($file in ($files | Sort-Object FullName -Unique)) {
            [void] $notices.AppendLine("--- $($file.Name) ---").AppendLine([IO.File]::ReadAllText($file.FullName))
        }
        [void] $notices.AppendLine()
    }
    [IO.File]::WriteAllText("$Stage\licenses\THIRD-PARTY-NOTICES.txt", $notices.ToString(), [Text.UTF8Encoding]::new($false))
    $license = "WordCraft - MIT OR Apache-2.0`r`n`r`nThis application is free and open source. You may choose either the MIT License or the Apache License, Version 2.0. This fork retains the original authors' notices. Third-party components retain their own licenses, installed in the licenses folder. ChatGPT is an optional online service governed by OpenAI's terms; no account or credentials are supplied with WordCraft.`r`n`r`n"
    foreach ($name in @('LICENSE-MIT', 'LICENSE-APACHE')) { $license += [IO.File]::ReadAllText((Join-Path $Root $name)) + "`r`n`r`n" }
    [IO.File]::WriteAllText("$Stage\license.txt", $license, [Text.UTF8Encoding]::new($true))
    $welcome = @'
WordCraft with ChatGPT

Write and edit documents with the native WordCraft editor and an optional ChatGPT sidebar.

Setup installs for all Windows users and requests administrator permission. The default folder is C:\Program Files\WordCraft; you can choose another drive. Rust, Cargo and a separate ChatGPT integration are not required to run the installed app.

ChatGPT is optional. After installation, open ChatGPT in WordCraft and choose Continue with ChatGPT to sign in with your own account. Internet access and an eligible account/model are required. Your requests and any document/file context you provide are sent to OpenAI when using this feature. Web search is enabled by default and can be disabled in ChatGPT settings.

No authentication JSON, tokens, chat history or personal documents are included in this installer. Login data is stored privately in your WordCraft user profile, separately from the installation folder. When started normally from its shortcut, the application runs with your normal Windows permissions. ChatGPT login belongs to each user individually.

Uninstalling preserves your documents, settings and saved login. To remove ChatGPT credentials, choose Sign out in WordCraft before uninstalling.
'@
    [IO.File]::WriteAllText("$Stage\welcome.txt", $welcome, [Text.UTF8Encoding]::new($true))
    # Original artwork, drawn in code; no bundled Office or Windows artwork.
    Add-Type -AssemblyName System.Drawing
    foreach ($size in @(@(192, 386, 'wizard.bmp'), @(64, 64, 'wizard-small.bmp'))) {
        $bitmap = [Drawing.Bitmap]::new($size[0], $size[1])
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        $graphics.Clear([Drawing.Color]::FromArgb(20, 36, 65))
        $icon = [Drawing.Image]::FromFile((Join-Path $Root 'assets\app-icon\wordcraft-1024.png'))
        try {
            $edge = if ($size[0] -gt 64) { 112 } else { 48 }
            $x = [int](($size[0] - $edge) / 2)
            $y = if ($size[0] -gt 64) { 64 } else { 8 }
            $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $graphics.DrawImage($icon, [Drawing.Rectangle]::new($x, $y, $edge, $edge))
            if ($size[0] -gt 64) {
                $font = [Drawing.Font]::new('Segoe UI', 18, [Drawing.FontStyle]::Bold)
                $small = [Drawing.Font]::new('Segoe UI', 10)
                $brush = [Drawing.SolidBrush]::new([Drawing.Color]::White)
                try {
                    $graphics.DrawString('WordCraft', $font, $brush, 22, 210)
                    $graphics.DrawString("Write. Edit. Create.`n`nWith ChatGPT", $small, $brush, 24, 252)
                } finally { $font.Dispose(); $small.Dispose(); $brush.Dispose() }
            }
            $bitmap.Save((Join-Path $Stage $size[2]), [Drawing.Imaging.ImageFormat]::Bmp)
        } finally { $icon.Dispose(); $graphics.Dispose(); $bitmap.Dispose() }
    }
    Invoke-Native 'Compile setup wizard' {
        & $IsccPath "/DAppVersion=$Version" "/DFileVersion=$FileVersion" "/DBuildArch=$Arch" "/DSetupArch=$SetupArch" "/DStageDir=$Stage" "/DOutputDir=$Dist" "/DIconPath=$Root\assets\app-icon\wordcraft.ico" (Join-Path $PSScriptRoot 'wordcraft.iss')
    }
    $Installer = Join-Path $Dist "wordcraft-$Version-windows-$Arch-setup.exe"
    & (Join-Path $PSScriptRoot 'sign.ps1') $Installer
    # Build-side inventory contains relative names and hashes only, no local source paths.
    $inventory = @(Get-ChildItem -LiteralPath $Stage -Recurse -File | ForEach-Object {
        [ordered]@{ path = $_.FullName.Substring($Stage.Length + 1).Replace('\', '/'); bytes = $_.Length; sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
    })
    $inventory | ConvertTo-Json | Set-Content -LiteralPath "$Installer.payload.json" -Encoding UTF8
    $hash = (Get-FileHash -LiteralPath $Installer -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $([IO.Path]::GetFileName($Installer))" | Set-Content -LiteralPath "$Installer.sha256" -Encoding ASCII
    Get-Item -LiteralPath $Installer | Select-Object Name, Length, FullName
} finally {
    $env:RUSTFLAGS = $oldFlags
    $env:WORDCRAFT_REQUIRE_WINRES = $oldRequireResources
    Pop-Location
}
