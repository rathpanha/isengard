# Install (or update) Isengard on Windows from the latest GitHub release:
#   irm https://raw.githubusercontent.com/rathpanha/isengard/master/scripts/install-windows.ps1 | iex
#
# Isengard is not code-signed (no paid certificate). A zip downloaded in a
# browser is blocked by SmartScreen ("Windows protected your PC"); files fetched
# by PowerShell carry no internet mark, so this installer avoids the warning.
# The download is checked against the release's SHA256SUMS, and its minisign
# signature when `minisign` is installed.
#
# Installs per user, no administrator rights: %LOCALAPPDATA%\Programs\Isengard,
# a Start Menu shortcut, and that folder added to your user PATH (`isengard .`).
# Env: VERSION (default: latest release).
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest is far faster without the bar
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Repo = 'rathpanha/isengard'
# Must match PUBLIC_KEY in src/update.rs.
$MinisignPubKey = 'RWQbB4mA17dCElWr4j6a897OzAzdEm5LyFvTfGCEd+2EKyoin6HtzQZH'

function Fail($msg) { Write-Host "error: $msg" -ForegroundColor Red; throw $msg }

if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { Fail 'only 64-bit Intel/AMD Windows builds are published' }

if ($env:VERSION) {
    $Tag = 'v' + $env:VERSION.TrimStart('v')
} else {
    $Tag = (Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ 'User-Agent' = 'isengard-installer' }).tag_name
    if (-not $Tag) { Fail 'could not find the latest release' }
}
$Version = $Tag.TrimStart('v')
$Asset = "isengard-$Version-windows-x86_64.zip"
$Base = "https://github.com/$Repo/releases/download/$Tag"

$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\Isengard'
if (Get-Process -Name isengard -ErrorAction SilentlyContinue) {
    Fail 'Isengard is running. Close it and run the installer again.'
}

$Tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("isengard-install-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $Tmp | Out-Null
try {
    Write-Host "Downloading Isengard $Version..."
    try { Invoke-WebRequest "$Base/$Asset" -OutFile (Join-Path $Tmp $Asset) -UseBasicParsing }
    catch { Fail "no Windows build in release $Tag" }
    Invoke-WebRequest "$Base/SHA256SUMS" -OutFile (Join-Path $Tmp 'SHA256SUMS') -UseBasicParsing

    if (Get-Command minisign -ErrorAction SilentlyContinue) {
        Invoke-WebRequest "$Base/SHA256SUMS.minisig" -OutFile (Join-Path $Tmp 'SHA256SUMS.minisig') -UseBasicParsing
        & minisign -Vq -P $MinisignPubKey -m (Join-Path $Tmp 'SHA256SUMS') -x (Join-Path $Tmp 'SHA256SUMS.minisig')
        if ($LASTEXITCODE -ne 0) { Fail 'SHA256SUMS signature does not verify' }
    } else {
        Write-Host 'note: minisign not installed; skipping signature check (scoop install minisign)'
    }

    $Expected = $null
    foreach ($line in Get-Content (Join-Path $Tmp 'SHA256SUMS')) {
        $parts = $line -split '\s+', 2
        if ($parts.Count -eq 2 -and ($parts[1].Trim().TrimStart('*') -replace '^\./', '') -eq $Asset) {
            $Expected = $parts[0].ToLower()
        }
    }
    if (-not $Expected) { Fail "$Asset is not listed in SHA256SUMS" }
    $Actual = (Get-FileHash (Join-Path $Tmp $Asset) -Algorithm SHA256).Hash.ToLower()
    if ($Actual -ne $Expected) { Fail "checksum mismatch for $Asset" }

    $Unpacked = Join-Path $Tmp 'unpacked'
    Expand-Archive -Path (Join-Path $Tmp $Asset) -DestinationPath $Unpacked
    if (-not (Test-Path (Join-Path $Unpacked 'isengard.exe'))) { Fail 'archive has no isengard.exe' }
    # No-op for PowerShell downloads; protects against odd setups.
    Get-ChildItem $Unpacked -Recurse -File | Unblock-File

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $InstallDir 'isengard.exe.old')
    Copy-Item (Join-Path $Unpacked '*') $InstallDir -Force

    $Exe = Join-Path $InstallDir 'isengard.exe'
    $Shortcut = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Isengard.lnk'
    $Link = (New-Object -ComObject WScript.Shell).CreateShortcut($Shortcut)
    $Link.TargetPath = $Exe
    $Link.WorkingDirectory = $InstallDir
    $Link.Save()

    $UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not (($UserPath -split ';') -contains $InstallDir)) {
        $NewPath = if ($UserPath) { "$UserPath;$InstallDir" } else { $InstallDir }
        [Environment]::SetEnvironmentVariable('Path', $NewPath, 'User')
        Write-Host 'Added the install folder to your PATH (open a new terminal to use `isengard`).'
    }
    Write-Host "Installed $Exe"
} finally {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $Tmp
}
