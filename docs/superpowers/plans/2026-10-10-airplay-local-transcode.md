# AirPlay lot 2: local conversion with a bundled ffmpeg — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When an AirPlay receiver cannot play a title as it is, Flick converts it on this computer with a bundled ffmpeg and the receiver pulls the result (HLS, fMP4 segments) from the local relay.

**Architecture:** A pure planner (`transcode/plan.rs`) decides, per track, copy or convert. An argument builder (`transcode/args.rs`) turns the plan into an ffmpeg command line. A `Job` runs ffmpeg into a temp folder; a `Session` owns the job, a loopback relay that feeds ffmpeg the server's stream (so credentials never appear on a command line) and a LAN relay that serves the temp folder. A `Converting` receiver wraps `AirPlay`, shifts positions by the job's start offset and restarts the job (then reloads the stream) on a seek outside what is produced.

**Tech Stack:** Rust (tokio process, existing `Proxy`), ffmpeg sidecar (GPL build), Node/PowerShell bundling scripts, Tauri resources, React (one label).

**Spec:** `docs/superpowers/specs/2026-10-10-airplay-local-transcode-design.md`

## Global Constraints

- Errors: every user-visible error carries a `FLK-AREA-NNN` code from `crates/core/src/codes.rs`, raised with `Error::X(codes::NAME.tag("clear sentence"))`; `ERROR_IDENTIFIER.md` is regenerated with `UPDATE_ERROR_DOC=1 cargo test -p oneshot-core codes`. A code must be raised somewhere in the same task that adds it (a test fails otherwise).
- Server credentials are never on a command line: ffmpeg reads the source from a loopback relay URL.
- Output is HLS with fMP4 segments of 6 s, `-hls_playlist_type event`, written to a temp folder.
- Video: copy H.264 (8-bit, progressive) and HEVC; otherwise H.264 (`h264_videotoolbox` on macOS, `libx264 -preset veryfast` elsewhere), capped at 1080p, 12 Mb/s. Audio: copy AAC / AC-3 / E-AC-3 / ALAC; otherwise AAC stereo 192 kb/s.
- ffmpeg lookup order: `ONESHOT_FFMPEG`, then the same folders as libmpv (exe dir, `resources/libmpv`, `third_party/mpv/*` in debug), then `PATH`.
- ffmpeg is a GPL build (Flick is GPL-3.0); on Windows it is spawned with `CREATE_NO_WINDOW`.
- Tests that need a real ffmpeg skip themselves when none is found.

## Deviations from the spec

- The spec said ffmpeg would be copied to `third_party/ffmpeg/<platform>/`. On macOS it goes **next to libmpv** (`third_party/mpv/macos-arm64/ffmpeg`): the folder already holds the matching `libav*` 63.1.102 dylibs, so the binary adds almost nothing to the bundle. Windows keeps `third_party/ffmpeg/windows-x64/ffmpeg.exe`.
- Seeking: a seek outside the produced part restarts ffmpeg **and reloads the stream on the receiver** (new `/play`), since an event playlist cannot be seeked past its live edge. The spec's "restarts the job" did not say the receiver is reloaded.
- Subtitles that are external files (`external: true`) are not burnt in this lot.

## Review Focus

- ffmpeg missing: the local player must **not** be stopped; error `FLK-CAST-015` comes first (Task 6).
- ffmpeg exits at once (bad input, bad map) or never writes a segment: `FLK-CAST-016` / `FLK-CAST-017`, no hang, no orphan process (Tasks 3, 5).
- Stopping a cast, or the app quitting, leaves no ffmpeg process and no temp folder (Tasks 3, 5).
- A path like `/c/<token>/../../etc/passwd` or `..%2f` in the folder relay answers 404 (Task 4).
- Resume at the very end of a title / seek to 0 / seek backwards before the job's start (Task 5 `seek_plan` tests).
- A source with no audio, or whose selected audio is not the default (must not be `Direct`), or interlaced H.264 (Task 1).

## File structure

| File | Responsibility |
|---|---|
| `crates/cast/src/transcode/mod.rs` | module root, `missing_error()`, re-exports |
| `crates/cast/src/transcode/plan.rs` | `Plan`, `Convert`, `plan()` (pure) |
| `crates/cast/src/transcode/args.rs` | `args()` ffmpeg command line (pure) |
| `crates/cast/src/transcode/locate.rs` | finding the ffmpeg executable |
| `crates/cast/src/transcode/job.rs` | spawning ffmpeg, waiting for the first segment, killing it |
| `crates/cast/src/transcode/session.rs` | job + relays + temp dir + restart |
| `crates/cast/src/transcode/receiver.rs` | `Converting`: `Receiver` over `AirPlay` with offset and restart |
| `crates/cast/src/proxy.rs` | gains a folder mode (`serve_dir`) |
| `crates/cast/src/airplay.rs` | `/play` factored out, `AirPlay::load` |
| `crates/cast/src/lib.rs` | `CastMedia.convert`, `Caster::with_ffmpeg_dirs`, `can_convert`, wiring in `start` |
| `app/src/commands/cast.rs`, `app/src/main.rs` | planner call, ffmpeg dirs |
| `ui/src/features/player/CastPanel.tsx` | "Converting…" label |
| `tools/bundle-libmpv-macos.mjs`, `tools/fetch-ffmpeg.ps1`, `tools/ensure-libmpv.mjs`, `app/tauri.*.conf.json`, `third_party/mpv/README.md`, `TECHNICAL.md` | shipping the binary |

---

### Task 1: The planner

**Files:**
- Create: `crates/cast/src/transcode/mod.rs`, `crates/cast/src/transcode/plan.rs`
- Modify: `crates/cast/src/lib.rs` (add `pub mod transcode;`)

**Interfaces:**
- Produces:
  - `pub enum Plan { Direct, Convert(Convert) }`
  - `pub struct Convert { pub video_pos: usize, pub audio_pos: Option<usize>, pub video: VideoPlan, pub hevc: bool, pub audio: AudioPlan, pub burn: Option<Burn>, pub deinterlace: bool, pub scale_down: bool }`
  - `pub enum VideoPlan { Copy, H264 }`, `pub enum AudioPlan { Copy, Aac }`, `pub struct Burn { pub position: usize, pub bitmap: bool }`
  - `pub struct NoVideo;`
  - `pub fn plan(source: &MediaSource, audio: Option<&AudioStream>, subtitle: Option<&SubtitleStream>) -> Result<Plan, NoVideo>`
  - `positions are indexes into source.video / source.audio / source.subtitles`

- [ ] **Step 1: Create the module root**

`crates/cast/src/transcode/mod.rs`:

```rust
//! Converting a title on this computer for an AirPlay receiver that cannot play it as it is.

mod plan;

pub use plan::{AudioPlan, Burn, Convert, NoVideo, Plan, VideoPlan, plan};
```

In `crates/cast/src/lib.rs`, after `pub mod proxy;` add `pub mod transcode;`.

- [ ] **Step 2: Write the failing tests**

