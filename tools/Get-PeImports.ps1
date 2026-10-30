param([Parameter(Mandatory)][string]$Path)
$ErrorActionPreference = 'Stop'
$bytes = [IO.File]::ReadAllBytes([IO.Path]::GetFullPath($Path))
function U16([int]$Offset) { [BitConverter]::ToUInt16($bytes, $Offset) }
function U32([int]$Offset) { [BitConverter]::ToUInt32($bytes, $Offset) }
if ($bytes.Length -lt 256 -or (U16 0) -ne 0x5a4d) { throw 'Not a PE image.' }
$pe = U32 0x3c
if ((U32 $pe) -ne 0x4550) { throw 'Invalid PE signature.' }
$count = U16 ($pe + 6)
$optional = $pe + 24
$sectionTable = $optional + (U16 ($pe + 20))
$magic = U16 $optional
$dataDirectory = $optional + $(if ($magic -eq 0x20b) { 112 } elseif ($magic -eq 0x10b) { 96 } else { throw 'Unsupported PE optional header.' })
$sections = for ($i = 0; $i -lt $count; $i++) {
    $s = $sectionTable + $i * 40
    @{ VirtualSize = (U32 ($s + 8)); Address = (U32 ($s + 12)); RawSize = (U32 ($s + 16)); Raw = (U32 ($s + 20)) }
}
function RvaToOffset([uint32]$Rva) {
    foreach ($s in $sections) {
        if ($Rva -ge $s.Address -and $Rva -lt $s.Address + [Math]::Max($s.VirtualSize, $s.RawSize)) {
            return [int]($Rva - $s.Address + $s.Raw)
        }
    }
    return [int]$Rva
}
$importRva = U32 ($dataDirectory + 8)
if ($importRva -eq 0) { return }
$descriptor = RvaToOffset $importRva
while ((U32 ($descriptor + 12)) -ne 0) {
    $nameStart = RvaToOffset (U32 ($descriptor + 12))
    $nameEnd = $nameStart
    while ($nameEnd -lt $bytes.Length -and $bytes[$nameEnd] -ne 0) { $nameEnd++ }
    [Text.Encoding]::ASCII.GetString($bytes, $nameStart, $nameEnd - $nameStart)
    $descriptor += 20
}
