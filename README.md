<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="app/icons/basic/flick-wordmark.svg">
    <img src="docs/assets/flick-wordmark-light.svg" alt="Flick" width="360">
  </picture>
</p>

<h3 align="center">A native desktop player for Jellyfin and Plex.</h3>

<p align="center">
  Every server in one library. Real Direct Play, HDR and lossless audio.<br>
  An interface made for the desk and for the couch.
</p>

<p align="center">
  <img alt="Windows" src="https://img.shields.io/badge/Windows-available-2ea043?style=flat-square&logo=windows&logoColor=white">
  <img alt="macOS" src="https://img.shields.io/badge/macOS-available-2ea043?style=flat-square&logo=apple&logoColor=white">
  <img alt="Built with Tauri and Rust" src="https://img.shields.io/badge/Tauri%20%2B%20Rust-native-24c8db?style=flat-square&logo=tauri&logoColor=white">
  <a href="LICENSE"><img alt="License: GPL-3.0" src="https://img.shields.io/badge/license-GPL--3.0-blue?style=flat-square"></a>
</p>

---

## Why Flick

Most media clients play your films inside a web page, and a web page decides
what your server has to transcode. Flick plays them the way a dedicated
player does. The files go straight to **mpv**, a real playback engine, and the
video is drawn by the GPU underneath a fluid, animated interface. The
interface never touches the picture.

The result: the file you stored is the file you watch.

## Highlights

### 🗂️ One library, all your servers
Connect as many **Jellyfin** and **Plex** servers as you like. Flick merges
them into a single library, with one Home and one search. A film that lives on
two servers shows up once, and a server that goes offline never blocks the
others.

### 🎬 Playback that respects the file
- **Direct Play first.** Your server is never asked to transcode because of
  the client.
- **GPU decoding** up to 4K: H.264, HEVC Main10, and AV1 when your GPU
  supports it.
- **HDR10 and HLG** sent to an HDR display, or tone-mapped properly on an SDR
  one. **Dolby Vision** profiles 5 and 8 are reshaped to HDR10.
- **Audio passthrough** to your receiver: AC3, E-AC3, DTS, and on Windows
  DTS-HD, TrueHD and Atmos. Flick falls back to PCM when the receiver
  refuses.
- **ASS and PGS subtitles** rendered exactly, chapters, **Skip Intro and
  Recap**, and **Up Next**.
- **It explains itself.** Every playback decision comes with its reason, and
  what mpv actually does is checked against the plan.

### 📺 Flick Frame, the TV mode
One button turns the desktop app into a full-screen, ten-foot interface
inspired by tvOS. It has a tab bar, large artwork and focus-driven navigation
for a **gamepad, a remote or the keyboard**. A short launch animation covers
the switch.

### 👥 Who's watching?
Profiles for the whole household, built on your servers' own users or on
Flick's. The animated profile picker comes with an optional **PIN**, personal
settings for each profile, and a quick switch from the sidebar.

### ⭐ Favourites and Watchlist
Jellyfin favourites and your **Plex Watchlist** gathered in one place.

### 🎭 People
Tap an actor or a director to see a biography and photo from **TMDB**, every
title of theirs across all your servers, and what else they are known for.

### ✨ The little things
- **Picture-in-picture** and a mini player.
- **Continue Watching** and **Next Up** on Home.
- Ambient light taken from the artwork.
- Back navigation everywhere.

## Private by design

- **No telemetry.** Flick talks to your servers, to plex.tv for Plex
  accounts, and to TMDB only if you add a key.
- **No password is ever stored.** Jellyfin trades it once for a token, and
  Plex signs in with a PIN.
- **Credentials stay in your system's keychain**: server tokens, Plex
  accounts, the TMDB key. They never leave the native side of the app.
- **Profile PINs** are hashed with Argon2id. Wrong guesses lock the profile
  for longer and longer.
- **The interface is sealed off.** It cannot reach the internet: every image
  goes through the app itself, under a strict content security policy.

## Platforms

| Platform | Status |
|---|---|
| **Windows 10 / 11** (x64) | ✅ Available |
| **macOS** (Apple Silicon) | ✅ Available (SDR playback): [details and limits](docs/MACOS_PORT.md) |
| **Linux** | 🗺️ Designed, not started |

## Build it yourself

```sh
pnpm install
pnpm desktop      # run in development
pnpm build:win    # Windows installer
pnpm build:mac    # Apple silicon macOS app and DMG
```

Requirements, tooling, tests and repository layout are in
**[TECHNICAL.md](TECHNICAL.md)**.

## Honest limits

- **No true Dolby Vision signal.** No PC player on Windows or Linux can send
  one to a TV. Flick says "Dolby Vision → HDR10 (reshaping)", never
  "Dolby Vision".
- **TrueHD and DTS-HD passthrough are Windows-only.** macOS cannot bitstream
  them.
- **HDR on the display** depends on HDR being turned on in the OS. Flick
  detects when a display is capable but switched off, and tells you.

Everything that was measured, and on which hardware, is in
[docs/PLAYBACK_VALIDATION.md](docs/PLAYBACK_VALIDATION.md).

## Built with

[Tauri](https://tauri.app) · [Rust](https://www.rust-lang.org) ·
[mpv](https://mpv.io) (libmpv, libplacebo, FFmpeg) · [React](https://react.dev) ·
[Tailwind CSS](https://tailwindcss.com) · [Motion](https://motion.dev) ·
[Remotion](https://www.remotion.dev)

## License

Flick is free software under the [GNU General Public License v3.0](LICENSE).

Flick is an independent project. It is not affiliated with Jellyfin, Plex or
TMDB. Person data and images come from TMDB:

<a href="https://www.themoviedb.org"><img alt="TMDB" src="https://www.themoviedb.org/assets/2/v4/logos/v2/blue_short-8e7b30f73a4020692ccca9c88bafe5dcb6f8a62a4c6bc55cd9ba82bb2cd95f6c.svg" height="14"></a>
&nbsp;This product uses the TMDB API but is not endorsed or certified by TMDB.

## Notes from the dev

Built with Claude, fully brainstormed and thought by a human