`crates/cast/src/transcode/plan.rs` (tests first; the implementation follows in Step 4):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use oneshot_core::stream::{AudioCodec, AudioStream, DynamicRange, MediaSource, SubtitleFormat, SubtitleStream, VideoCodec, VideoStream};

    fn video(codec: VideoCodec) -> VideoStream {
        VideoStream { index: 0, codec, profile: None, level: None, width: 1920, height: 1080, bit_depth: Some(8), frame_rate: Some(24.0), bitrate: None, range: DynamicRange::Sdr, interlaced: false, title: None, is_default: true }
    }
    fn audio(index: u32, codec: AudioCodec, is_default: bool) -> AudioStream {
        AudioStream { index, codec, profile: None, channels: 2, channel_layout: None, sample_rate: None, bitrate: None, spatial: None, language: None, title: None, is_default, is_commentary: false }
    }
    fn subtitle(index: u32, format: SubtitleFormat, external: bool) -> SubtitleStream {
        SubtitleStream { index, format, language: None, title: None, forced: false, hearing_impaired: false, is_default: false, external, delivery_path: None }
    }
    fn source(container: &str, v: VideoStream, a: Vec<AudioStream>, s: Vec<SubtitleStream>) -> MediaSource {
        MediaSource { id: "s".into(), name: None, container: Some(container.into()), size_bytes: None, bitrate: None, duration_ms: Some(1000), video: vec![v], audio: a, subtitles: s }
    }

    #[test]
    fn an_mp4_the_receiver_plays_is_direct() {
        let s = source("mov,mp4,m4a,3gp,3g2,mj2", video(VideoCodec::H264), vec![audio(1, AudioCodec::Aac, true)], vec![]);
        assert_eq!(plan(&s, s.default_audio(), None), Ok(Plan::Direct));
    }

    #[test]
    fn an_mkv_with_good_codecs_is_only_remuxed() {
        let s = source("matroska,webm", video(VideoCodec::Hevc), vec![audio(1, AudioCodec::Eac3, true)], vec![]);
        let Ok(Plan::Convert(c)) = plan(&s, s.default_audio(), None) else { panic!("not converted") };
        assert_eq!((c.video, c.audio, c.hevc), (VideoPlan::Copy, AudioPlan::Copy, true));
    }

    #[test]
    fn an_unsupported_audio_codec_is_converted_and_the_video_copied() {
        let s = source("matroska,webm", video(VideoCodec::H264), vec![audio(1, AudioCodec::DtsHd, true)], vec![]);
        let Ok(Plan::Convert(c)) = plan(&s, s.default_audio(), None) else { panic!("not converted") };
        assert_eq!((c.video, c.audio), (VideoPlan::Copy, AudioPlan::Aac));
    }

    #[test]
    fn an_unsupported_video_codec_is_encoded_to_h264() {
        let s = source("matroska,webm", video(VideoCodec::Av1), vec![audio(1, AudioCodec::Aac, true)], vec![]);
        let Ok(Plan::Convert(c)) = plan(&s, s.default_audio(), None) else { panic!("not converted") };
        assert_eq!((c.video, c.audio), (VideoPlan::H264, AudioPlan::Copy));
        assert!(!c.scale_down);
    }

    #[test]
    fn interlaced_and_10_bit_h264_are_encoded_and_a_big_picture_is_scaled() {
        let mut v = video(VideoCodec::H264);
        v.interlaced = true;
        let s = source("mp4", v, vec![audio(1, AudioCodec::Aac, true)], vec![]);
        let Ok(Plan::Convert(c)) = plan(&s, s.default_audio(), None) else { panic!("not converted") };
        assert_eq!(c.video, VideoPlan::H264);
        assert!(c.deinterlace);

        let mut v = video(VideoCodec::H264);
        v.bit_depth = Some(10);
        v.height = 2160;
        v.width = 3840;
        let s = source("mp4", v, vec![audio(1, AudioCodec::Aac, true)], vec![]);
        let Ok(Plan::Convert(c)) = plan(&s, s.default_audio(), None) else { panic!("not converted") };
        assert_eq!(c.video, VideoPlan::H264);
        assert!(c.scale_down);
    }

    #[test]
    fn a_track_other_than_the_default_audio_is_never_direct() {
        let s = source("mp4", video(VideoCodec::H264), vec![audio(1, AudioCodec::Aac, true), audio(2, AudioCodec::Aac, false)], vec![]);
        let Ok(Plan::Convert(c)) = plan(&s, s.audio.get(1), None) else { panic!("not converted") };
        assert_eq!(c.audio_pos, Some(1));
        assert_eq!(c.video, VideoPlan::Copy);
    }

    #[test]
    fn a_title_without_audio_is_planned_without_audio() {
        let s = source("matroska", video(VideoCodec::H264), vec![], vec![]);
        let Ok(Plan::Convert(c)) = plan(&s, None, None) else { panic!("not converted") };
        assert_eq!(c.audio_pos, None);
    }

    #[test]
    fn a_burnt_subtitle_forces_an_encode_and_bitmaps_are_flagged() {
        let s = source("mp4", video(VideoCodec::H264), vec![audio(1, AudioCodec::Aac, true)], vec![subtitle(2, SubtitleFormat::Srt, false), subtitle(3, SubtitleFormat::Pgs, false)]);
        let Ok(Plan::Convert(c)) = plan(&s, s.default_audio(), s.subtitles.first()) else { panic!("not converted") };
        assert_eq!((c.video, c.burn), (VideoPlan::H264, Some(Burn { position: 0, bitmap: false })));
        let Ok(Plan::Convert(c)) = plan(&s, s.default_audio(), s.subtitles.get(1)) else { panic!("not converted") };
        assert_eq!(c.burn, Some(Burn { position: 1, bitmap: true }));
    }

    #[test]
    fn an_external_subtitle_is_not_burnt() {
        let s = source("mp4", video(VideoCodec::H264), vec![audio(1, AudioCodec::Aac, true)], vec![subtitle(2, SubtitleFormat::Srt, true)]);
        assert_eq!(plan(&s, s.default_audio(), s.subtitles.first()), Ok(Plan::Direct));
    }

    #[test]
    fn no_video_is_refused() {
        let mut s = source("mp4", video(VideoCodec::H264), vec![], vec![]);
        s.video.clear();
        assert_eq!(plan(&s, None, None), Err(NoVideo));
    }
}
```

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -p oneshot-cast transcode::plan`
Expected: FAIL to compile (`plan`, `Plan`… not defined).

- [ ] **Step 4: Write the implementation**

Prepend to `crates/cast/src/transcode/plan.rs`:

```rust
//! What to do with a title for an AirPlay receiver: nothing, or convert only what it cannot play.

use oneshot_core::stream::{AudioCodec, AudioStream, MediaSource, SubtitleFormat, SubtitleStream, VideoCodec};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoPlan {
    Copy,
    H264,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioPlan {
    Copy,
    Aac,
}

/// A subtitle drawn into the picture. `position` is its place in `MediaSource::subtitles`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Burn {
    pub position: usize,
    /// PGS, VobSub, DVB: drawn with an overlay; text goes through the `subtitles` filter.
    pub bitmap: bool,
}

/// How ffmpeg is to produce the HLS stream. Positions are places in the source's track lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Convert {
    pub video_pos: usize,
    pub audio_pos: Option<usize>,
    pub video: VideoPlan,
    /// The video is HEVC (copied, it needs the `hvc1` tag to play in fMP4).
    pub hevc: bool,
    pub audio: AudioPlan,
    pub burn: Option<Burn>,
    pub deinterlace: bool,
    /// The encoded picture is taller than 1080 lines.
    pub scale_down: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// The original file goes to the receiver untouched.
    Direct,
    Convert(Convert),
}

/// The title has no video track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoVideo;

/// What an AirPlay receiver plays from an MP4 as it is.
fn audio_direct(codec: &AudioCodec) -> bool {
    matches!(codec, AudioCodec::Aac | AudioCodec::Ac3 | AudioCodec::Eac3 | AudioCodec::Alac | AudioCodec::Mp3)
}

/// What can be put in an HLS fMP4 segment without conversion.
fn audio_hls(codec: &AudioCodec) -> bool {
    matches!(codec, AudioCodec::Aac | AudioCodec::Ac3 | AudioCodec::Eac3 | AudioCodec::Alac)
}

/// `audio` and `subtitle` are the tracks the person gets (same choice as when watching in Flick).
pub fn plan(source: &MediaSource, audio: Option<&AudioStream>, subtitle: Option<&SubtitleStream>) -> Result<Plan, NoVideo> {
    let video = source.primary_video().ok_or(NoVideo)?;
    let video_pos = source.video.iter().position(|v| v.index == video.index).unwrap_or(0);
    let audio_pos = audio.and_then(|a| source.audio.iter().position(|x| x.index == a.index));
    let burn = subtitle.filter(|s| !s.external).and_then(|s| {
        let position = source.subtitles.iter().position(|x| x.index == s.index)?;
        match s.format {
            SubtitleFormat::Srt | SubtitleFormat::Ass | SubtitleFormat::WebVtt => Some(Burn { position, bitmap: false }),
            SubtitleFormat::Pgs | SubtitleFormat::VobSub | SubtitleFormat::Dvb => Some(Burn { position, bitmap: true }),
            SubtitleFormat::Other => None,
        }
    });

    let hevc = video.codec == VideoCodec::Hevc;
    let video_ok = !video.interlaced && burn.is_none() && (hevc || (video.codec == VideoCodec::H264 && video.bit_depth.unwrap_or(8) <= 8));
    let container = source.container.as_deref().unwrap_or_default().to_ascii_lowercase();
    let mp4 = container.split(',').any(|c| matches!(c.trim(), "mp4" | "m4v" | "mov"));
    let is_default_audio = audio.is_none_or(|a| source.default_audio().is_some_and(|d| d.index == a.index));
    if mp4 && video_ok && is_default_audio && audio.is_none_or(|a| audio_direct(&a.codec)) {
        return Ok(Plan::Direct);
    }

    let encode = !video_ok;
    Ok(Plan::Convert(Convert {
        video_pos,
        audio_pos,
        video: if encode { VideoPlan::H264 } else { VideoPlan::Copy },
        hevc,
        audio: if audio.is_none_or(|a| audio_hls(&a.codec)) { AudioPlan::Copy } else { AudioPlan::Aac },
        burn,
        deinterlace: encode && video.interlaced,
        scale_down: encode && video.height > 1080,
    }))
}
```

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test -p oneshot-cast transcode::plan`
Expected: 9 passed. (If `VideoCodec` lacks `PartialEq`, it has it: the existing code compares with `matches!`; if the compare fails to compile, use `matches!(video.codec, VideoCodec::Hevc)`.)

- [ ] **Step 6: Commit**

```bash
git add crates/cast/src/transcode crates/cast/src/lib.rs
git commit -m "AirPlay: plan what to convert for a receiver (copy what it plays)"
```

---

### Task 2: The ffmpeg command line

**Files:**
- Create: `crates/cast/src/transcode/args.rs`
- Modify: `crates/cast/src/transcode/mod.rs`

**Interfaces:**
- Consumes: `Convert`, `VideoPlan`, `AudioPlan`, `Burn` (Task 1).
- Produces: `pub fn args(convert: &Convert, input: &str, start_ms: u64, dir: &Path) -> Vec<String>` and `pub const SEGMENT_SECS: u64 = 6;` and `pub const PLAYLIST: &str = "index.m3u8";`.

- [ ] **Step 1: Write the failing tests**

`crates/cast/src/transcode/args.rs` (tests only for now):

```rust
#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::transcode::{AudioPlan, Burn, Convert, VideoPlan};

    fn convert() -> Convert {
        Convert { video_pos: 0, audio_pos: Some(1), video: VideoPlan::Copy, hevc: false, audio: AudioPlan::Copy, burn: None, deinterlace: false, scale_down: false }
    }
    fn joined(c: &Convert, start_ms: u64) -> String {
        args(c, "http://127.0.0.1:9/c/t/f.mkv", start_ms, Path::new("/tmp/x/0")).join(" ")
    }

    #[test]
    fn a_remux_copies_both_tracks_and_writes_fmp4_hls() {
        let a = joined(&convert(), 0);
        assert!(a.contains("-map 0:v:0 -map 0:a:1"), "{a}");
        assert!(a.contains("-c:v copy") && a.contains("-c:a copy"), "{a}");
        assert!(a.contains("-f hls") && a.contains("-hls_segment_type fmp4") && a.contains("-hls_playlist_type event"), "{a}");
        assert!(a.ends_with("/tmp/x/0/index.m3u8"), "{a}");
        assert!(!a.contains("-ss"), "{a}");
    }

    #[test]
    fn a_start_position_seeks_the_input() {
        let a = joined(&convert(), 90_500);
        assert!(a.contains("-ss 90.500 -i http://127.0.0.1:9/c/t/f.mkv"), "{a}");
    }

    #[test]
    fn copied_hevc_is_tagged_and_converted_audio_is_stereo_aac() {
        let c = Convert { hevc: true, audio: AudioPlan::Aac, ..convert() };
        let a = joined(&c, 0);
        assert!(a.contains("-c:v copy -tag:v hvc1"), "{a}");
        assert!(a.contains("-c:a aac -b:a 192k -ac 2"), "{a}");
    }

    #[test]
    fn no_audio_means_no_audio_map() {
        let c = Convert { audio_pos: None, ..convert() };
        let a = joined(&c, 0);
        assert!(!a.contains("0:a:") && a.contains("-an"), "{a}");
    }

    #[test]
    fn an_encode_caps_the_picture_and_forces_a_keyframe_per_segment() {
        let c = Convert { video: VideoPlan::H264, scale_down: true, deinterlace: true, ..convert() };
        let a = joined(&c, 0);
        assert!(a.contains("-vf yadif,scale=-2:1080"), "{a}");
        assert!(a.contains("-force_key_frames expr:gte(t,n_forced*6)"), "{a}");
        assert!(a.contains("-pix_fmt yuv420p") && a.contains("-b:v 12M"), "{a}");
        assert!(a.contains(if cfg!(target_os = "macos") { "h264_videotoolbox" } else { "libx264" }), "{a}");
    }

    #[test]
    fn a_text_subtitle_goes_through_the_subtitles_filter_in_the_input_timeline() {
        let c = Convert { video: VideoPlan::H264, burn: Some(Burn { position: 2, bitmap: false }), ..convert() };
        let a = joined(&c, 90_000);
        assert!(a.contains("-vf setpts=PTS+90.000/TB,subtitles=filename='http\\://127.0.0.1\\:9/c/t/f.mkv':si=2,setpts=PTS-STARTPTS"), "{a}");
    }

    #[test]
    fn a_bitmap_subtitle_is_overlaid() {
        let c = Convert { video: VideoPlan::H264, burn: Some(Burn { position: 1, bitmap: true }), ..convert() };
        let a = joined(&c, 0);
        assert!(a.contains("-filter_complex [0:v:0][0:s:1]overlay[v] -map [v]"), "{a}");
        assert!(!a.contains("-map 0:v:0"), "{a}");
    }

    #[test]
    fn a_quote_in_the_url_is_escaped_in_a_filter() {
        assert_eq!(filter_value("http://h:1/a'b"), "'http\\://h\\:1/a'\\''b'");
    }
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p oneshot-cast transcode::args`
Expected: FAIL to compile.

- [ ] **Step 3: Write the implementation**

Prepend to `args.rs` (and in `mod.rs` add `mod args;` and `pub use args::{PLAYLIST, SEGMENT_SECS, args};`):

```rust
//! The ffmpeg command line for a [`Convert`]: HLS with fMP4 segments, written to a folder.

