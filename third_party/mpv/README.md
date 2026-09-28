# libmpv binaries (not committed)

OneShotTV loads libmpv **dynamically at runtime** (`libloading`), so the Rust
build never links against it. Place the platform library here for development:

| Platform | Expected file | Source |
|---|---|---|
| Windows x64 | `windows-x64/libmpv-2.dll` | `tools/fetch-libmpv.ps1` (LGPL build from zhongfly/mpv-winbuild) |
| macOS (Apple silicon) | `macos-arm64/libmpv.2.dylib` + dependencies | `tools/bundle-libmpv-macos.mjs` (from Homebrew `mpv`) |
| Linux | system `libmpv.so.2` | distro package (`libmpv2` / `mpv-libs`) |

**Licensing:** Flick is GPL-3.0, so a GPL build of libmpv (with GPL-only
FFmpeg components) can be shipped as well as an LGPL one.

The runtime search order is: `ONESHOT_LIBMPV` env var → the app resource dir →
this folder (debug builds only) → system library path.
