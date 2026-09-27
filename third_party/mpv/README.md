# libmpv binaries (not committed)

OneShotTV loads libmpv **dynamically at runtime** (`libloading`), so the Rust
build never links against it. Place the platform library here for development:

| Platform | Expected file | Source |
|---|---|---|
| Windows x64 | `windows-x64/libmpv-2.dll` | `tools/fetch-libmpv.ps1` (LGPL build from zhongfly/mpv-winbuild) |
| macOS | `macos-universal/libmpv.2.dylib` | Homebrew `mpv` or a custom LGPL build |
| Linux | system `libmpv.so.2` | distro package (`libmpv2` / `mpv-libs`) |

**Licensing:** ship an **LGPL** build of libmpv (built with `-Dgpl=false`) so the
application can stay under a non-GPL licence. GPL builds pull in GPL-only
FFmpeg components and would force the whole app under GPL.

The runtime search order is: `ONESHOT_LIBMPV` env var → the app resource dir →
this folder (debug builds only) → system library path.