use std::path::Path;

use super::{AudioPlan, Convert, VideoPlan};

pub const SEGMENT_SECS: u64 = 6;
pub const PLAYLIST: &str = "index.m3u8";

/// A value in a filter description, quoted so `:` and `\` stay literal.
fn filter_value(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace(':', "\\:").replace('\'', "'\\''");
    format!("'{escaped}'")
}

fn push(out: &mut Vec<String>, items: &[&str]) {
    out.extend(items.iter().map(|s| (*s).to_owned()));
}

/// `input` is a URL on the loopback relay (no credential in it); `start_ms` where to begin in the title;
/// `dir` the folder the playlist and segments are written to.
pub fn args(c: &Convert, input: &str, start_ms: u64, dir: &Path) -> Vec<String> {
    let start = format!("{:.3}", start_ms as f64 / 1000.0);
    let mut a = Vec::new();
    push(&mut a, &["-hide_banner", "-loglevel", "error", "-nostdin"]);
    if start_ms > 0 {
        push(&mut a, &["-ss", &start]);
    }
    push(&mut a, &["-i", input]);

    // Which pictures and sounds go out.
    let encode = c.video == VideoPlan::H264;
    match &c.burn {
        Some(b) if b.bitmap => {
            push(&mut a, &["-filter_complex", &format!("[0:v:{}][0:s:{}]overlay[v]", c.video_pos, b.position), "-map", "[v]"]);
        }
        _ => push(&mut a, &["-map", &format!("0:v:{}", c.video_pos)]),
    }
    match c.audio_pos {
        Some(p) => push(&mut a, &["-map", &format!("0:a:{p}")]),
        None => push(&mut a, &["-an"]),
    }
    push(&mut a, &["-sn", "-dn"]);

    // The picture.
    if encode {
        let mut filters: Vec<String> = Vec::new();
        if let Some(b) = c.burn.as_ref().filter(|b| !b.bitmap) {
            // The subtitles filter reads the title itself from its start: shift the clock to the title's.
            filters.push(format!("setpts=PTS+{start}/TB"));
            filters.push(format!("subtitles=filename={}:si={}", filter_value(input), b.position));
            filters.push("setpts=PTS-STARTPTS".into());
        }
        if c.deinterlace {
            filters.push("yadif".into());
        }
        if c.scale_down {
            filters.push("scale=-2:1080".into());
        }
        if !filters.is_empty() {
            push(&mut a, &["-vf", &filters.join(",")]);
        }
        let encoder = if cfg!(target_os = "macos") { "h264_videotoolbox" } else { "libx264" };
        push(&mut a, &["-c:v", encoder, "-b:v", "12M", "-pix_fmt", "yuv420p"]);
        if !cfg!(target_os = "macos") {
            push(&mut a, &["-preset", "veryfast"]);
        }
        push(&mut a, &["-force_key_frames", &format!("expr:gte(t,n_forced*{SEGMENT_SECS})")]);
    } else {
        push(&mut a, &["-c:v", "copy"]);
        if c.hevc {
            push(&mut a, &["-tag:v", "hvc1"]);
        }
    }

    // The sound.
    match (c.audio_pos, c.audio) {
        (None, _) => {}
        (Some(_), AudioPlan::Copy) => push(&mut a, &["-c:a", "copy"]),
        (Some(_), AudioPlan::Aac) => push(&mut a, &["-c:a", "aac", "-b:a", "192k", "-ac", "2"]),
    }

    // The stream.
    push(&mut a, &["-f", "hls", "-hls_time", &SEGMENT_SECS.to_string(), "-hls_playlist_type", "event", "-hls_segment_type", "fmp4"]);
    push(&mut a, &["-hls_flags", "independent_segments+temp_file", "-hls_fmp4_init_filename", "init.mp4"]);
    a.push("-hls_segment_filename".into());
    a.push(dir.join("seg%05d.m4s").to_string_lossy().into_owned());
    a.push(dir.join(PLAYLIST).to_string_lossy().into_owned());
    a
}
```

Note: the `subtitles` filter test expects `filename='http\://127.0.0.1\:9/c/t/f.mkv'`; the `filter_value` above yields that.

- [ ] **Step 4: Run to see them pass**

Run: `cargo test -p oneshot-cast transcode::args`
Expected: 8 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/cast/src/transcode
git commit -m "AirPlay: the ffmpeg command line for an HLS fMP4 conversion"
```

---

### Task 3: Finding ffmpeg and running a job

**Files:**
- Create: `crates/cast/src/transcode/locate.rs`, `crates/cast/src/transcode/job.rs`
- Modify: `crates/cast/src/transcode/mod.rs`, `crates/cast/Cargo.toml`, `crates/core/src/codes.rs`, `ERROR_IDENTIFIER.md` (regenerated)

