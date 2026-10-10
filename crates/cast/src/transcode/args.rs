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
