[CmdletBinding()]
param(
    [string]$Engine = (Join-Path $PSScriptRoot '..\engine\Touchline Engine.exe'),
    [string]$Discovery = (Join-Path $env:LOCALAPPDATA 'TouchlineEditor\api.json'),
    [string]$Python = 'python',
    [int]$StartupTimeout = 45
)
$ErrorActionPreference = 'Stop'
$Engine = [IO.Path]::GetFullPath($Engine)
if (-not (Test-Path -LiteralPath $Engine -PathType Leaf)) { throw "API 엔진이 없습니다: $Engine" }
& $Python (Join-Path $PSScriptRoot 'touchline_api.py') start --engine $Engine --discovery $Discovery --startup-timeout $StartupTimeout
if ($LASTEXITCODE -ne 0) { throw "Touchline API 실행 실패 (exit $LASTEXITCODE)." }