**Interfaces:**
- Consumes: `args()`, `PLAYLIST` (Task 2), `Convert` (Task 1).
- Produces:
  - `pub fn locate(dirs: &[PathBuf]) -> Option<PathBuf>`
  - `pub fn missing_error() -> Error` (code `CAST_FFMPEG_MISSING`)
  - `pub struct Job` with `pub async fn start(ffmpeg: &Path, convert: &Convert, input: &str, start_ms: u64, dir: &Path) -> Result<Job>`, `pub fn produced_ms(&self) -> u64`, `pub fn finished(&mut self) -> bool`, and a kill on drop.
  - codes `CAST_FFMPEG_MISSING` (`FLK-CAST-015`), `CAST_CONVERT_FAILED` (`FLK-CAST-016`), `CAST_CONVERT_SLOW` (`FLK-CAST-017`).

- [ ] **Step 1: Add the codes**

In `crates/core/src/codes.rs`, before `CAST_OTHER`:

```rust
    CAST_FFMPEG_MISSING = "FLK-CAST-015", "CAST", "ffmpeg, which converts videos for AirPlay, was not found.", "Reinstall Flick; when building it yourself, run `node tools/ensure-libmpv.mjs`.";
    CAST_CONVERT_FAILED = "FLK-CAST-016", "CAST", "The conversion of the video for AirPlay stopped unexpectedly.", "Retry; if it persists, try another version of the title (the log has ffmpeg's reason).";
    CAST_CONVERT_SLOW = "FLK-CAST-017", "CAST", "The conversion of the video for AirPlay did not start in time.", "Try again, or watch it in Flick. A very large or damaged file can take too long to open.";
```

Cargo: in `crates/cast/Cargo.toml` change the tokio line to `tokio = { workspace = true, features = ["net", "io-util", "time", "sync", "rt", "process", "fs"] }` and add `tempfile = "3"` under `[dependencies]`.

- [ ] **Step 2: Write the failing tests**

`crates/cast/src/transcode/locate.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;

    use super::*;

    fn fake(dir: &std::path::Path) -> std::path::PathBuf {
        let p = dir.join(NAME);
        fs::write(&p, b"").unwrap();
        p
    }

    #[test]
    fn the_env_var_wins_then_the_folders_then_the_path() {
        let (a, b, c) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (in_a, in_b, in_c) = (fake(a.path()), fake(b.path()), fake(c.path()));
        let dirs = vec![a.path().to_path_buf()];
        let path = Some(std::env::join_paths([c.path()]).unwrap());
        assert_eq!(locate_in(Some(in_b.clone().into_os_string()), &dirs, path.clone()), Some(in_b));
        assert_eq!(locate_in(None, &dirs, path.clone()), Some(in_a));
        assert_eq!(locate_in(None, &[], path), Some(in_c));
    }

    #[test]
    fn an_env_var_to_a_missing_file_is_ignored() {
        assert_eq!(locate_in(Some(OsString::from("/nope/ffmpeg")), &[], None), None);
    }
}
```

`crates/cast/src/transcode/job.rs` tests (the integration one skips without ffmpeg):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcode::{AudioPlan, Convert, VideoPlan};

    fn convert() -> Convert {
        Convert { video_pos: 0, audio_pos: None, video: VideoPlan::H264, hevc: false, audio: AudioPlan::Copy, burn: None, deinterlace: false, scale_down: false }
    }

    #[test]
    fn the_playlist_length_is_the_sum_of_its_segments() {
        let list = "#EXTM3U\n#EXT-X-VERSION:7\n#EXTINF:6.000000,\nseg00000.m4s\n#EXTINF:5.500,\nseg00001.m4s\n";
        assert_eq!(playlist_ms(list), 11_500);
        assert_eq!(playlist_ms("#EXTM3U\n"), 0);
    }

    #[tokio::test]
    async fn a_bad_input_fails_with_the_conversion_code_and_no_hang() {
        let Some(ffmpeg) = crate::transcode::locate(&[]) else { return };
        let dir = tempfile::tempdir().unwrap();
        let err = match Job::start(&ffmpeg, &convert(), "http://127.0.0.1:1/none.mkv", 0, dir.path()).await {
            Ok(_) => panic!("started on a missing input"),
            Err(e) => e,
        };
        assert_eq!(err.code(), oneshot_core::codes::CAST_CONVERT_FAILED.id);
    }

    #[tokio::test]
    async fn a_real_clip_gets_a_playlist_and_the_process_dies_with_the_job() {
        let Some(ffmpeg) = crate::transcode::locate(&[]) else { return };
        let work = tempfile::tempdir().unwrap();
        let clip = work.path().join("clip.mp4");
        let made = std::process::Command::new(&ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=24:duration=14", "-f", "lavfi", "-i", "sine=frequency=440:duration=14", "-c:v", "libx264", "-c:a", "aac", "-shortest"])
            .arg(&clip)
            .status();
        if !made.is_ok_and(|s| s.success()) {
            return; // an ffmpeg without libx264
        }
        let out = work.path().join("out");
        let c = Convert { video: VideoPlan::Copy, audio_pos: Some(0), ..convert() };
        let mut job = Job::start(&ffmpeg, &c, clip.to_str().unwrap(), 0, &out).await.unwrap();
        assert!(out.join("index.m3u8").exists() && out.join("init.mp4").exists());
        assert!(job.produced_ms() >= 5_000);
        drop(job);
    }
}
```

- [ ] **Step 3: Run to see them fail**

Run: `cargo test -p oneshot-cast transcode`
Expected: FAIL to compile (`locate_in`, `Job`… not defined).

- [ ] **Step 4: Write `locate.rs`**

```rust
//! Where the ffmpeg executable is.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub(crate) const NAME: &str = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };

/// The `ONESHOT_FFMPEG` file, else `ffmpeg` in one of `dirs` (the folders libmpv is searched in), else on the `PATH`.
pub fn locate(dirs: &[PathBuf]) -> Option<PathBuf> {
    locate_in(std::env::var_os("ONESHOT_FFMPEG"), dirs, std::env::var_os("PATH"))
}

pub(crate) fn locate_in(env: Option<OsString>, dirs: &[PathBuf], path: Option<OsString>) -> Option<PathBuf> {
    let env = env.map(PathBuf::from).filter(|p| p.is_file());
    let in_dirs = || dirs.iter().map(|d| d.join(NAME)).find(|p| p.is_file());
    let on_path = || path.as_deref().and_then(|p| std::env::split_paths(p).map(|d| d.join(NAME)).find(|p: &PathBuf| Path::is_file(p)));
    env.or_else(in_dirs).or_else(on_path)
}
```

- [ ] **Step 5: Write `job.rs`**

```rust
//! One running ffmpeg: it writes the playlist and segments into a folder until the title ends or it is dropped.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use oneshot_core::codes;
use oneshot_core::{Error, Result};
use parking_lot::Mutex;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use super::{Convert, PLAYLIST, args};

/// How long ffmpeg has to open the title and finish its first segment.
const FIRST_SEGMENT: Duration = Duration::from_secs(45);

pub struct Job {
    child: Child,
    dir: PathBuf,
}

/// The length of the segments a playlist lists.
pub(crate) fn playlist_ms(playlist: &str) -> u64 {
    let secs: f64 = playlist.lines().filter_map(|l| l.strip_prefix("#EXTINF:")).filter_map(|v| v.trim_end_matches(',').split(',').next()?.parse::<f64>().ok()).sum();
    (secs * 1000.0) as u64
}

impl Job {
    /// Starts ffmpeg on `input` from `start_ms`, and returns once the first segment is on disk.
    pub async fn start(ffmpeg: &Path, convert: &Convert, input: &str, start_ms: u64, dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir).map_err(|e| Error::Storage(codes::CAST_CONVERT_FAILED.tag(format!("Flick could not prepare a folder for the conversion ({e})."))))?;
        let mut cmd = Command::new(ffmpeg);
        cmd.args(args(convert, input, start_ms, dir)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashing on screen
        let mut child = cmd.spawn().map_err(|e| Error::Playback(codes::CAST_FFMPEG_MISSING.tag(format!("ffmpeg could not be started ({e}). Reinstall Flick."))))?;

        // ffmpeg's own words go to the log; the last lines explain a failure.
        let tail = Arc::new(Mutex::new(String::new()));
        if let Some(stderr) = child.stderr.take() {
            let tail = Arc::clone(&tail);
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(target: "cast", "ffmpeg: {line}");
                    let mut t = tail.lock();
                    if t.len() > 600 {
                        t.clear();
                    }
                    t.push_str(&line);
                    t.push('\n');
                }
            });
        }

        let mut job = Self { child, dir: dir.to_path_buf() };
        let started = tokio::time::Instant::now();
        loop {
            if job.produced_ms() > 0 {
                return Ok(job);
            }
            if job.child.try_wait().ok().flatten().is_some() {
                // Let the stderr reader take the last lines.
                tokio::time::sleep(Duration::from_millis(100)).await;
                tracing::warn!(target: "cast", "ffmpeg stopped before its first segment: {}", tail.lock());
                return Err(Error::Playback(codes::CAST_CONVERT_FAILED.tag("The conversion of the video for AirPlay stopped before it began. Try another version of the title.")));
            }
            if started.elapsed() > FIRST_SEGMENT {
                return Err(Error::Playback(codes::CAST_CONVERT_SLOW.tag("The conversion of the video for AirPlay did not start in time. Try again, or watch it in Flick.")));
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }

    /// How much of the title is ready to be played (the playlist's length).
    pub fn produced_ms(&self) -> u64 {
        std::fs::read_to_string(self.dir.join(PLAYLIST)).map(|p| playlist_ms(&p)).unwrap_or(0)
    }

    /// ffmpeg has written the whole title.
    pub fn finished(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_some()
    }
}
```

`kill_on_drop(true)` makes dropping the `Job` kill ffmpeg. In `mod.rs` add:

```rust
mod job;
mod locate;

