# Building Touchline Editor

The Windows desktop UI uses C# / WPF with WPF UI. The data and editing engine is Rust.
The normal GUI does not need Python, Chrome, WebView2 or a cloud service.

## Prerequisites

- Windows 10/11 x64.
- Stable Rust with the `x86_64-pc-windows-msvc` target, Cargo and Rust 2024 edition support.
- Visual Studio C++ Build Tools, the Windows SDK (`rc.exe`), and the x64 Visual C++ redistributable files supplied with Visual Studio.
- .NET 10 SDK on `PATH`. Do not copy another user's `tools/dotnet` folder.
- Internet access for Cargo/NuGet dependency restore on the first build.
- Python 3 only if you want to run the optional semantic API client tests.

Run all commands from the extracted source repository root. Keep its folder structure.

## Checks

```powershell
cargo test --locked
dotnet run --project desktop/Touchline.ClientTests/Touchline.ClientTests.csproj --configuration Release
dotnet run --project desktop/Touchline.UpdaterTests/Touchline.UpdaterTests.csproj --configuration Release
python -m unittest discover -s tools -p test_touchline_api.py
```

The client tests without `--engine`/`--snapshot` run their portable tests and synthetic bridge fixture.
The author's private snapshots, game saves and local graphics are intentionally not shipped. Optional
ignored tests requiring a running game or a local graphics pack are not part of the default test run.
The ignored graphics test accepts a comma-separated `TOUCHLINE_TEST_PORTRAIT_IDS` environment variable.

## Portable Windows release

```powershell
powershell -NoProfile -File .\build-wpf.ps1
```

The builder runs tests, builds the Rust engine, publishes the self-contained WPF app, builds the native
launcher and creates `dist/Touchline-Editor-0.14.3-win-x64.zip` with its SHA-256 sidecar. The output folder
must be empty; existing release folders and ZIP files are not overwritten. The build uses a locally
installed `dotnet` SDK; its SDK directory must contain the official .NET license/third-party notices.

For a development build without the portable packager:

```powershell
cargo build --release --locked
dotnet build desktop/Touchline.Wpf/Touchline.Wpf.csproj --configuration Release
```

The release packager arranges files as a launcher plus `app/`, `engine/`, `api/`, `docs/` and `licenses/`.
Do not distribute the raw Cargo executable alone: its transitive dependency notices and native runtime
requirements still apply. The `build-release.ps1` script is a legacy egui-only developer convenience,
not the supported public WPF release packaging workflow.

## Optional API

The portable package includes `api/touchline_api.py` and `engine/Touchline Engine.exe`. See
[`reference/local-api.md`](reference/local-api.md). For source development, pass `--engine` explicitly
to the client after building the Rust engine, or build the optional API package using `build-api.ps1`.
The server binds only to loopback, creates per-session authentication locally, and does not advance,
save or load FM. Never commit its discovery file, authentication token, saves or snapshots.

## Game compatibility

The source adapters check exact build identities. FM27 support is limited to the verified build and
field families described in [`distribution/SUPPORT.ko.md`](distribution/SUPPORT.ko.md). A successful
build does not mean every game update or write operation has been tested. No game installation files,
FMRTE executable/code dumps or personal game data are needed to compile this repository.

## License status

No project-level license was present in the original project and this source snapshot does not add
one. Making the source visible is not a grant of an MIT/Apache or other open-source license. The
upstream licenses of bundled fonts and restored dependencies continue to apply. See
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md).

## Optional diagnostic collector

The separate Cloudflare Worker source is in `services/diagnostics-collector`. See its README for deployment. The source includes no server secrets. Run `npm ci` and `npm test` there with Node 22 or newer. Local credential entry helpers and local Wrangler state are intentionally excluded.

## Unshown WPF layout checks

`desktop/Touchline.UiScaleTests` exercises real WPF measure/arrange, screen scale,
text weight and apply guidance without showing windows, capturing the desktop,
starting the engine or accessing FM. Keep its temporary settings isolated:

```powershell
$previousLayoutAppData = $env:LOCALAPPDATA
try {
    $env:LOCALAPPDATA = [IO.Path]::GetFullPath('output/ui-layout-test-profile')
    dotnet run --project desktop/Touchline.UiScaleTests/Touchline.UiScaleTests.csproj --configuration Release -- output/ui-layout-test-results.json
} finally {
    $env:LOCALAPPDATA = $previousLayoutAppData
}
```
