[CmdletBinding()]
param(
    [string]$Source = (Join-Path $PSScriptRoot '..\assets\icons\touchline.png'),
    [string]$Destination = (Join-Path $PSScriptRoot '..\assets\icons\touchline.ico'),
    [string]$PreviewDirectory = (Join-Path $PSScriptRoot '..\output\app-icon')
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

# Format conversion only: retain the generated artwork and its alpha channel.
# ICO embeds real PNG frames rather than stretching one bitmap at every DPI.
$sourcePath = [IO.Path]::GetFullPath($Source)
$destinationPath = [IO.Path]::GetFullPath($Destination)
$previewPath = [IO.Path]::GetFullPath($PreviewDirectory)
[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destinationPath)) | Out-Null
[IO.Directory]::CreateDirectory($previewPath) | Out-Null
$art = [Drawing.Bitmap]::FromFile($sourcePath)
try {
    if ($art.Width -ne $art.Height) { throw 'Application artwork must be square.' }
    if ($art.GetPixel(0, 0).A -ne 0) { throw 'Expected transparent outer corners.' }
    $frames = [Collections.Generic.List[object]]::new()
    foreach ($size in @(16, 20, 24, 32, 40, 48, 64, 96, 128, 256)) {
        $bitmap = [Drawing.Bitmap]::new($size, $size, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        $stream = [IO.MemoryStream]::new()
        try {
            $graphics.CompositingMode = [Drawing.Drawing2D.CompositingMode]::SourceCopy
            $graphics.CompositingQuality = [Drawing.Drawing2D.CompositingQuality]::HighQuality
            $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $graphics.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
            $graphics.Clear([Drawing.Color]::Transparent)
            $graphics.DrawImage($art, [Drawing.Rectangle]::new(0, 0, $size, $size), 0, 0, $art.Width, $art.Height, [Drawing.GraphicsUnit]::Pixel)
            $bitmap.Save($stream, [Drawing.Imaging.ImageFormat]::Png)
            $bytes = $stream.ToArray()
            [IO.File]::WriteAllBytes((Join-Path $previewPath "touchline-$size.png"), $bytes)
            $frames.Add([pscustomobject]@{ Size = $size; Bytes = $bytes })
        } finally {
            $stream.Dispose()
            $graphics.Dispose()
            $bitmap.Dispose()
        }
    }
    $file = [IO.File]::Create($destinationPath)
    $writer = [IO.BinaryWriter]::new($file)
    try {
        $writer.Write([uint16]0)
        $writer.Write([uint16]1)
        $writer.Write([uint16]$frames.Count)
        $offset = 6 + 16 * $frames.Count
        foreach ($frame in $frames) {
            $dimension = if ($frame.Size -eq 256) { 0 } else { $frame.Size }
            $writer.Write([byte]$dimension)
            $writer.Write([byte]$dimension)
            $writer.Write([byte]0)
            $writer.Write([byte]0)
            $writer.Write([uint16]1)
            $writer.Write([uint16]32)
            $writer.Write([uint32]$frame.Bytes.Length)
            $writer.Write([uint32]$offset)
            $offset += $frame.Bytes.Length
        }
        foreach ($frame in $frames) { $writer.Write([byte[]]$frame.Bytes) }
    } finally { $writer.Dispose(); $file.Dispose() }
    [pscustomobject]@{
        source = $sourcePath
        source_width = $art.Width
        source_height = $art.Height
        corner_alpha = $art.GetPixel(0, 0).A
        icon = $destinationPath
        sizes = @($frames | ForEach-Object { $_.Size })
        sha256 = (Get-FileHash -LiteralPath $destinationPath -Algorithm SHA256).Hash
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $previewPath 'icon-package.json') -Encoding utf8
    Write-Output $destinationPath
} finally { $art.Dispose() }
