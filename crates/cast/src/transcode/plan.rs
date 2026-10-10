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
    /// A text subtitle as a file of its own (the server extracted it), set when the conversion starts.
    /// Without it the `subtitles` filter reads the whole title to collect the subtitles: a 4 GB film over
    /// the network never starts.
    pub file: Option<String>,
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
            SubtitleFormat::Srt | SubtitleFormat::Ass | SubtitleFormat::WebVtt => Some(Burn { position, bitmap: false, file: None }),
            SubtitleFormat::Pgs | SubtitleFormat::VobSub | SubtitleFormat::Dvb => Some(Burn { position, bitmap: true, file: None }),
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
        assert_eq!((c.video, c.burn), (VideoPlan::H264, Some(Burn { position: 0, bitmap: false, file: None })));
        let Ok(Plan::Convert(c)) = plan(&s, s.default_audio(), s.subtitles.get(1)) else { panic!("not converted") };
        assert_eq!(c.burn, Some(Burn { position: 1, bitmap: true, file: None }));
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
