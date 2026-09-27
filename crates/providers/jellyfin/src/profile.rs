//! `DeviceProfile` generation.
//!
//! Jellyfin decides DirectPlay/DirectStream/Transcode by matching sources
//! against the profile the client sends. A browser-like profile is the #1
//! cause of needless transcoding; ours advertises everything the local
//! engine decodes and every HDR range it can render (passthrough or local
//! tone mapping), so the server only transcodes when *its* policy requires it.

use oneshot_core::playback::ClientProfile;
use oneshot_core::stream::{AudioCodec, SubtitleFormat, VideoCodec};
use serde_json::{Value, json};

fn video_codec(c: &VideoCodec) -> Option<&'static str> {
    Some(match c {
        VideoCodec::H264 => "h264",
        VideoCodec::Hevc => "hevc",
        VideoCodec::Av1 => "av1",
        VideoCodec::Vp9 => "vp9",
        VideoCodec::Vp8 => "vp8",
        VideoCodec::Mpeg2 => "mpeg2video",
        VideoCodec::Mpeg4 => "mpeg4",
        VideoCodec::Vc1 => "vc1",
        VideoCodec::Other(_) => return None,
    })
}

fn audio_codecs(c: &AudioCodec) -> &'static [&'static str] {
    match c {
        AudioCodec::Aac => &["aac"],
        AudioCodec::Ac3 => &["ac3"],
        AudioCodec::Eac3 => &["eac3"],
        // Jellyfin reports DTS-HD as codec "dts" with a profile.
        AudioCodec::Dts | AudioCodec::DtsHd => &["dts", "dca"],
        AudioCodec::TrueHd => &["truehd", "mlp"],
        AudioCodec::Flac => &["flac"],
        AudioCodec::Alac => &["alac"],
        AudioCodec::Opus => &["opus"],
        AudioCodec::Vorbis => &["vorbis"],
        AudioCodec::Mp3 => &["mp3", "mp2"],
        AudioCodec::Pcm => &["pcm_s16le", "pcm_s24le", "pcm_s32le", "pcm_f32le", "pcm_bluray", "pcm_dvd"],
        AudioCodec::Other(_) => &[],
    }
}

fn subtitle_formats(f: SubtitleFormat) -> &'static [&'static str] {
    match f {
        SubtitleFormat::Srt => &["srt", "subrip"],
        SubtitleFormat::Ass => &["ass", "ssa"],
        SubtitleFormat::WebVtt => &["vtt"],
        SubtitleFormat::Pgs => &["pgssub", "pgs"],
        SubtitleFormat::VobSub => &["dvdsub", "vobsub"],
        SubtitleFormat::Dvb => &["dvbsub"],
        SubtitleFormat::Other => &[],
    }
}

/// All `VideoRangeType`s we render: HDR ones via passthrough or tone mapping.
const RANGES: &str = "SDR|HDR10|HDR10Plus|HLG|DOVI|DOVIWithHDR10|DOVIWithHLG|DOVIWithSDR|DOVIWithHDR10Plus|DOVIWithEL|DOVIWithELHDR10Plus";

pub fn device_profile(p: &ClientProfile) -> Value {
    let video: Vec<&str> = p.video_codecs.iter().filter_map(video_codec).collect();
    let audio: Vec<&str> = p.audio_codecs.iter().flat_map(audio_codecs).copied().collect();
    let containers = p.containers.join(",");
    let subtitle_profiles: Vec<Value> = p
        .subtitle_formats
        .iter()
        .flat_map(|f| subtitle_formats(*f))
        .flat_map(|f| {
            let embed = json!({ "Format": f, "Method": "Embed" });
            // Bitmap formats cannot be delivered as sidecar files by Jellyfin.
            if ["pgssub", "pgs", "dvdsub", "vobsub", "dvbsub"].contains(f) {
                vec![embed]
            } else {
                vec![embed, json!({ "Format": f, "Method": "External" })]
            }
        })
        .collect();
    let codec_profiles: Vec<Value> = ["hevc", "av1", "vp9"]
        .iter()
        .filter(|c| video.contains(c))
        .map(|c| {
            json!({
                "Type": "Video",
                "Codec": c,
                "Conditions": [{ "Condition": "EqualsAny", "Property": "VideoRangeType", "Value": RANGES, "IsRequired": false }]
            })
        })
        .collect();
    let transcode_video = if p.video_codecs.contains(&VideoCodec::Hevc) { "hevc,h264" } else { "h264" };

    json!({
        "Name": p.name,
        "MaxStreamingBitrate": p.max_bitrate.unwrap_or(1_000_000_000),
        "MaxStaticBitrate": 1_000_000_000u64,
        "MusicStreamingTranscodingBitrate": 384_000,
        "DirectPlayProfiles": [
            { "Container": containers, "Type": "Video", "VideoCodec": video.join(","), "AudioCodec": audio.join(",") },
            { "Container": "mp3,flac,aac,m4a,ogg,opus,wav,alac,dts,ac3,eac3,truehd", "Type": "Audio" }
        ],
        "TranscodingProfiles": [
            {
                "Container": "ts", "Type": "Video", "Protocol": "hls", "Context": "Streaming",
                "VideoCodec": transcode_video, "AudioCodec": "aac,ac3,eac3",
                "MaxAudioChannels": p.max_audio_channels.to_string(),
                "MinSegments": 1, "BreakOnNonKeyFrames": true, "CopyTimestamps": false
            },
            { "Container": "mp3", "Type": "Audio", "Protocol": "http", "Context": "Streaming", "AudioCodec": "mp3" }
        ],
        "ContainerProfiles": [],
        "CodecProfiles": codec_profiles,
        "SubtitleProfiles": subtitle_profiles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> ClientProfile {
        ClientProfile {
            name: "t".into(),
            max_bitrate: None,
            video_codecs: vec![VideoCodec::H264, VideoCodec::Hevc, VideoCodec::Av1],
            audio_codecs: vec![AudioCodec::TrueHd, AudioCodec::DtsHd, AudioCodec::Aac],
            containers: vec!["mkv".into(), "mp4".into()],
            subtitle_formats: vec![SubtitleFormat::Pgs, SubtitleFormat::Srt],
            max_width: 7680,
            max_height: 4320,
            max_audio_channels: 8,
        }
    }

    #[test]
    fn direct_play_profile_lists_hd_audio_and_hevc() {
        let v = device_profile(&profile());
        let dp = &v["DirectPlayProfiles"][0];
        assert_eq!(dp["Container"], "mkv,mp4");
        assert!(dp["VideoCodec"].as_str().unwrap().contains("hevc"));
        let a = dp["AudioCodec"].as_str().unwrap();
        assert!(a.contains("truehd") && a.contains("dts"));
        assert_eq!(v["TranscodingProfiles"][0]["MaxAudioChannels"], "8");
    }

    #[test]
    fn hdr_ranges_declared_for_hevc() {
        let v = device_profile(&profile());
        let hevc = v["CodecProfiles"].as_array().unwrap().iter().find(|c| c["Codec"] == "hevc").unwrap();
        assert!(hevc["Conditions"][0]["Value"].as_str().unwrap().contains("DOVIWithHDR10"));
    }

    #[test]
    fn pgs_is_embed_only() {
        let v = device_profile(&profile());
        let subs = v["SubtitleProfiles"].as_array().unwrap();
        assert!(!subs.iter().any(|s| s["Format"] == "pgssub" && s["Method"] == "External"));
        assert!(subs.iter().any(|s| s["Format"] == "srt" && s["Method"] == "External"));
    }
}