pub use job::Job;
pub use locate::locate;

/// ffmpeg is not installed with Flick: raised before the local player is stopped.
pub fn missing_error() -> oneshot_core::Error {
    oneshot_core::Error::Playback(oneshot_core::codes::CAST_FFMPEG_MISSING.tag("ffmpeg, which converts this video for AirPlay, was not found. Reinstall Flick."))
}
```

- [ ] **Step 6: Run to see them pass, regenerate the doc**

Run: `UPDATE_ERROR_DOC=1 cargo test -p oneshot-core codes && cargo test -p oneshot-cast transcode`
Expected: all pass (the clip test runs only when an ffmpeg with libx264 is on `PATH`; this Mac has Homebrew's). `every_documented_code_is_raised_somewhere` passes because the three codes are now used in `job.rs` / `mod.rs`.

- [ ] **Step 7: Commit**

```bash
git add crates/cast crates/core/src/codes.rs ERROR_IDENTIFIER.md
git commit -m "AirPlay: find ffmpeg and run a conversion job (FLK-CAST-015..017)"
```

---

### Task 4: The relay serves a folder

**Files:**
- Modify: `crates/cast/src/proxy.rs`

**Interfaces:**
- Consumes: existing `Proxy`, `Route`, `handle`.
- Produces: `pub async fn serve_dir(&self, root: PathBuf, local: IpAddr) -> Result<Url>` — the base URL `http://ip:port/c/<token>/`; a file at `root/a/b.m3u8` is `…/c/<token>/a/b.m3u8`. `fn byte_range(header: &str, len: u64) -> Option<(u64, u64)>`.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` of `proxy.rs`:

```rust
    #[test]
    fn byte_ranges() {
        assert_eq!(byte_range("bytes=0-3", 10), Some((0, 3)));
        assert_eq!(byte_range("bytes=4-", 10), Some((4, 9)));
        assert_eq!(byte_range("bytes=-3", 10), Some((7, 9)));
        assert_eq!(byte_range("bytes=2-99", 10), Some((2, 9)));
        assert_eq!(byte_range("bytes=10-", 10), None);
        assert_eq!(byte_range("items=0-1", 10), None);
    }

    #[tokio::test]
    async fn serves_a_folder_under_the_secret_and_refuses_to_leave_it() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("0")).unwrap();
        std::fs::write(root.path().join("0/index.m3u8"), "#EXTM3U\n").unwrap();
        std::fs::write(root.path().join("0/seg00000.m4s"), b"0123456789").unwrap();
        std::fs::write(root.path().parent().unwrap().join("outside.txt"), "secret").ok();

        let proxy = Proxy::default();
        let base = proxy.serve_dir(root.path().to_path_buf(), "127.0.0.1".parse().unwrap()).await.unwrap();
        let addr = format!("{}:{}", base.host_str().unwrap(), base.port().unwrap());
        let at = |rest: &str| format!("{}{rest}", base.path());

        let list = get(&addr, &at("0/index.m3u8"), "").await;
        assert!(list.starts_with("HTTP/1.1 200") && list.contains("application/vnd.apple.mpegurl") && list.ends_with("#EXTM3U\n"), "{list}");
        let seg = get(&addr, &at("0/seg00000.m4s"), "Range: bytes=2-4\r\n").await;
        assert!(seg.starts_with("HTTP/1.1 206") && seg.contains("video/mp4") && seg.contains("bytes 2-4/10") && seg.ends_with("234"), "{seg}");

        for bad in [at("../outside.txt"), at("0/../../outside.txt"), at("0/..%2f..%2foutside.txt"), at("0\\..\\..\\outside.txt"), at("/etc/passwd"), "/0/index.m3u8".to_owned(), at("0/missing.m4s")] {
            let r = get(&addr, &bad, "").await;
            assert!(r.starts_with("HTTP/1.1 404"), "{bad}: {r}");
        }
        proxy.stop();
    }
```

(`tempfile` is a regular dependency after Task 3, so tests can use it.)

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p oneshot-cast proxy`
Expected: FAIL to compile (`serve_dir`, `byte_range`).

- [ ] **Step 3: Refactor `Route` and add the folder mode**

In `proxy.rs`:

1. Replace the `Route` struct and `serve` body so a route is either an upstream or a folder:

```rust
enum Kind {
    Upstream { dir: Url, query: Vec<(String, String)>, headers: Vec<(String, String)>, http: Client },
    Files(std::path::PathBuf),
}

struct Route {
    token: String,
    kind: Kind,
}
```

2. In `serve`, build `Route { token, kind: Kind::Upstream { dir, query, headers, http } }`, and move the listener loop into a helper used by both modes:

```rust
    fn run(&self, listener: TcpListener, route: Route) {
        let route = std::sync::Arc::new(route);
        let task = tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else { break };
                let route = std::sync::Arc::clone(&route);
                tokio::spawn(async move {
                    if let Err(e) = handle(socket, &route).await {
                        tracing::debug!(target: "cast", "relay request ended: {e}");
                    }
                });
            }
        });
        *self.running.lock() = Some(task);
    }

    /// Serves the files of `root` under a secret path; the URL to give out ends with `/c/<token>/`.
    pub async fn serve_dir(&self, root: std::path::PathBuf, local: IpAddr) -> Result<Url> {
        self.stop();
        let listener = TcpListener::bind(SocketAddr::new(local, 0)).await.map_err(|e| Error::Network(oneshot_core::codes::CAST_RELAY.tag(format!("Flick could not open the local relay the device pulls the video from ({e}). Check no firewall blocks Flick on the local network."))))?;
        let addr = listener.local_addr().map_err(|e| Error::Network(oneshot_core::codes::CAST_RELAY.tag(format!("Flick could not open the local relay ({e})."))))?;
        let token = uuid::Uuid::new_v4().simple().to_string();
        let base = Url::parse(&format!("http://{addr}/c/{token}/")).map_err(|e| Error::Invalid(oneshot_core::codes::CAST_RELAY.tag(format!("The relay address could not be built ({e})."))))?;
        self.run(listener, Route { token, kind: Kind::Files(root) });
        Ok(base)
    }
```

3. `upstream_for` takes the upstream fields: change its signature to `fn upstream_for(token: &str, dir: &Url, query: &[(String, String)], head: &Head) -> Option<Url>` (body unchanged, using `token`, `dir`, `query`).

4. Add the folder lookup and range parser:

```rust
/// The file a relay path stands for, if it carries the secret and stays inside `root`.
fn file_for(token: &str, root: &std::path::Path, head: &Head) -> Option<std::path::PathBuf> {
    let rest = head.path.strip_prefix("/c/")?.strip_prefix(token)?.strip_prefix('/')?;
    let mut path = root.to_path_buf();
    for part in rest.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains(['\\', ':', '%', '\0']) {
            return None;
        }
        path.push(part);
    }
    Some(path)
}

fn content_type(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("m3u8") => "application/vnd.apple.mpegurl",
        Some("m4s" | "mp4") => "video/mp4",
        _ => "application/octet-stream",
    }
}

/// `bytes=a-b`, `bytes=a-` and `bytes=-n` over a body of `len` bytes: the first and last byte.
fn byte_range(header: &str, len: u64) -> Option<(u64, u64)> {
    let spec = header.strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    let (first, last) = match (a.parse::<u64>().ok(), b.parse::<u64>().ok()) {
        (Some(a), Some(b)) => (a, b.min(len.saturating_sub(1))),
        (Some(a), None) => (a, len.saturating_sub(1)),
        (None, Some(n)) if n > 0 => (len.saturating_sub(n), len.saturating_sub(1)),
        _ => return None,
    };
    (first <= last && first < len).then_some((first, last))
}

async fn serve_file(socket: &mut TcpStream, route_token: &str, root: &std::path::Path, head: &Head) -> std::io::Result<()> {
    let Some(path) = file_for(route_token, root, head) else { return respond(socket, "404 Not Found", "").await };
    let Ok(body) = tokio::fs::read(&path).await else { return respond(socket, "404 Not Found", "").await };
    let len = body.len() as u64;
    let (status, slice, range) = match head.range.as_deref().and_then(|r| byte_range(r, len)) {
        Some((a, b)) => ("206 Partial Content", &body[a as usize..=b as usize], format!("content-range: bytes {a}-{b}/{len}\r\n")),
        None => ("200 OK", &body[..], String::new()),
    };
    let out = format!("HTTP/1.1 {status}\r\n{CORS}content-type: {}\r\ncontent-length: {}\r\naccept-ranges: bytes\r\n{range}Cache-Control: no-cache\r\nConnection: close\r\n\r\n", content_type(&path), slice.len());
    socket.write_all(out.as_bytes()).await?;
    if head.method == "GET" {
        socket.write_all(slice).await?;
    }
    socket.shutdown().await
}
```

5. In `handle`, after the method checks, branch on the kind:

```rust
    let upstream = match &route.kind {
        Kind::Files(root) => return serve_file(&mut socket, &route.token, root, &head).await,
        Kind::Upstream { dir, query, headers, http } => (dir, query, headers, http),
    };
    let (dir, query, headers, http) = upstream;
    let Some(url) = upstream_for(&route.token, dir, query, &head) else { return respond(&mut socket, "404 Not Found", "").await };
    let mut req = if head.method == "HEAD" { http.head(url) } else { http.get(url) };
    for (k, v) in headers {
        req = req.header(k, v);
    }
