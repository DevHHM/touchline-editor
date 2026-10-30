$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    cargo test --locked
    if ($LASTEXITCODE -ne 0) { throw 'Tests failed.' }
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
    $packageDir = Join-Path $PSScriptRoot 'dist'
    New-Item -ItemType Directory -Path $packageDir -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'target\release\touchline-editor.exe') -Destination (Join-Path $packageDir 'Touchline Editor.exe') -Force
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'README.md') -Destination (Join-Path $packageDir 'README.md') -Force
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'assets\fonts\OFL-Pretendard.txt') -Destination (Join-Path $packageDir 'OFL-Pretendard.txt') -Force
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'assets\fonts\SOURCES.txt') -Destination (Join-Path $packageDir 'FONT-SOURCES.txt') -Force
    Write-Output (Join-Path $packageDir 'Touchline Editor.exe')
} finally { Pop-Location }
