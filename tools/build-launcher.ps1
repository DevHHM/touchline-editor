[CmdletBinding()]
param([Parameter(Mandatory)][string]$OutputDirectory, [Parameter(Mandatory)][string]$Version)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$out = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $out -Force | Out-Null
$scratch = Join-Path $projectRoot ('output\launcher-build-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null
$rustc = (Get-Command rustc -ErrorAction Stop).Source
$sdk = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
$rc = Get-ChildItem -LiteralPath $sdk -Filter rc.exe -Recurse -File |
    Where-Object { $_.FullName -match '\\x64\\rc\.exe$' } |
    Sort-Object FullName -Descending | Select-Object -First 1
if ($null -eq $rc) { throw 'Windows SDK x64 rc.exe is required to embed the launcher icon and version.' }
$icon = (Join-Path $projectRoot 'assets\icons\touchline.ico').Replace('\', '\\')
$v = [Version]$Version
$resource = @"
#pragma code_page(65001)
1 ICON "$icon"
1 VERSIONINFO
FILEVERSION $($v.Major),$($v.Minor),$($v.Build),0
PRODUCTVERSION $($v.Major),$($v.Minor),$($v.Build),0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
 BLOCK "StringFileInfo"
 BEGIN
  BLOCK "040904b0"
  BEGIN
   VALUE "FileDescription", "Touchline Editor"
   VALUE "ProductName", "Touchline Editor"
   VALUE "FileVersion", "$Version"
   VALUE "ProductVersion", "$Version"
   VALUE "OriginalFilename", "Touchline Editor.exe"
  END
 END
 BLOCK "VarFileInfo"
 BEGIN
  VALUE "Translation", 0x409, 1200
 END
END
"@
$resourcePath = Join-Path $scratch 'launcher.rc'
$res = Join-Path $scratch 'launcher.res'
Set-Content -LiteralPath $resourcePath -Value $resource -Encoding utf8
& $rc.FullName /nologo /fo $res $resourcePath
if ($LASTEXITCODE -ne 0) { throw 'Launcher resource build failed.' }
$builtExe = Join-Path $scratch 'Touchline Editor.exe'
& $rustc (Join-Path $PSScriptRoot 'launcher.rs') --edition 2024 --crate-name touchline_launcher `
    -C opt-level=s -C lto=thin -C strip=symbols -C debuginfo=0 -C target-feature=+crt-static -C "link-arg=$res" -C link-arg=/DEBUG:NONE `
    "--remap-path-prefix=$projectRoot=/touchline" "--remap-path-prefix=$env:USERPROFILE=/build-user" -o $builtExe
if ($LASTEXITCODE -ne 0) { throw 'Native launcher build failed.' }
$exe = Join-Path $out 'Touchline Editor.exe'
Copy-Item -LiteralPath $builtExe -Destination $exe
Write-Output $exe