```

(keep the rest of `handle` as it is; `route.headers`/`route.http` references are replaced by the locals above). Also add `tokio` feature `fs` is already added in Task 3.

- [ ] **Step 4: Run to see them pass**

Run: `cargo test -p oneshot-cast proxy`
Expected: the existing relay test and the two new ones pass.

- [ ] **Step 5: Commit**

```bash
git add crates/cast/src/proxy.rs
git commit -m "Cast relay: serve a folder (with Range) under the secret path"
```

---

### Task 5: The session and the converting receiver

**Files:**
- Create: `crates/cast/src/transcode/session.rs`, `crates/cast/src/transcode/receiver.rs`
- Modify: `crates/cast/src/airplay.rs`, `crates/cast/src/transcode/mod.rs`

**Interfaces:**
- Consumes: `Job` (Task 3), `Proxy::serve` / `serve_dir` (Task 4), `Convert` (Task 1), `AirPlay` and `Receiver`/`Remote`.
- Produces:
  - `AirPlay::load(&self, url: &Url, start_ms: u64, duration_ms: Option<u64>) -> Result<()>` (a new `/play` on the open link)
  - `pub(crate) struct Session` with `start(ffmpeg, convert, upstream, headers, local, http, start_ms) -> Result<Session>`, `playlist_url() -> Url`, `base_ms() -> u64`, `produced_ms() -> u64`, `finished() -> bool`, `restart_at(ms) -> Result<()>`
  - `pub(crate) enum SeekPlan { Within(u64), Restart }`, `pub(crate) fn seek_plan(base: u64, produced: u64, ms: u64) -> SeekPlan`
  - `pub(crate) struct Converting` implementing `Receiver`, `Converting::new(airplay: AirPlay, session: Session, total_ms: Option<u64>)`.

- [ ] **Step 1: Write the failing test for the seek rule**

`crates/cast/src/transcode/receiver.rs` (tests first):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seek_inside_the_produced_part_is_a_plain_scrub_relative_to_the_start() {
        // The job began at 60 s and has 100 s ready: 60..160 s of the title.
        assert_eq!(seek_plan(60_000, 100_000, 90_000), SeekPlan::Within(30_000));
        assert_eq!(seek_plan(60_000, 100_000, 60_000), SeekPlan::Within(0));
    }

    #[test]
    fn a_seek_before_the_start_or_near_or_past_the_edge_restarts_the_job() {
        assert_eq!(seek_plan(60_000, 100_000, 10_000), SeekPlan::Restart);
        assert_eq!(seek_plan(60_000, 100_000, 155_000), SeekPlan::Restart); // within 12 s of the edge
        assert_eq!(seek_plan(60_000, 100_000, 900_000), SeekPlan::Restart);
        assert_eq!(seek_plan(0, 5_000, 1_000), SeekPlan::Restart); // too little produced to trust
    }

    #[test]
    fn the_receivers_position_is_shifted_by_the_start_and_capped_by_the_title() {
        assert_eq!(title_position(60_000, 30_000, Some(3_600_000)), 90_000);
        assert_eq!(title_position(3_599_000, 5_000, Some(3_600_000)), 3_600_000);
        assert_eq!(title_position(0, 5_000, None), 5_000);
    }
}
```

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p oneshot-cast transcode::receiver`
Expected: FAIL to compile.

- [ ] **Step 3: Factor `/play` out of `AirPlay::start` and add `load`**

In `airplay.rs`, replace the `/play` block of `start` by a shared function and add `load`:

```rust
/// `POST /play` on an open link; the receiver then fetches `url`.
async fn send_play(link: &mut Link, session: &str, url: &url::Url, fraction: f64, encrypted: bool) -> Result<()> {
    let resp = link
        .request("POST", "/play", &[("Content-Type", "application/x-apple-binary-plist"), ("X-Apple-Session-ID", session), ("User-Agent", USER_AGENT)], &play_body(url.as_str(), fraction)?)
        .await
        .map_err(link_err)?;
    tracing::debug!(target: "cast", status = resp.status, encrypted, "AirPlay /play answered");
    match resp.status {
        s if (200..300).contains(&s) => Ok(()),
        // 470: "connection authorization required".
        401 | 403 | 470 => Err(pin_wanted()),
        s => Err(Error::Playback(codes::CAST_AIRPLAY.tag(format!("The AirPlay device refused the stream (HTTP {s}).")))),
    }
}
```

In `start`, after computing `fraction`, call `send_play(&mut link, &session, url, fraction, creds.is_some()).await?;` in place of the inline request and `match`. Then add to `impl AirPlay`:

```rust
    /// Plays another stream on the same connection (the conversion restarted somewhere else).
    pub async fn load(&self, url: &url::Url, start_ms: u64, duration_ms: Option<u64>) -> Result<()> {
        let fraction = match duration_ms {
            Some(d) if d > 0 => start_ms as f64 / d as f64,
            _ => 0.0,
        };
        send_play(&mut *self.link.lock().await, &self.session, url, fraction, true).await?;
        *self.seen_playing.lock() = false;
        *self.last.lock() = Remote { state: CastState::Loading, position_ms: start_ms, duration_ms, ..Default::default() };
        Ok(())
    }
```

- [ ] **Step 4: Write `session.rs`**

```rust
//! A conversion in progress: the job, the relays around it, and the folder it writes to.

use std::net::IpAddr;
use std::path::PathBuf;

use oneshot_core::Result;
use url::Url;

use super::{Convert, Job};
use crate::proxy::Proxy;

pub(crate) struct Session {
    ffmpeg: PathBuf,
    convert: Convert,
    /// ffmpeg reads the title here (loopback, the server's credentials stay in the relay).
    input: Url,
    _source: Proxy,
    /// The receiver pulls the playlist and segments here.
    files: Proxy,
    base: Url,
    dir: tempfile::TempDir,
    job: Option<Job>,
    generation: u32,
    base_ms: u64,
}

impl Session {
    pub async fn start(ffmpeg: PathBuf, convert: Convert, upstream: &Url, headers: Vec<(String, String)>, local: IpAddr, http: reqwest::Client, start_ms: u64) -> Result<Self> {
        let source = Proxy::default();
        let input = source.serve(upstream, headers, IpAddr::from([127, 0, 0, 1]), http).await?;
        let dir = tempfile::Builder::new().prefix("flick-airplay-").tempdir().map_err(|e| oneshot_core::Error::Storage(oneshot_core::codes::CAST_CONVERT_FAILED.tag(format!("Flick could not prepare a folder for the conversion ({e})."))))?;
        let files = Proxy::default();
        let base = files.serve_dir(dir.path().to_path_buf(), local).await?;
        let mut session = Self { ffmpeg, convert, input, _source: source, files, base, dir, job: None, generation: 0, base_ms: 0 };
        session.begin(start_ms).await?;
        Ok(session)
    }

    async fn begin(&mut self, ms: u64) -> Result<()> {
        self.job = None; // the previous ffmpeg is killed
        self.generation += 1;
        let out = self.dir.path().join(self.generation.to_string());
        self.base_ms = ms;
        self.job = Some(Job::start(&self.ffmpeg, &self.convert, self.input.as_str(), ms, &out).await?);
        Ok(())
    }

    /// Converts again from `ms`: a new folder, a new playlist.
    pub async fn restart_at(&mut self, ms: u64) -> Result<()> {
        let old = self.dir.path().join(self.generation.to_string());
        let result = self.begin(ms).await;
        let _ = std::fs::remove_dir_all(old);
        result
    }

    pub fn playlist_url(&self) -> Url {
        self.base.join(&format!("{}/{}", self.generation, super::PLAYLIST)).unwrap_or_else(|_| self.base.clone())
    }

    /// Where in the title the current playlist begins.
    pub fn base_ms(&self) -> u64 {
        self.base_ms
    }

    pub fn produced_ms(&self) -> u64 {
        self.job.as_ref().map_or(0, Job::produced_ms)
    }

    pub fn finished(&mut self) -> bool {
        self.job.as_mut().is_none_or(Job::finished)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.job = None;
        self.files.stop();
    }
}
```

(`TempDir` removes the folder on drop; `Job`'s `kill_on_drop` ends ffmpeg.)

- [ ] **Step 5: Write `receiver.rs`**

Above the tests added in Step 1:

```rust
//! The AirPlay receiver as seen by the rest of Flick while a conversion feeds it.

use async_trait::async_trait;
use oneshot_core::Result;

use super::Session;
use crate::airplay::AirPlay;
use crate::{CastState, Receiver, Remote};

