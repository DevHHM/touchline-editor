# Third-party material

- `assets/fonts/Pretendard-*.otf`: unmodified Pretendard 1.3.9 by Kil Hyung-jin, SIL Open Font License
  1.1. Full license and source hashes are in `assets/fonts/OFL-Pretendard.txt` and `assets/fonts/SOURCES.txt`.
- `assets/icons/touchline.png` and `.ico`: original project artwork. The generation provenance is
  recorded in `assets/icons/generation.json`; it contains no club logo or game portrait pack.
- `WPF-UI` 4.3.0 is restored from NuGet and uses its upstream MIT license. .NET/WPF runtime files are
  restored from the .NET SDK; their Microsoft and third-party notices accompany the binary package.
- Rust dependencies are resolved through `Cargo.lock`, with license identifiers and source repositories
  available from `cargo metadata --locked`. The portable binary packager includes dependency notices.
  This includes egui/epaint default fonts, whose nested OFL, Ubuntu Font License, Hack and emoji font
  notices must accompany binaries embedding those fonts.
- Visual C++ runtime DLLs are included only by the binary packager, from an installed official Visual
  Studio redistribution folder, with its redistributable and third-party notices.

This source snapshot does not vendor the SDK, dependency source trees, runtime DLLs, FM/FMRTE binaries,
decompiler dumps, personal player portraits, club badges, saved games, authentication tokens or logs.
The short byte signatures and adapter offsets in our source are used to identify supported executable
builds before accessing live data; they are not a redistributed game implementation.

No project-wide license has been selected. The notices here preserve third-party attribution without
relicensing our application or any upstream component.
