[CmdletBinding()]
param(
    [string]$OutputDirectory = '',
    [switch]$SkipTests,
    [switch]$SkipEngineBuild,
    [switch]$SkipZip
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = [System.IO.Path]::GetFullPath($PSScriptRoot)
$localDotnet = Join-Path $projectRoot 'tools\dotnet\dotnet.exe'
$dotnetPath = if (Test-Path -LiteralPath $localDotnet) { $localDotnet } else { (Get-Command dotnet -ErrorAction Stop).Source }
$dotnetRoot = Split-Path -Parent $dotnetPath
[xml]$uiProjectMetadata = Get-Content -LiteralPath (Join-Path $projectRoot 'desktop\Touchline.Wpf\Touchline.Wpf.csproj') -Raw
$packageVersion = [string]$uiProjectMetadata.Project.PropertyGroup.Version
if ([string]::IsNullOrWhiteSpace($packageVersion)) { throw 'WPF package version is missing.' }
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) { $OutputDirectory = "dist\wpf-$packageVersion" }
$packageDir = [IO.Path]::GetFullPath((Join-Path $projectRoot $OutputDirectory))
if ((Test-Path -LiteralPath $packageDir) -and @(Get-ChildItem -LiteralPath $packageDir -Force).Count -gt 0) {
    throw "Package destination is not empty. Choose a new folder; running/previous releases are never overwritten: $packageDir"
}
$appDir = Join-Path $packageDir 'app'
$engineDir = Join-Path $packageDir 'engine'
$docsDir = Join-Path $packageDir 'docs'
$apiDir = Join-Path $packageDir 'api'
$zipPath = Join-Path (Split-Path -Parent $packageDir) "Touchline-Editor-$packageVersion-win-x64.zip"
if (-not $SkipZip -and ((Test-Path -LiteralPath $zipPath) -or (Test-Path -LiteralPath "$zipPath.sha256"))) {
    throw 'Release ZIP already exists; use a new output parent or preserve/rename the old ZIP first.'
}
$buildEnvironment = @{}
foreach ($key in @('DOTNET_ROOT', 'DOTNET_ROOT_X64', 'DOTNET_CLI_TELEMETRY_OPTOUT', 'DOTNET_NOLOGO', 'PATH', 'CARGO_ENCODED_RUSTFLAGS')) {
    $buildEnvironment[$key] = [Environment]::GetEnvironmentVariable($key, 'Process')
}

function Assert-NativeSuccess([string]$Step) {
    if ($LASTEXITCODE -ne 0) { throw "$Step failed (exit $LASTEXITCODE)." }
}

function Copy-RequiredFile([string]$Source, [string]$Destination) {
    if (-not (Test-Path -LiteralPath $Source -PathType Leaf)) { throw "Required package file missing: $Source" }
    Copy-Item -LiteralPath $Source -Destination $Destination -Force
}