/// Seeks this close to the end of what is produced restart the conversion instead: the receiver
/// would stall at the edge.
const EDGE_MS: u64 = 12_000;
/// Less than this is produced: do not trust it for a scrub.
const MIN_MS: u64 = 12_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SeekPlan {
    /// A scrub on the receiver, at this position of its own (shifted) timeline.
    Within(u64),
    /// Convert again from the asked position and reload the stream.
    Restart,
}

/// The playlist covers `base..base + produced` of the title.
pub(crate) fn seek_plan(base: u64, produced: u64, ms: u64) -> SeekPlan {
    if produced >= MIN_MS && ms >= base && ms + EDGE_MS <= base + produced { SeekPlan::Within(ms - base) } else { SeekPlan::Restart }
}

/// A position of the receiver's timeline as a position in the title.
pub(crate) fn title_position(base: u64, receiver: u64, total: Option<u64>) -> u64 {
    let ms = base + receiver;
    total.map_or(ms, |t| ms.min(t))
}

pub(crate) struct Converting {
    airplay: AirPlay,
    session: tokio::sync::Mutex<Session>,
    total_ms: Option<u64>,
}

impl Converting {
    pub fn new(airplay: AirPlay, session: Session, total_ms: Option<u64>) -> Self {
        Self { airplay, session: tokio::sync::Mutex::new(session), total_ms }
    }
}

#[async_trait]
impl Receiver for Converting {
    async fn pause(&self) -> Result<()> {
        self.airplay.pause().await
    }

    async fn resume(&self) -> Result<()> {
        self.airplay.resume().await
    }

    async fn seek(&self, ms: u64) -> Result<()> {
        let mut session = self.session.lock().await;
        match seek_plan(session.base_ms(), session.produced_ms(), ms) {
            SeekPlan::Within(at) => self.airplay.seek(at).await,
            SeekPlan::Restart => {
                session.restart_at(ms).await?;
                self.airplay.load(&session.playlist_url(), 0, None).await
            }
        }
    }

    async fn set_volume(&self, volume: f32) -> Result<()> {
        self.airplay.set_volume(volume).await
    }

    async fn status(&self) -> Remote {
        let mut r = self.airplay.status().await;
        let mut session = self.session.lock().await;
        r.position_ms = title_position(session.base_ms(), r.position_ms, self.total_ms);
        r.duration_ms = self.total_ms.or(r.duration_ms.map(|d| d + session.base_ms()));
        // The receiver reaching the edge of a playlist still being written is not the end of the title.
        if r.state == CastState::Ended && !session.finished() {
            r.state = CastState::Buffering;
        }
        r
    }

    async fn stop(&self) {
        self.airplay.stop().await;
    }
}
```

In `transcode/mod.rs` add `mod receiver; mod session;` and `pub(crate) use receiver::Converting; pub(crate) use session::Session;`. In `airplay.rs` make `AirPlay` visible (`pub(crate) struct AirPlay` already) and in `lib.rs` change `mod airplay;` to keep private but `transcode` uses `crate::airplay::AirPlay` — fine inside the crate.

- [ ] **Step 6: Run to see the tests pass**

Run: `cargo test -p oneshot-cast`
Expected: the seek/position tests, the earlier tests and the airplay tests all pass.

- [ ] **Step 7: Commit**

```bash
git add crates/cast
git commit -m "AirPlay: a conversion session and a receiver that restarts it on a far seek"
```

---

### Task 6: Wiring in the caster and the app

**Files:**
- Modify: `crates/cast/src/lib.rs`, `app/src/commands/cast.rs`, `app/src/main.rs`, `crates/core/src/codes.rs` (CAST_FORMAT wording), `ERROR_IDENTIFIER.md` (regenerated)

**Interfaces:**
- Consumes: `transcode::{plan, Plan, Convert, locate, missing_error, Session, Converting}`.
- Produces: `CastMedia.convert: Option<transcode::Convert>`; `Caster::with_ffmpeg_dirs(self, dirs: Vec<PathBuf>) -> Self`; `Caster::can_convert(&self) -> bool`.

- [ ] **Step 1: `CastMedia` and `Caster`**

In `lib.rs`:

```rust
pub struct CastMedia {
    // …existing fields…
    /// Set when the original file is not playable as it is: the app converts it for the receiver.
    pub convert: Option<transcode::Convert>,
}
```

Add a field `ffmpeg_dirs: Vec<std::path::PathBuf>` to `Caster` (initialised `Vec::new()` in `new`), and:

```rust
    /// Where ffmpeg is looked for (the folders libmpv is searched in), after `ONESHOT_FFMPEG`.
    pub fn with_ffmpeg_dirs(mut self, dirs: Vec<std::path::PathBuf>) -> Self {
        self.ffmpeg_dirs = dirs;
        self
    }

    /// Whether ffmpeg is there to convert a title.
    pub fn can_convert(&self) -> bool {
        transcode::locate(&self.ffmpeg_dirs).is_some()
    }
```

- [ ] **Step 2: `Caster::start`**

Replace the relay line and the AirPlay arm:

```rust
        let relayed = if media.convert.is_none() { Some(self.proxy.serve(&media.url, media.headers.clone(), local, http.clone()).await?) } else { None };
        // … status = Loading …
        let loaded = match device.kind {
            CastKind::Chromecast => {
                let relayed = relayed.as_ref().ok_or_else(|| Error::Invalid(oneshot_core::codes::CAST_OTHER.tag("A Chromecast does not take a converted stream.")))?;
                chromecast::Chromecast::start(addr, relayed, &media).await.map(|r| Box::new(r) as Box<dyn Receiver>)
            }
            CastKind::AirPlay => {
                let creds = self.store.as_ref().and_then(|s| s.load(&device.id)).and_then(|c| hap::Credentials::decode(&c));
                let started = match (&media.convert, &relayed) {
                    (Some(convert), _) => self.start_converted(addr, local, convert.clone(), &media, http, creds.as_ref()).await,
                    (None, Some(relayed)) => airplay::AirPlay::start(addr, relayed, &media, creds.as_ref()).await.map(|r| Box::new(r) as Box<dyn Receiver>),
                    (None, None) => unreachable!("a direct cast always has its relay"),
                };
                // (the existing "pairing no longer honoured: forget it" block stays, on `started`)
                started
            }
        };
```

Add:

```rust
    /// Converts the title with ffmpeg and plays the result on the receiver.
    async fn start_converted(&self, addr: SocketAddr, local: IpAddr, convert: transcode::Convert, media: &CastMedia, http: reqwest::Client, creds: Option<&hap::Credentials>) -> Result<Box<dyn Receiver>> {
        let ffmpeg = transcode::locate(&self.ffmpeg_dirs).ok_or_else(transcode::missing_error)?;
        let session = transcode::Session::start(ffmpeg, convert, &media.url, media.headers.clone(), local, http, media.start_ms).await?;
        // The receiver's own timeline begins where the conversion does.
        let hls = CastMedia { url: session.playlist_url(), content_type: "application/x-mpegURL".into(), start_ms: 0, duration_ms: None, convert: None, headers: Vec::new(), title: media.title.clone() };
        let airplay = airplay::AirPlay::start(addr, &hls.url, &hls, creds).await?;
        Ok(Box::new(transcode::Converting::new(airplay, session, media.duration_ms)))
    }
```

(If `AirPlay::start` fails the `session` is dropped: ffmpeg is killed and the folder removed. The pairing-forget block compares `started.as_ref().is_err_and(...)`; keep it on the same value.)

- [ ] **Step 3: The app**

In `app/src/commands/cast.rs`:

1. Delete `airplay_direct()`. Keep `airplay_profile()`.
2. Replace the AirPlay branch of `delivery` and the media construction:

```rust
    let mut convert = None;
    let delivery = if airplay {
        // The original file in every case: when the receiver cannot play it, ffmpeg converts it here.
        match oneshot_cast::transcode::plan(source, audio, subtitle).map_err(|_| Error::Playback(oneshot_core::codes::CAST_FORMAT.tag("AirPlay cannot play this title: it has no video Flick can convert. Watch it in Flick, or cast it to a Chromecast.")))? {
            oneshot_cast::transcode::Plan::Direct => {}
            oneshot_cast::transcode::Plan::Convert(c) => {
                // Before the local player stops: no ffmpeg, nothing changes on screen.
                if !state.cast.can_convert() {
                    return Err(oneshot_cast::transcode::missing_error());
                }
                convert = Some(c);
            }
        }
        DeliveryRequest::Direct
    } else { /* unchanged */ };
