# Build the Windows release zip: dist\windows\isengard-<ver>-windows-x86_64.zip
# (isengard.exe + MSVC CRT DLLs + LICENSE + credits). Unsigned: see
# scripts/install-windows.ps1. CRT DLLs ship beside the exe so clean PCs do not
# need a separate VC++ redistributable (and we avoid +crt-static, which linked
# but crashed on open with GPUI/D3D).
# Env: VERSION (default: Cargo.toml version).
$ErrorActionPreference = 'Stop'

$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

$Version = $env:VERSION
if (-not $Version) {
    $Version = (Select-String -Path 'Cargo.toml' -Pattern '^version = "(.*)"' |
        Select-Object -First 1).Matches[0].Groups[1].Value
}

cargo build --release
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

$Dist = Join-Path $Root 'dist\windows'
if (Test-Path $Dist) { Remove-Item -Recurse -Force $Dist }
New-Item -ItemType Directory -Path $Dist | Out-Null

$Stage = Join-Path ([System.IO.Path]::GetTempPath()) ("isengard-stage-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $Stage | Out-Null
try {
    Copy-Item (Join-Path $Root 'target\release\isengard.exe') $Stage
    Copy-Item (Join-Path $Root 'LICENSE') $Stage
    Copy-Item (Join-Path $Root 'docs\credits.md') $Stage

    # App-local MSVC CRT (vcruntime140.dll was the clean-PC missing DLL).
    $Vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $Vswhere)) { throw "vswhere not found at $Vswhere" }
    $VsRoot = & $Vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $VsRoot) { throw 'Visual Studio with VC tools not found (needed for CRT DLLs)' }
    $CrtDir = Get-ChildItem (Join-Path $VsRoot 'VC\Redist\MSVC') -Directory |
        ForEach-Object { Get-ChildItem (Join-Path $_.FullName 'x64\Microsoft.VC*.CRT') -Directory -ErrorAction SilentlyContinue } |
        Sort-Object FullName -Descending |
        Select-Object -First 1
    if (-not $CrtDir) { throw "no x64 Microsoft.VC*.CRT under $VsRoot\VC\Redist\MSVC" }
    foreach ($name in @('vcruntime140.dll', 'vcruntime140_1.dll', 'msvcp140.dll')) {
        $src = Join-Path $CrtDir.FullName $name
        if (-not (Test-Path $src)) { throw "missing CRT DLL: $src" }
        Copy-Item $src $Stage
    }
    Write-Host "Bundled CRT from $($CrtDir.FullName)"

    $Zip = Join-Path $Dist "isengard-$Version-windows-x86_64.zip"
    Compress-Archive -Path (Join-Path $Stage '*') -DestinationPath $Zip
} finally {
    Remove-Item -Recurse -Force $Stage
}

Get-FileHash $Zip -Algorithm SHA256 | ForEach-Object {
    "$($_.Hash.ToLower())  ./$(Split-Path -Leaf $Zip)" | Set-Content -Encoding ascii (Join-Path $Dist 'SHA256SUMS')
}
Write-Host "Artifacts in ${Dist}:"
Get-ChildItem $Dist | Format-Table Name, Length
