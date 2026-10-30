[CmdletBinding()]
param([Parameter(Mandatory)][string]$Destination, [string]$SourceDirectory = (Join-Path $PSScriptRoot '..\assets\licenses'))
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$sourceRoot = [IO.Path]::GetFullPath($SourceDirectory)
$manifest = Get-Content -LiteralPath (Join-Path $sourceRoot 'sources.json') -Raw | ConvertFrom-Json
foreach ($required in @('epaint-default-fonts-0.33.3-OFL.txt', 'epaint-default-fonts-0.33.3-UFL.txt', 'epaint-default-fonts-0.33.3-Hack-Regular.txt', 'epaint-default-fonts-0.33.3-emoji-icon-font-mit-license.txt')) {
    if (@($manifest | Where-Object { $_.file -eq $required }).Count -ne 1) { throw "Embedded font notice missing from source manifest: $required" }
}
New-Item -ItemType Directory -Path $Destination -Force | Out-Null
foreach ($entry in $manifest) {
    $name = [string]$entry.file
    if ($name -ne [IO.Path]::GetFileName($name) -or $name -match '[\\/]') { throw 'Supplemental license manifest must contain filenames only.' }
    $source = Join-Path $sourceRoot $name
    if ((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash -ne $entry.sha256) { throw "Supplemental notice changed: $name" }
    Copy-Item -LiteralPath $source -Destination (Join-Path $Destination $name)
}
Copy-Item -LiteralPath (Join-Path $sourceRoot 'sources.json') -Destination (Join-Path $Destination 'supplemental-SOURCES.json')
Copy-Item -LiteralPath (Join-Path $sourceRoot 'README.txt') -Destination (Join-Path $Destination 'supplemental-README.txt')
Write-Output "Supplemental upstream notices verified and copied: $($manifest.Count)"
