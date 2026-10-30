[CmdletBinding()]
param([Parameter(Mandatory)][string]$PackageDirectory, [switch]$SkipZip)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = [IO.Path]::GetFullPath($PackageDirectory).TrimEnd('\', '/')
$prefix = $root + [IO.Path]::DirectorySeparatorChar
$report = Get-Content -LiteralPath (Join-Path $root 'package-report.json') -Raw | ConvertFrom-Json
if ($report.layout -ne 'app-engine-v1') { throw 'Unexpected package layout.' }
$version = [string]$report.version
$zipPath = Join-Path (Split-Path -Parent $root) "Touchline-Editor-$version-win-x64.zip"
if (-not $SkipZip -and ((Test-Path -LiteralPath $zipPath) -or (Test-Path -LiteralPath "$zipPath.sha256"))) { throw 'Release ZIP/hash already exists; existing releases are never overwritten.' }
# Inventory is generated after final QA documents are added. Do not keep stale
# build-time counts or self-referential report/manifest byte totals in the report.
$report.PSObject.Properties.Remove('files')
$report.PSObject.Properties.Remove('bytes')
$report | Add-Member -NotePropertyName inventory -NotePropertyValue 'package-manifest.json contains the final per-file size and SHA256 inventory.' -Force
$report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $root 'package-report.json') -Encoding utf8
$entries = @(Get-ChildItem -LiteralPath $root -Recurse -File -Force | Sort-Object FullName | ForEach-Object {
    $relative = $_.FullName.Substring($prefix.Length).Replace('\', '/')
    if ($relative -eq 'package-manifest.json') { return }
    if ($relative -notmatch '^(app/|engine/|docs/|api/|licenses/|Touchline Editor\.exe$|README\.txt$|THIRD-PARTY-NOTICES\.txt$|package-report\.json$)') { throw "Unexpected package file: $relative" }
    if ($relative -match '(?i)(\.pdb$|(^|/)(__pycache__|history|snapshots?|verification[^/]*)/|(^|/)(api\.json|index\.json)$)') { throw "Private/development file unexpectedly included: $relative" }
    if ($relative -notmatch '^(app|licenses)/' -and $_.Extension -in @('.json', '.md', '.txt', '.py', '.ps1')) {
        $text = [IO.File]::ReadAllText($_.FullName)
        if ($text -match '(?i)[a-z]:[\\/]+(Users|dev)[\\/]') { throw "Local development path in public file: $relative" }
    }
    [ordered]@{ path = $relative; bytes = $_.Length; sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
})
[ordered]@{ version = $version; layout = 'app-engine-v1'; algorithm = 'SHA256'; files = $entries } |
    ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $root 'package-manifest.json') -Encoding utf8
& (Join-Path $root 'docs\Verify-Package.ps1') -PackageDirectory $root
if (-not $SkipZip) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Compression.ZipFile]::CreateFromDirectory($root, $zipPath, [IO.Compression.CompressionLevel]::Optimal, $true)
    $zipHash = (Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content -LiteralPath "$zipPath.sha256" -Value "$zipHash  $([IO.Path]::GetFileName($zipPath))" -Encoding ascii
    Write-Output $zipPath
}
