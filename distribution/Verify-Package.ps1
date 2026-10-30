[CmdletBinding()]
param([string]$PackageDirectory = (Join-Path $PSScriptRoot '..'), [switch]$SkipLauncher)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = [IO.Path]::GetFullPath($PackageDirectory).TrimEnd('\', '/')
$prefix = $root + [IO.Path]::DirectorySeparatorChar
$manifestPath = Join-Path $root 'package-manifest.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
if ($manifest.layout -ne 'app-engine-v1' -or $manifest.algorithm -ne 'SHA256') { throw 'Unsupported package manifest.' }
$expected = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
[void]$expected.Add('package-manifest.json')
foreach ($file in $manifest.files) {
    $relative = [string]$file.path
    if ([IO.Path]::IsPathRooted($relative) -or $relative -match '(^|[\/])\.\.([\/]|$)' -or -not $expected.Add($relative)) { throw "Invalid or duplicate manifest path: $relative" }
    $path = [IO.Path]::GetFullPath((Join-Path $root $relative))
    if (-not $path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Manifest path escapes package.' }
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing file: $relative" }
    if ((Get-Item -LiteralPath $path).Length -ne [long]$file.bytes) { throw "Size mismatch: $relative" }
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $file.sha256) { throw "SHA256 mismatch: $relative" }
}
foreach ($file in Get-ChildItem -LiteralPath $root -Recurse -File -Force) {
    $relative = $file.FullName.Substring($prefix.Length).Replace('\', '/')
    if (-not $expected.Contains($relative)) { throw "Unexpected file: $relative" }
}
if (@(Get-ChildItem -LiteralPath $root -File -Filter '*.dll').Count -ne 0) { throw 'Runtime DLLs must remain in app/ or engine/.' }
if (-not $SkipLauncher) {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = Join-Path $root 'Touchline Editor.exe'
    $start.Arguments = '--launcher-check'
    $start.WorkingDirectory = [IO.Path]::GetTempPath()
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($start)
    try {
        if (-not $process.WaitForExit(10000)) { $process.Kill(); throw 'Launcher check timed out.' }
        $text = $process.StandardOutput.ReadToEnd()
        $errorText = $process.StandardError.ReadToEnd()
        if ($process.ExitCode -ne 0) { throw "Launcher check failed: $text $errorText" }
        $check = $text | ConvertFrom-Json
        if (-not $check.ok -or $check.layout -ne 'app-engine-v1') { throw 'Launcher rejected package layout.' }
    } finally { $process.Dispose() }
}
[ordered]@{ success = $true; version = $manifest.version; verified_files = $manifest.files.Count; layout = $manifest.layout; game_writes = $false } | ConvertTo-Json