Push-Location $projectRoot
try {
    $env:DOTNET_ROOT = $dotnetRoot
    $env:DOTNET_ROOT_X64 = $dotnetRoot
    $env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
    $env:DOTNET_NOLOGO = '1'
    $env:PATH = "$dotnetRoot;$($buildEnvironment['PATH'])"
    $rustFlags = [Collections.Generic.List[string]]::new()
    if ($env:CARGO_ENCODED_RUSTFLAGS) {
        foreach ($flag in $env:CARGO_ENCODED_RUSTFLAGS.Split([char]31)) { $rustFlags.Add($flag) }
    } elseif ($env:RUSTFLAGS) {
        throw 'Set custom Rust flags through CARGO_ENCODED_RUSTFLAGS for this portable builder; plain RUSTFLAGS cannot safely preserve paths with spaces.'
    }
    $rustFlags.Add("--remap-path-prefix=$projectRoot=/touchline")
    $rustFlags.Add("--remap-path-prefix=$env:USERPROFILE=/build-user")
    $env:CARGO_ENCODED_RUSTFLAGS = $rustFlags -join [char]31

    if (-not $SkipTests) {
        cargo test --locked
        Assert-NativeSuccess 'Rust tests'
    }
    if (-not $SkipEngineBuild) {
        cargo build --release --locked
        Assert-NativeSuccess 'Rust release build'
    }

    $enginePath = Join-Path $projectRoot 'target\release\touchline-editor.exe'
    $snapshotPath = Join-Path $projectRoot 'verification-v2\index.json'
    # Keep release verification separate from an older running test helper.
    $clientTestArtifacts = Join-Path $projectRoot "output\client-tests-$packageVersion"
    $clientTestArguments = @('run', '--project', 'desktop\Touchline.ClientTests\Touchline.ClientTests.csproj', '--configuration', 'Release', '--artifacts-path', $clientTestArtifacts, '--')
    if (Test-Path -LiteralPath $snapshotPath -PathType Leaf) {
        $clientTestArguments += @('--engine', $enginePath, '--snapshot', $snapshotPath)
    }
    if (-not $SkipTests) {
        & $dotnetPath @clientTestArguments
        Assert-NativeSuccess 'Native client tests'
        & $dotnetPath run --project 'desktop\Touchline.UpdaterTests\Touchline.UpdaterTests.csproj' --configuration Release
        Assert-NativeSuccess 'Offline updater safety tests'
    }

    # Restore the Windows runtime explicitly. The lock file records this resolved
    # runtime target; publication below uses exactly those restored assets.
    $uiProject = Join-Path $projectRoot 'desktop\Touchline.Wpf\Touchline.Wpf.csproj'
    & $dotnetPath restore $uiProject --runtime win-x64 -p:SelfContained=true
    Assert-NativeSuccess 'WPF and Windows runtime restore'
    foreach ($directory in @($packageDir, $appDir, $engineDir, $docsDir, $apiDir)) { New-Item -ItemType Directory -Path $directory -Force | Out-Null }
    & $dotnetPath publish $uiProject --configuration Release --runtime win-x64 --self-contained true --no-restore --output $appDir -p:PublishSingleFile=false -p:PublishTrimmed=false -p:DebugType=None -p:DebugSymbols=false "-p:PathMap=$projectRoot=/touchline"
    Assert-NativeSuccess 'WPF self-contained publish'

    # The helper must run outside app/ while that entire folder is replaced.
    # Ship a self-contained single executable and copy it to the update job on exit.
    $updaterProject = Join-Path $projectRoot 'desktop\Touchline.Updater\Touchline.Updater.csproj'
    $updaterOutput = Join-Path $projectRoot ("output\updater-publish-$packageVersion-" + [Guid]::NewGuid().ToString('N'))
    & $dotnetPath publish $updaterProject --configuration Release --runtime win-x64 --self-contained true --output $updaterOutput -p:PublishSingleFile=true -p:IncludeNativeLibrariesForSelfExtract=true -p:DebugType=None -p:DebugSymbols=false "-p:Version=$packageVersion" "-p:PathMap=$projectRoot=/touchline"
    Assert-NativeSuccess 'Self-contained updater publish'
    Copy-RequiredFile (Join-Path $updaterOutput 'Touchline Updater.exe') (Join-Path $appDir 'Touchline Updater.exe')
    $helperCheck = Start-Process -FilePath (Join-Path $appDir 'Touchline Updater.exe') -ArgumentList '--helper-check' -WindowStyle Hidden -Wait -PassThru
    if ($helperCheck.ExitCode -ne 0) { throw 'Packaged updater helper check failed.' }

    Copy-RequiredFile $enginePath (Join-Path $engineDir 'Touchline Engine.exe')
    & (Join-Path $projectRoot 'tools\build-launcher.ps1') -OutputDirectory $packageDir -Version $packageVersion
    Copy-RequiredFile (Join-Path $projectRoot 'distribution\README.txt') (Join-Path $packageDir 'README.txt')
    $readmePath = Join-Path $packageDir 'README.txt'
    [IO.File]::WriteAllText($readmePath, [IO.File]::ReadAllText($readmePath).Replace('{{VERSION}}', $packageVersion), [Text.UTF8Encoding]::new($false))
    Copy-RequiredFile (Join-Path $projectRoot 'distribution\diagnostics-collector.json') (Join-Path $appDir 'diagnostics-collector.json')
    foreach ($name in @('QUICKSTART.ko.md', 'SUPPORT.ko.md', 'VERIFICATION.ko.md', 'UPDATES.ko.md', 'DIAGNOSTICS.ko.md', 'SHORTLISTS.ko.md', 'Verify-Package.ps1')) {
        Copy-RequiredFile (Join-Path $projectRoot "distribution\$name") (Join-Path $docsDir $name)
    }
    $releaseNotes = Join-Path $projectRoot "distribution\RELEASE-$packageVersion.ko.md"
    if (Test-Path -LiteralPath $releaseNotes -PathType Leaf) {
        Copy-RequiredFile $releaseNotes (Join-Path $docsDir 'RELEASE.ko.md')
    }
    Copy-RequiredFile (Join-Path $projectRoot 'tools\touchline_api.py') (Join-Path $apiDir 'touchline_api.py')
    Copy-RequiredFile (Join-Path $projectRoot 'distribution\start-api.ps1') (Join-Path $apiDir 'start-api.ps1')

    $licenseDir = Join-Path $packageDir 'licenses'
    New-Item -ItemType Directory -Path $licenseDir -Force | Out-Null
    Copy-RequiredFile (Join-Path $projectRoot 'assets\fonts\OFL-Pretendard.txt') (Join-Path $licenseDir 'OFL-Pretendard.txt')
    Copy-RequiredFile (Join-Path $projectRoot 'assets\fonts\SOURCES.txt') (Join-Path $licenseDir 'FONT-SOURCES.txt')
    $assetsPath = Join-Path $projectRoot 'desktop\Touchline.Wpf\obj\project.assets.json'
    $resolvedAssets = Get-Content -LiteralPath $assetsPath -Raw | ConvertFrom-Json
    $packageRoots = @($resolvedAssets.packageFolders.PSObject.Properties.Name)
    $noticeEntries = [System.Collections.Generic.List[string]]::new()
    $noticeEntries.Add("Touchline Editor $packageVersion uses the following third-party components. Full notices are included beside this file and in licenses/.")
    $noticeEntries.Add('Pretendard v1.3.9: SIL Open Font License 1.1. See licenses/OFL-Pretendard.txt and licenses/FONT-SOURCES.txt.')
    $copiedWpfLicense = $false
    foreach ($library in $resolvedAssets.libraries.PSObject.Properties) {
        if ($library.Value.type -ne 'package') { continue }
        $packageKey = $library.Name.ToLowerInvariant()
        foreach ($cacheRoot in $packageRoots) {
            $cacheDir = Join-Path $cacheRoot $packageKey
            if (-not (Test-Path -LiteralPath $cacheDir -PathType Container)) { continue }
            $safeName = $packageKey.Replace('/', '-')
            $licenses = @(Get-ChildItem -LiteralPath $cacheDir -File | Where-Object { $_.Name -match '(?i)^(license|copying|notice|thirdpartynotices)' })
            foreach ($license in $licenses) {
                $destName = "$safeName-$($license.Name)"
                Copy-RequiredFile $license.FullName (Join-Path $licenseDir $destName)
                $noticeEntries.Add("$($library.Name): licenses/$destName")
                if ($packageKey -eq 'wpf-ui/4.3.0' -and $license.Name -eq 'LICENSE.md') { $copiedWpfLicense = $true }
            }
            break
        }
    }
    if (-not $copiedWpfLicense) { throw 'WPF UI MIT license was not found in the restored NuGet package cache.' }

    # The SDK installation carries .NET notices even when runtime packages put
    # theirs in a different internal path. Preserve these full texts as well.
    Copy-RequiredFile (Join-Path $dotnetRoot 'LICENSE.txt') (Join-Path $licenseDir 'dotnet-LICENSE.txt')
    Copy-RequiredFile (Join-Path $dotnetRoot 'ThirdPartyNotices.txt') (Join-Path $licenseDir 'dotnet-ThirdPartyNotices.txt')
    $noticeEntries.Add('.NET Windows Desktop runtime: licenses/dotnet-LICENSE.txt and licenses/dotnet-ThirdPartyNotices.txt; any additional runtime package notices are also copied.')

    # Retain Rust dependency license texts available in Cargo's resolved cache.
    $cargoMetadataText = cargo metadata --locked --format-version 1
    Assert-NativeSuccess 'Rust dependency metadata'
    $cargoMetadata = $cargoMetadataText | ConvertFrom-Json
    foreach ($crate in $cargoMetadata.packages) {
        if ($null -eq $crate.source) { continue }
        $crateDir = Split-Path -Parent $crate.manifest_path
        $cratePrefix = "rust-$($crate.name)-$($crate.version)"
        $licenseFiles = @(Get-ChildItem -LiteralPath $crateDir -File | Where-Object { $_.Name -match '(?i)^(license|copying|notice|unlicense)' })
        foreach ($license in $licenseFiles) { Copy-RequiredFile $license.FullName (Join-Path $licenseDir "$cratePrefix-$($license.Name)") }
        $noticeEntries.Add("Rust $($crate.name) $($crate.version): $($crate.license); source $($crate.repository); license texts under licenses/$cratePrefix-* where supplied by the crate.")
    }
    & (Join-Path $projectRoot 'tools\Copy-Supplemental-Notices.ps1') -Destination $licenseDir
    $noticeEntries.Add('Additional required Rust and embedded-font notices: licenses/supplemental-README.txt and licenses/supplemental-SOURCES.json. Includes egui family MIT/Apache, Noto OFL, Ubuntu UFL, Hack/Bitstream Vera, emoji-icon-font, clipboard-win Boost, enum-map, profiling and gl-rs notices.')
    # Resolve native imports from the actual release image, not from a developer
    # machine's already-installed runtimes. Only official VS redist DLLs are used.
    $engineImports = @(& (Join-Path $projectRoot 'tools\Get-PeImports.ps1') $enginePath | Sort-Object -Unique)
    $requiredCrt = @($engineImports | Where-Object { $_ -match '^(?i)(vcruntime|msvcp|concrt)\d+.*\.dll$' })
    $bundledCrt = [Collections.Generic.List[string]]::new()
    if ($requiredCrt.Count -gt 0) {
        $crtCandidates = [Collections.Generic.List[object]]::new()
        foreach ($vsRoot in @((Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio'), (Join-Path $env:ProgramFiles 'Microsoft Visual Studio'))) {
            if (-not (Test-Path -LiteralPath $vsRoot -PathType Container)) { continue }
            foreach ($year in Get-ChildItem -LiteralPath $vsRoot -Directory) {
                foreach ($edition in Get-ChildItem -LiteralPath $year.FullName -Directory) {
                    $redistRoot = Join-Path $edition.FullName 'VC\Redist\MSVC'
                    if (-not (Test-Path -LiteralPath $redistRoot -PathType Container)) { continue }
                    foreach ($redistVersion in Get-ChildItem -LiteralPath $redistRoot -Directory) {
                        $x64 = Join-Path $redistVersion.FullName 'x64'
                        if (-not (Test-Path -LiteralPath $x64 -PathType Container)) { continue }
                        foreach ($crt in Get-ChildItem -LiteralPath $x64 -Directory -Filter 'Microsoft.VC*.CRT') {
                            $crtCandidates.Add([pscustomobject]@{ Path = $crt.FullName; Version = $redistVersion.Name; Vs = $edition.FullName })
                        }
                    }
                }
            }
        }
        $selectedCrt = $crtCandidates | Sort-Object Version -Descending | Where-Object {
            $candidate = $_.Path
            @($requiredCrt | Where-Object { -not (Test-Path -LiteralPath (Join-Path $candidate $_) -PathType Leaf) }).Count -eq 0
        } | Select-Object -First 1
        if ($null -eq $selectedCrt) { throw 'Required x64 Visual C++ runtime was not found in an official Visual Studio redist folder.' }
        $queue = [Collections.Generic.Queue[string]]::new()
        foreach ($name in $requiredCrt) { $queue.Enqueue($name) }
        while ($queue.Count -gt 0) {
            $name = $queue.Dequeue()
            if ($bundledCrt.Contains($name)) { continue }
            $source = Join-Path $selectedCrt.Path $name
            Copy-RequiredFile $source (Join-Path $engineDir $name)
            $bundledCrt.Add($name)
            foreach ($dependency in @(& (Join-Path $projectRoot 'tools\Get-PeImports.ps1') $source)) {
                if ($dependency -match '^(?i)(vcruntime|msvcp|concrt)\d+.*\.dll$' -and -not $bundledCrt.Contains($dependency)) { $queue.Enqueue($dependency) }
            }
        }
        $vsLicenseRoot = Join-Path $selectedCrt.Vs 'Licenses'
        $redistNotice = Get-ChildItem -LiteralPath $vsLicenseRoot -Filter Redist.txt -Recurse -File | Select-Object -First 1
        $thirdPartyNotice = Get-ChildItem -LiteralPath $vsLicenseRoot -Filter ThirdPartyNotices.txt -Recurse -File | Select-Object -First 1
        if ($null -eq $redistNotice -or $null -eq $thirdPartyNotice) { throw 'Visual C++ redistributable notices missing.' }
        Copy-RequiredFile $redistNotice.FullName (Join-Path $licenseDir 'visual-cpp-REDIST.txt')
        Copy-RequiredFile $thirdPartyNotice.FullName (Join-Path $licenseDir 'visual-cpp-ThirdPartyNotices.txt')
        $noticeEntries.Add("Microsoft Visual C++ x64 runtime $($selectedCrt.Version): $($bundledCrt -join ', '), copied from the official VC redistributable directory. See licenses/visual-cpp-REDIST.txt and licenses/visual-cpp-ThirdPartyNotices.txt.")
    }
    Set-Content -LiteralPath (Join-Path $packageDir 'THIRD-PARTY-NOTICES.txt') -Value $noticeEntries -Encoding utf8

    $appPath = Join-Path $packageDir 'Touchline Editor.exe'
    if (-not (Test-Path -LiteralPath $appPath -PathType Leaf)) { throw 'Native launcher missing after build.' }
    if (-not (Test-Path -LiteralPath (Join-Path $appDir 'coreclr.dll'))) { throw 'Self-contained .NET runtime missing.' }
    if (-not (Test-Path -LiteralPath (Join-Path $appDir 'PresentationFramework.dll'))) { throw 'Native WPF runtime missing.' }
    $files = @(Get-ChildItem -LiteralPath $packageDir -Recurse -File)
    $packageReport = [ordered]@{
        version = $packageVersion
        ui = 'C# / WPF + WPF UI'
        engine = 'Rust / semantic JSON pipes'
        target = 'win-x64'
        self_contained = $true
        chrome_required = $false
        webview_required = $false
        game_modified_during_build = $false
        layout = 'app-engine-v1'
        updater = 'github-release-sha256-v1'
        update_repository = 'DevHHM/touchline-editor'
        tests_run_by_builder = (-not $SkipTests)
        engine_built_by_builder = (-not $SkipEngineBuild)
        engine_imports = $engineImports
        engine_bundled_runtime = @($bundledCrt)
        system_runtime = 'Windows 10/11 x64 includes UCRT and Windows API sets; these system DLLs are not redistributed.'
        inventory = 'package-manifest.json contains the final per-file size and SHA256 inventory.'
        executables = @(
            @{ name = 'Touchline Editor.exe'; sha256 = (Get-FileHash -LiteralPath $appPath -Algorithm SHA256).Hash },
            @{ name = 'app/Touchline Editor.exe'; sha256 = (Get-FileHash -LiteralPath (Join-Path $appDir 'Touchline Editor.exe') -Algorithm SHA256).Hash },
            @{ name = 'engine/Touchline Engine.exe'; sha256 = (Get-FileHash -LiteralPath (Join-Path $engineDir 'Touchline Engine.exe') -Algorithm SHA256).Hash }
        )
    }
    $packageReport | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $packageDir 'package-report.json') -Encoding utf8
    & (Join-Path $projectRoot 'tools\package-wpf.ps1') -PackageDirectory $packageDir -SkipZip:$SkipZip
    Write-Output $appPath
} finally {
    foreach ($key in $buildEnvironment.Keys) { [Environment]::SetEnvironmentVariable($key, $buildEnvironment[$key], 'Process') }
    Pop-Location
}
