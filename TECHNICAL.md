# Flick — technical notes

How to build, run and test Flick, and where things live. For what Flick
*is*, see the [README](README.md).

## Further reading

The design documents are written in French.

| Document | What it covers |
|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | Why each choice was made, and what is guaranteed or not: playback engine, video under the UI, HDR, audio, decision engine, security. |
| [docs/PLAYBACK_VALIDATION.md](docs/PLAYBACK_VALIDATION.md) | What was measured, on which hardware, and how to reproduce it. |
| [docs/DESIGN_SYSTEM.md](docs/DESIGN_SYSTEM.md) | Components, motion, Flick Frame, window chrome. |
| [docs/MACOS_PORT.md](docs/MACOS_PORT.md) | The macOS port: decisions, state, work left. |

## Requirements

- **Rust ≥ 1.85**, **Node ≥ 20**, **pnpm**.
- **libmpv ≥ 0.41**, loaded at runtime (the Rust build never links it):
  - **Windows**: downloaded automatically into `third_party/mpv/windows-x64`
    on the first `pnpm desktop` or build;
  - **macOS / Linux**: the system libmpv (`brew install mpv`, `libmpv2`…);
  - **anywhere**: `ONESHOT_LIBMPV=/path/to/libmpv` overrides the search.
- macOS builds: `rustup target add aarch64-apple-darwin x86_64-apple-darwin`
  for the universal app.

## Commands

Run them from the repository root.

```sh
pnpm install        # once
pnpm desktop        # tauri dev: Vite and the app, with hot reload
pnpm build          # production build for the current platform
pnpm build:win      # Windows x64 installers (.msi and NSIS .exe)
pnpm build:mac      # universal macOS app and DMG, signed ad hoc
pnpm test           # Rust tests, UI tests and UI typecheck
pnpm lint           # clippy (zero warnings) and UI typecheck
pnpm bindings       # regenerate the TypeScript types from Rust (ts-rs)
pnpm libmpv         # fetch or check libmpv only
```

The macOS build is signed ad hoc, without an Apple Developer account. It is
not notarised, so on another Mac it is opened for the first time with
right-click › Open, or with *System Settings › Privacy & Security › Open
Anyway*.

## Diagnostics (optional)

```sh
cargo run -p oneshot-capabilities --example report         # displays/HDR, audio/passthrough, GPU decoders
bash tools/gen-test-media.sh                               # test corpus (ffmpeg) → test-media/
cargo run -p oneshot-mpv --example probe -- test-media/*   # what mpv really does with each file
```

## Tests against your own servers

These tests are ignored unless their variable is set:

```sh
# Jellyfin: an `oneshot` / `oneshot` account is expected
ONESHOT_JELLYFIN_URL=http://…:8096 cargo test -p oneshot-jellyfin --test live

# Plex: an unclaimed server reachable without a token
ONESHOT_PLEX_URL=http://…:32400 cargo test -p oneshot-plex --test live
```

## Repository layout

```
crates/core           domain model, capabilities, contracts (no I/O)
crates/mpv            libmpv FFI (loaded dynamically)
crates/capabilities   OS probes (DXGI/CCD HDR, D3D11 decoders, WASAPI IEC 61937)
crates/playback       decision engine (pure) and client profile
crates/player         playback session, video presentation, reconciliation, reporting
crates/catalog        multi-server aggregation, merging, caching
crates/storage        settings, profiles and PINs, OS keychain, SQLite and image caches
crates/net            shared HTTP policy
crates/providers/*    Jellyfin, Plex, TMDB
app/                  Tauri shell (commands, image protocol, window)
ui/                   React 19 + strict TypeScript (Tailwind v4, Motion, Norigin, Remotion)
tools/                dev scripts (libmpv fetch, test media, dev servers)
```

## Licensing

Flick is licensed under the [GPL-3.0](LICENSE). Because the application is
GPL, it can ship a **GPL build of libmpv** (with the GPL parts of FFmpeg)
as well as an LGPL one.
