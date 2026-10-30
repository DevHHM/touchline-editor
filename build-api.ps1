[CmdletBinding()]
param([string]$OutputDirectory = 'dist\api')
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = [System.IO.Path]::GetFullPath($PSScriptRoot)
$packageDir = [System.IO.Path]::GetFullPath((Join-Path $projectRoot $OutputDirectory))
Push-Location $projectRoot
try {
    & cargo test --locked
    if ($LASTEXITCODE -ne 0) { throw 'Rust API tests failed.' }
    & cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Rust API build failed.' }
    New-Item -ItemType Directory -Path $packageDir -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $projectRoot 'target\release\touchline-editor.exe') -Destination (Join-Path $packageDir 'Touchline Engine.exe') -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'reference\local-api.md') -Destination (Join-Path $packageDir 'README.md') -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'reference\local-api-operations.json') -Destination (Join-Path $packageDir 'local-api-operations.json') -Force
    $licenseSource = Join-Path $projectRoot 'dist\wpf-0.10.2'
    if (Test-Path -LiteralPath (Join-Path $licenseSource 'licenses')) {
        $licenseDest = Join-Path $packageDir 'licenses'
        New-Item -ItemType Directory -Path $licenseDest -Force | Out-Null
        Get-ChildItem -LiteralPath (Join-Path $licenseSource 'licenses') -File | ForEach-Object {
            Copy-Item -LiteralPath $_.FullName -Destination $licenseDest -Force
        }
        Copy-Item -LiteralPath (Join-Path $licenseSource 'THIRD-PARTY-NOTICES.txt') -Destination (Join-Path $packageDir 'THIRD-PARTY-NOTICES.txt') -Force
    }
    Copy-Item -LiteralPath (Join-Path $projectRoot 'assets\fonts\OFL-Pretendard.txt') -Destination (Join-Path $packageDir 'OFL-Pretendard.txt') -Force
    $engine = Get-Item -LiteralPath (Join-Path $packageDir 'Touchline Engine.exe')
    [ordered]@{ protocol_version = 1; engine_sha256 = (Get-FileHash -LiteralPath $engine.FullName -Algorithm SHA256).Hash; engine_bytes = $engine.Length; built_utc = [DateTime]::UtcNow.ToString('o'); startup = '--api --port 0' } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $packageDir 'package-report.json') -Encoding utf8
    Write-Output "API package: $packageDir"
} finally { Pop-Location }