```

3. Audio/subtitle indexes for the AirPlay stream request stay `None` (the original file). Add `convert` to the `CastMedia { … }` literal. The content type for a converted title is set by the caster.

4. In `app/src/main.rs`: `cast: oneshot_cast::Caster::with_store(Arc::new(crate::pairings::VaultPairings)).with_ffmpeg_dirs(libmpv_dirs(&handle)),` (`libmpv_dirs` already lists the exe dir, `resources/libmpv` and the dev folders).

5. `CastMedia` has another construction site? `grep -n "CastMedia {" -r app crates` and add `convert: None` to each (Chromecast path).

- [ ] **Step 4: Reword `CAST_FORMAT`**

In `codes.rs`: `CAST_FORMAT = "FLK-CAST-012", "CAST", "AirPlay cannot play this title: it has no video Flick can convert.", "Watch it in Flick, or cast it to a Chromecast.";`, then regenerate the doc.

- [ ] **Step 5: Verify**

Run:

```bash
UPDATE_ERROR_DOC=1 cargo test -p oneshot-core codes && cargo test -p oneshot-core -p oneshot-cast && cargo check -p Flick
```

Expected: all pass, no warnings about unused `AudioCodec` imports in `cast.rs` (remove the ones left unused by deleting `airplay_direct`).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "AirPlay: convert with ffmpeg when the receiver cannot play the file as it is"
```

---

### Task 7: The UI label

**Files:**
- Modify: `ui/src/features/player/CastPanel.tsx`

- [ ] **Step 1: Change the label**

At `CastPanel.tsx:163` replace `Connecting…` with `Connecting… (a title Flick converts for the device takes a few seconds more)`. Keep it one line; if the text wraps badly in the menu, shorten to `Connecting… (converting if needed)`.

- [ ] **Step 2: Verify and commit**

Run: `cd ui && npx tsc --noEmit`
Expected: no output.

```bash
git add ui/src/features/player/CastPanel.tsx
git commit -m "Cast menu: say a conversion may take a few seconds"
```

---

### Task 8: Shipping ffmpeg

**Files:**
- Modify: `tools/bundle-libmpv-macos.mjs`, `tools/ensure-libmpv.mjs`, `app/tauri.windows.conf.json`, `third_party/mpv/README.md`, `TECHNICAL.md`, `.gitignore`
- Create: `tools/fetch-ffmpeg.ps1`

- [ ] **Step 1: macOS — bundle the `ffmpeg` binary with the dylibs**

In `bundle-libmpv-macos.mjs`, the breadth-first copy starts from `source` (libmpv). Seed it with Homebrew's `ffmpeg` too, so the same relocation, re-signing and "nothing points outside" checks apply and the shared `libav*` are copied once:

```js
const ffmpegSource = (() => {
  try {
    return realpathSync(join(run("brew", ["--prefix", "ffmpeg"]).trim(), "bin", "ffmpeg"));
  } catch {
    console.error("ffmpeg not found. Install it with `brew install ffmpeg`.");
    process.exit(1);
  }
})();
```

Add `ffmpegSource` to `queue` and `names` (`names.set(ffmpegSource, "ffmpeg")`); after the copy loop, `chmodSync(join(dest, "ffmpeg"), 0o755)` (the loop sets 0644). Make the early exit `if (existsSync(entry) && existsSync(join(dest, "ffmpeg")) && !process.argv.includes("--force")) process.exit(0);`. Update the final log to say `libmpv and ffmpeg bundled`.

- [ ] **Step 2: Run it and check the binary works from the folder**

Run:

```bash
node tools/bundle-libmpv-macos.mjs --force && third_party/mpv/macos-arm64/ffmpeg -hide_banner -encoders 2>/dev/null | grep -E "h264_videotoolbox|libx264 |aac "
```

Expected: the three encoders are listed; no `still references` error.

- [ ] **Step 3: Windows**

`tools/fetch-ffmpeg.ps1`:

```powershell
# Downloads a GPL ffmpeg build for Windows x64 into third_party/ffmpeg/windows-x64 (used to convert titles for AirPlay).
# Source: https://github.com/BtbN/FFmpeg-Builds (ffmpeg-master-latest-win64-gpl.zip).
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root "third_party\ffmpeg\windows-x64"
New-Item -ItemType Directory -Force $dest | Out-Null
$archive = Join-Path $env:TEMP "ffmpeg-win64-gpl.zip"
$extracted = Join-Path $env:TEMP "ffmpeg-win64-gpl"
Write-Host "Downloading ffmpeg (GPL build)"
Invoke-WebRequest "https://github.com/BtbN/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-win64-gpl.zip" -OutFile $archive
if (Test-Path $extracted) { Remove-Item -Recurse -Force $extracted }
Expand-Archive $archive -DestinationPath $extracted
$exe = Get-ChildItem -Recurse $extracted -Filter ffmpeg.exe | Select-Object -First 1
if (-not $exe) { throw "ffmpeg.exe not found in the archive" }
Copy-Item $exe.FullName (Join-Path $dest "ffmpeg.exe") -Force
Remove-Item $archive
Remove-Item -Recurse -Force $extracted
Write-Host "ffmpeg ready in $dest"
```

In `ensure-libmpv.mjs`, in the Windows block, after the libmpv step, run the script when `third_party/ffmpeg/windows-x64/ffmpeg.exe` is missing (same `spawnSync("powershell", [...])` call, and propagate a non-zero status). In `app/tauri.windows.conf.json` add `"../third_party/ffmpeg/windows-x64/ffmpeg.exe": "libmpv/ffmpeg.exe"` to `resources` (next to the DLL, where `libmpv_dirs` looks). Add `/third_party/ffmpeg/` to `.gitignore`. The macOS resource entry already copies the whole `macos-arm64/` folder, binary included. Linux: the system `ffmpeg` on `PATH` is used; `ensure-libmpv.mjs` prints a hint when `ffmpeg` is not on `PATH` (a warning, not an error, like libmpv).

- [ ] **Step 4: Docs**

`third_party/mpv/README.md`: add a row for ffmpeg (macOS: bundled next to libmpv by `tools/bundle-libmpv-macos.mjs`; Windows: `tools/fetch-ffmpeg.ps1`; Linux: system) and the lookup order (`ONESHOT_FFMPEG` first). `TECHNICAL.md`: one paragraph "AirPlay conversion" (when it happens, HLS fMP4, the loopback source relay, where the binary lives).

- [ ] **Step 5: Commit**

```bash
git add tools app/tauri.windows.conf.json third_party/mpv/README.md TECHNICAL.md .gitignore
git commit -m "Ship ffmpeg with the app (next to libmpv on macOS, fetched on Windows)"
```

---

### Task 9: End-to-end check with a real clip

**Files:**
- Modify: `docs/PLAYBACK_VALIDATION.md`

- [ ] **Step 1: Run the full suites**

Run:

```bash
cargo test -p oneshot-core -p oneshot-cast && cargo check -p Flick && (cd ui && npx tsc --noEmit)
```

Expected: all green. `a_real_clip_gets_a_playlist_and_the_process_dies_with_the_job` ran (an ffmpeg is on `PATH`); confirm with `cargo test -p oneshot-cast a_real_clip -- --nocapture`.

- [ ] **Step 2: Convert a real MKV with the bundled binary, by hand**

```bash
bash tools/gen-test-media.sh /tmp/flick-media   # use the scratchpad directory instead of /tmp when running as an agent
ONESHOT_FFMPEG=$PWD/third_party/mpv/macos-arm64/ffmpeg cargo test -p oneshot-cast a_real_clip -- --nocapture
```

Expected: pass with the bundled binary (proves the relocated dylibs load). Then, with the app (`cargo tauri dev`), cast an MKV to an AirPlay device and check: it starts, pause/resume work, a scrub inside the produced part is instant, a scrub far ahead restarts (a few seconds of "Buffering…"), stopping leaves no `ffmpeg` in `pgrep -fl ffmpeg` and no `flick-airplay-*` folder in the temp directory. **These last checks need a real AirPlay receiver; the executor cannot run them and must say so rather than claim them.**

- [ ] **Step 3: Record what was and was not verified**

Add a short section to `docs/PLAYBACK_VALIDATION.md` ("AirPlay conversion") listing the checks of Step 2 with their result: measured (unit/integration tests, bundled binary loading) versus pending hardware (real receiver, Windows build).

- [ ] **Step 4: Commit**

```bash
git add docs/PLAYBACK_VALIDATION.md
git commit -m "Validation notes: AirPlay conversion"
```

---

## Self-review

- **Spec coverage:** §1 binary → Task 8; §2 planner, args, job → Tasks 1–3 (credential-safe input via the loopback relay: Task 5 `Session`); §3 relay folder mode and stop cleanup → Tasks 4–5; §4 wiring and UI → Tasks 6–7; §5 codes → Tasks 3 and 6 (`FLK-CAST-012` reworded in 6); §6 tests → in every task, integration in Tasks 3 and 9. The seek restart and the macOS binary location are recorded under "Deviations".
- **Placeholder scan:** none left; the only prose-only steps are the file edits of Tasks 6 and 8, which name the exact lines and code to change.
- **Types:** `Convert` fields are used identically in Tasks 1, 2, 3, 5, 6. `Job::start(&Path, &Convert, &str, u64, &Path)` matches its callers in `Session`. `AirPlay::load(&Url, u64, Option<u64>)` matches `Converting::seek`. `Proxy::serve_dir(PathBuf, IpAddr)` matches `Session::start`.
- **Review Focus coverage:** missing ffmpeg before the player stops (Task 6 Step 3); immediate ffmpeg failure (Task 3 test); orphan process and folder (Tasks 3 and 5 by `kill_on_drop` + `TempDir`, checked by hand in Task 9); path traversal (Task 4 test); seeks at the edges (Task 5 tests); no audio / non-default audio / interlaced (Task 1 tests).
