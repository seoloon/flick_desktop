//! The client profile sent to servers so they never transcode "because they
//! do not know this client". It advertises what the *engine* decodes (FFmpeg
//! inside libmpv), not what the GPU accelerates: CPU decoding is still local
//! playback, and the policy for "too heavy without GPU" lives in `decide`.

use oneshot_core::capabilities::CapabilityReport;
use oneshot_core::playback::ClientProfile;
use oneshot_core::settings::Settings;
use oneshot_core::stream::{AudioCodec, SubtitleFormat};

pub const CONTAINERS: &[&str] =
    &["mkv", "webm", "mp4", "m4v", "mov", "ts", "mpegts", "m2ts", "avi", "flv", "wmv", "asf", "ogg", "ogv", "3gp", "mpg", "mpeg", "vob"];

pub fn client_profile(caps: &CapabilityReport, settings: &Settings) -> ClientProfile {
    ClientProfile {
        name: "Flick (libmpv)".into(),
        max_bitrate: settings.playback.max_bitrate,
        video_codecs: caps.video.software_codecs.clone(),
        audio_codecs: vec![
            AudioCodec::Aac,
            AudioCodec::Ac3,
            AudioCodec::Eac3,
            AudioCodec::Dts,
            AudioCodec::DtsHd,
            AudioCodec::TrueHd,
            AudioCodec::Flac,
            AudioCodec::Alac,
            AudioCodec::Opus,
            AudioCodec::Vorbis,
            AudioCodec::Mp3,
            AudioCodec::Pcm,
        ],
        containers: CONTAINERS.iter().map(|c| (*c).to_owned()).collect(),
        subtitle_formats: vec![
            SubtitleFormat::Srt,
            SubtitleFormat::Ass,
            SubtitleFormat::WebVtt,
            SubtitleFormat::Pgs,
            SubtitleFormat::VobSub,
            SubtitleFormat::Dvb,
        ],
        max_width: 7680,
        max_height: 4320,
        // Decoding handles any layout up to 7.1; the output stage downmixes
        // locally, so the server must not downmix on our behalf.
        max_audio_channels: 8,
    }
}
