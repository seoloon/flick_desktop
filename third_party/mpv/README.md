# libmpv binaries (not committed)

OneShotTV loads libmpv **dynamically at runtime** (`libloading`), so the Rust
build never links against it. Place the platform library here for development:

| Platform | Expected file | Source |
|---|---|---|
| Windows x64 | `windows-x64/libmpv-2.dll` | `tools/fetch-libmpv.ps1` (LGPL build from zhongfly/mpv-winbuild) |
| macOS (Apple silicon) | `macos-arm64/libmpv.2.dylib` + dependencies | `tools/bundle-libmpv-macos.mjs` (from Homebrew `mpv`) |
| Linux | system `libmpv.so.2` | distro package (`libmpv2` / `mpv-libs`) |

**ffmpeg** (converts titles for AirPlay receivers that cannot play them as they are):

| Platform | Expected file | Source |
|---|---|---|
| Windows x64 | `../ffmpeg/windows-x64/ffmpeg.exe` (shipped next to the DLL) | `tools/fetch-ffmpeg.ps1` (GPL build from BtbN/FFmpeg-Builds) |
| macOS (Apple silicon) | `../ffmpeg/macos-arm64/ffmpeg` with its own dylibs (shipped in the resource folder `ffmpeg/`) | `tools/bundle-libmpv-macos.mjs` (from Homebrew `ffmpeg-full`, which has libass) |
| Linux | `ffmpeg` on the `PATH` | distro package |

The search order is: `ONESHOT_FFMPEG` env var → the same folders as libmpv
(executable's folder, resource `libmpv/`, and in debug builds the folders here)
→ the `PATH`.

**Licensing:** Flick is GPL-3.0, so a GPL build of libmpv (with GPL-only
FFmpeg components) can be shipped as well as an LGPL one.

The runtime search order is: `ONESHOT_LIBMPV` env var → the app resource dir →
this folder (debug builds only) → system library path.

On macOS the app is signed ad hoc with the hardened runtime, so the bundled
dylibs only load thanks to the entitlements in `app/entitlements.macos.plist`
(library validation off; JIT allowed for libmpv's built-in LuaJIT scripts).
