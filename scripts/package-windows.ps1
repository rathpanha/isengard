# Build the Windows release zip: dist\windows\isengard-<ver>-windows-x86_64.zip
# (isengard.exe + LICENSE + credits). Unsigned: see scripts/install-windows.ps1.
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
