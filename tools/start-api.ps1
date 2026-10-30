param(
    [string]$Engine = (Join-Path $PSScriptRoot '..\dist\api\Touchline Engine.exe'),
    [string]$Discovery = (Join-Path $env:LOCALAPPDATA 'TouchlineEditor\api.json'),
    [string]$Python = 'python',
    [int]$StartupTimeout = 45
)

$ErrorActionPreference = 'Stop'
$client = Join-Path $PSScriptRoot 'touchline_api.py'
if (-not (Test-Path -LiteralPath $Engine -PathType Leaf)) {
    throw "API 엔진이 없습니다: $Engine"
}
# The Python launcher uses CREATE_NO_WINDOW, reuses a healthy server, and never
# launches FM or submits a game-edit command. Output never includes the bearer token.
& $Python $client start --engine $Engine --discovery $Discovery --startup-timeout $StartupTimeout
if ($LASTEXITCODE -ne 0) {
    throw "Touchline API 실행 실패 (exit $LASTEXITCODE)."
}
