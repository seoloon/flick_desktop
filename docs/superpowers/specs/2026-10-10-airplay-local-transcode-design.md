# AirPlay lot 2: local conversion with a bundled ffmpeg

## Goal

When an AirPlay receiver cannot play a title as it is, Flick converts it on this computer and the
receiver pulls the result from the local relay. Today `airplay_direct()` refuses such files
(`FLK-CAST-012`, "Flick does not convert it yet"). The media server converts nothing for AirPlay;
that stays true. Lot 1 (PIN pairing, encrypted channel) is unchanged.

## Decisions taken

- **ffmpeg comes as a sidecar executable.** The libmpv bundle only holds `libav*` libraries (macOS)
  or a DLL with FFmpeg linked statically (Windows): there is no `ffmpeg` to run. A GPL build is fine
  (Flick is GPL-3.0).
- **Output is HLS with fMP4 segments**, written to a temporary folder and served by the relay.
  AirPlay plays HLS and can seek in it; progressive MP4 cannot be seeked while it is being produced.
- **Least work per track:** copy what the receiver already plays, convert only the rest.

## 1. The ffmpeg binary

- `tools/ensure-libmpv.mjs` also provides ffmpeg: macOS copies Homebrew's `ffmpeg` with its dylibs
  next to the libmpv ones (`third_party/ffmpeg/macos-arm64/`); Windows downloads a GPL static build
  (`third_party/ffmpeg/windows-x64/ffmpeg.exe`); Linux uses the system one.
- Declared as a resource in `app/tauri.conf.json`.
- Lookup order, as for libmpv: `ONESHOT_FFMPEG` env var, app resource dir, repo folder (debug
  builds), `PATH`.
- macOS: the existing entitlements (library validation off) already allow the bundled dylibs.

## 2. `crates/cast/src/transcode.rs`

- `plan(source, audio_track, subtitle_track) -> Plan`: `Direct` (nothing to do), `Remux`
  (container only), or `Convert { video: Copy | H264, audio: Copy | Aac, burn_subtitle }`.
  - Video: copy H.264/HEVC progressive; otherwise H.264 (VideoToolbox on macOS when ffmpeg has it,
    else libx264), capped at 1080p and 12 Mb/s like Chromecast.
  - Audio: copy AAC/AC-3/E-AC-3/ALAC/MP3; otherwise AAC stereo. The track follows the same language
    preferences as local playback (`tracks::select_audio`).
  - Subtitles: burnt into the picture when the automatic selection picks one.
- `Job::start(plan, input_url, headers, dir)` runs ffmpeg and returns once the playlist and the
  first segment exist (or fails). It kills the process on drop.
- The input URL and headers (server credentials) are passed through ffmpeg's `-headers` and an
  argument file / environment, never on a command line visible in `ps`.
- A seek outside what has been produced restarts the job at the new position.

## 3. Relay

`Proxy::serve` takes a second kind of source: a local directory, served under the same secret path,
with the same `..` refusal as the upstream mode. `Caster::stop()` kills the job and deletes the
folder. `CastMedia` carries the plan result; `content_type` is `application/x-mpegURL` for HLS.

## 4. App wiring and UI

- `commands/cast.rs`: for AirPlay, `airplay_direct()` becomes the planner's input; the `CAST_FORMAT`
  refusal disappears except when ffmpeg is missing or the file has no usable video.
- `CastPanel.tsx`: the connecting state shows "Converting…" while the job starts.

## 5. Errors (new codes in `codes.rs`, doc regenerated)

| Code | Meaning | What to do |
|---|---|---|
| `FLK-CAST-015` | ffmpeg, which converts videos for AirPlay, was not found. | Reinstall Flick; when building it yourself, run `node tools/ensure-libmpv.mjs`. |
| `FLK-CAST-016` | The conversion for AirPlay stopped unexpectedly. | Retry; if it persists, try another version of the title (the log has ffmpeg's reason). |
| `FLK-CAST-017` | The conversion is too slow to play in real time. | Pick a lower quality, or watch it in Flick. |

`FLK-CAST-012` is reworded to "AirPlay cannot play this file and Flick cannot convert it (no
usable video)".

## 6. Tests

- Unit: `plan()` over a table of sources (direct, remux, H.264 copy + AAC convert, full convert,
  burnt subtitle).
- Relay: directory mode serves files, refuses `..` and a missing token.
- Integration: a short clip from `tools/gen-test-media.sh` converted to HLS; skipped without ffmpeg.

## Out of scope

HDR to SDR tone mapping, quality settings in the UI, AirPlay 2 mirroring, converting for Chromecast
(the server still does it).
