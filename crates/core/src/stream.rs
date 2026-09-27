//! Technical description of media files: sources (files/versions) and their
//! elementary streams. This is what the playback decision engine reasons on.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum VideoCodec {
    H264,
    Hevc,
    Av1,
    Vp9,
    Vp8,
    Mpeg2,
    Mpeg4,
    Vc1,
    Other(String),
}

impl VideoCodec {
    /// Parses the codec names used by FFmpeg, Jellyfin and Plex.
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "h264" | "avc" | "avc1" => Self::H264,
            "hevc" | "h265" | "hvc1" | "hev1" | "dvhe" | "dvh1" => Self::Hevc,
            "av1" | "av01" => Self::Av1,
            "vp9" => Self::Vp9,
            "vp8" => Self::Vp8,
            "mpeg2video" | "mpeg2" => Self::Mpeg2,
            "mpeg4" | "msmpeg4v3" | "xvid" | "divx" => Self::Mpeg4,
            "vc1" | "wvc1" => Self::Vc1,
            other => Self::Other(other.to_owned()),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::H264 => "H.264",
            Self::Hevc => "HEVC",
            Self::Av1 => "AV1",
            Self::Vp9 => "VP9",
            Self::Vp8 => "VP8",
            Self::Mpeg2 => "MPEG-2",
            Self::Mpeg4 => "MPEG-4",
            Self::Vc1 => "VC-1",
            Self::Other(s) => s,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum AudioCodec {
    Aac,
    Ac3,
    Eac3,
    Dts,
    /// DTS-HD (High Resolution or Master Audio); core is plain DTS.
    DtsHd,
    TrueHd,
    Flac,
    Alac,
    Opus,
    Vorbis,
    Mp3,
    Pcm,
    Other(String),
}

impl AudioCodec {
    /// Parses a codec name plus optional profile (Jellyfin reports DTS-HD as
    /// codec `dts` + profile `DTS-HD MA`; Plex as `dca` + profile `ma`).
    pub fn parse(codec: &str, profile: Option<&str>) -> Self {
        let profile = profile.unwrap_or_default().to_ascii_lowercase();
        match codec.to_ascii_lowercase().as_str() {
            "aac" => Self::Aac,
            "ac3" | "a52" => Self::Ac3,
            "eac3" | "e-ac-3" | "ec3" => Self::Eac3,
            "dts" | "dca" => {
                // Jellyfin: "DTS-HD MA", "DTS-HD HRA", "DTS:X", "DTS Express";
                // Plex: "ma", "hra", "x", "express".
                let hd = matches!(profile.as_str(), "ma" | "hra" | "x" | "express")
                    || ["dts-hd", "dts:x", "express"].iter().any(|p| profile.contains(p));
                if hd { Self::DtsHd } else { Self::Dts }
            }
            "dtshd" | "dts-hd" => Self::DtsHd,
            "truehd" | "mlp" => Self::TrueHd,
            "flac" => Self::Flac,
            "alac" => Self::Alac,
            "opus" => Self::Opus,
            "vorbis" => Self::Vorbis,
            "mp3" | "mp2" => Self::Mp3,
            c if c.starts_with("pcm") || c == "lpcm" => Self::Pcm,
            other => Self::Other(other.to_owned()),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Aac => "AAC",
            Self::Ac3 => "Dolby Digital",
            Self::Eac3 => "Dolby Digital Plus",
            Self::Dts => "DTS",
            Self::DtsHd => "DTS-HD",
            Self::TrueHd => "Dolby TrueHD",
            Self::Flac => "FLAC",
            Self::Alac => "ALAC",
            Self::Opus => "Opus",
            Self::Vorbis => "Vorbis",
            Self::Mp3 => "MP3",
            Self::Pcm => "PCM",
            Self::Other(s) => s,
        }
    }

    /// Codecs that can be carried as an IEC 61937 bitstream to an AV receiver.
    pub fn bitstream_format(&self) -> Option<BitstreamFormat> {
        match self {
            Self::Ac3 => Some(BitstreamFormat::Ac3),
            Self::Eac3 => Some(BitstreamFormat::Eac3),
            Self::Dts => Some(BitstreamFormat::Dts),
            Self::DtsHd => Some(BitstreamFormat::DtsHd),
            Self::TrueHd => Some(BitstreamFormat::TrueHd),
            _ => None,
        }
    }
}

/// Compressed formats that can be sent untouched ("bitstreamed") over
/// S/PDIF or HDMI. Names match mpv's `--audio-spdif` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "kebab-case")]
pub enum BitstreamFormat {
    Ac3,
    Eac3,
    Dts,
    DtsHd,
    #[serde(rename = "truehd")]
    TrueHd,
}

impl BitstreamFormat {
    pub const ALL: [Self; 5] = [Self::Ac3, Self::Eac3, Self::Dts, Self::DtsHd, Self::TrueHd];

    /// Value for mpv's `--audio-spdif`.
    pub fn mpv_name(self) -> &'static str {
        match self {
            Self::Ac3 => "ac3",
            Self::Eac3 => "eac3",
            Self::Dts => "dts",
            Self::DtsHd => "dts-hd",
            Self::TrueHd => "truehd",
        }
    }

    /// S/PDIF (IEC 60958) only has the bandwidth for AC3 and DTS core.
    /// E-AC3 needs 4x rate and DTS-HD/TrueHD need HDMI HBR (8ch @ 192 kHz).
    pub fn fits_spdif(self) -> bool {
        matches!(self, Self::Ac3 | Self::Dts)
    }
}

/// Object-based / immersive audio carried inside a codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "kebab-case")]
pub enum SpatialAudio {
    /// Atmos in TrueHD or in E-AC3 (JOC). Only reproducible via bitstream to
    /// an Atmos-capable receiver; local decoding yields the channel bed only.
    DolbyAtmos,
    /// DTS:X inside DTS-HD MA. Same constraint as Atmos.
    DtsX,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "kebab-case")]
pub enum DolbyVisionCompat {
    /// Profile 5: IPTPQc2, no fallback layer. Wrong colours if not reshaped.
    None,
    /// Profile 8.1 / 7: HDR10-compatible base layer.
    Hdr10,
    /// Profile 8.2: SDR-compatible base layer.
    Sdr,
    /// Profile 8.4: HLG-compatible base layer.
    Hlg,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "kind", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum DynamicRange {
    Sdr,
    Hdr10,
    Hdr10Plus,
    Hlg,
    DolbyVision {
        profile: Option<u8>,
        compat: DolbyVisionCompat,
        /// Profile 7 enhancement layer present (FEL/MEL).
        enhancement_layer: bool,
    },
    Unknown,
}

impl DynamicRange {
    pub fn is_hdr(&self) -> bool {
        !matches!(self, Self::Sdr | Self::Unknown)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Sdr => "SDR",
            Self::Hdr10 => "HDR10",
            Self::Hdr10Plus => "HDR10+",
            Self::Hlg => "HLG",
            Self::DolbyVision { .. } => "Dolby Vision",
            Self::Unknown => "Unknown",
        }
    }

    /// Parses Jellyfin's `VideoRangeType`.
    pub fn from_jellyfin_range_type(s: &str) -> Self {
        let dv = |compat, el| Self::DolbyVision { profile: None, compat, enhancement_layer: el };
        match s {
            "SDR" => Self::Sdr,
            "HDR10" => Self::Hdr10,
            "HDR10Plus" => Self::Hdr10Plus,
            "HLG" => Self::Hlg,
            "DOVI" => dv(DolbyVisionCompat::None, false),
            "DOVIWithHDR10" | "DOVIWithHDR10Plus" => dv(DolbyVisionCompat::Hdr10, false),
            "DOVIWithSDR" => dv(DolbyVisionCompat::Sdr, false),
            "DOVIWithHLG" => dv(DolbyVisionCompat::Hlg, false),
            "DOVIWithEL" | "DOVIWithELHDR10Plus" => dv(DolbyVisionCompat::Hdr10, true),
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct VideoStream {
    pub index: u32,
    pub codec: VideoCodec,
    pub profile: Option<String>,
    pub level: Option<f32>,
    pub width: u32,
    pub height: u32,
    pub bit_depth: Option<u8>,
    pub frame_rate: Option<f32>,
    pub bitrate: Option<u64>,
    pub range: DynamicRange,
    pub interlaced: bool,
    pub title: Option<String>,
    pub is_default: bool,
}

impl VideoStream {
    /// Human resolution class ("4K", "1080p"...), based on width so that
    /// scope (2.39:1) encodes are not misclassified.
    pub fn resolution_label(&self) -> &'static str {
        match (self.width, self.height) {
            (w, _) if w >= 7000 => "8K",
            (w, h) if w >= 3200 || h >= 2000 => "4K",
            (w, h) if w >= 2200 || h >= 1300 => "1440p",
            (w, h) if w >= 1700 || h >= 1000 => "1080p",
            (w, h) if w >= 1100 || h >= 700 => "720p",
            _ => "SD",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AudioStream {
    pub index: u32,
    pub codec: AudioCodec,
    pub profile: Option<String>,
    pub channels: u8,
    pub channel_layout: Option<String>,
    pub sample_rate: Option<u32>,
    pub bitrate: Option<u64>,
    pub spatial: Option<SpatialAudio>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_default: bool,
    pub is_commentary: bool,
}

impl AudioStream {
    pub fn channels_label(&self) -> String {
        match self.channels {
            1 => "Mono".into(),
            2 => "Stereo".into(),
            3 => "2.1".into(),
            6 => "5.1".into(),
            7 => "6.1".into(),
            8 => "7.1".into(),
            n => format!("{n}ch"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum SubtitleFormat {
    /// Text formats rendered locally by libass.
    Srt,
    Ass,
    WebVtt,
    /// Bitmap formats (Blu-ray PGS, DVD VobSub, DVB).
    Pgs,
    VobSub,
    Dvb,
    Other,
}

impl SubtitleFormat {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "srt" | "subrip" => Self::Srt,
            "ass" | "ssa" => Self::Ass,
            "vtt" | "webvtt" => Self::WebVtt,
            "pgs" | "pgssub" | "hdmv_pgs_subtitle" => Self::Pgs,
            "vobsub" | "dvdsub" | "dvd_subtitle" => Self::VobSub,
            "dvbsub" | "dvb_subtitle" => Self::Dvb,
            _ => Self::Other,
        }
    }

    pub fn is_bitmap(self) -> bool {
        matches!(self, Self::Pgs | Self::VobSub | Self::Dvb)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct SubtitleStream {
    pub index: u32,
    pub format: SubtitleFormat,
    pub language: Option<String>,
    pub title: Option<String>,
    pub forced: bool,
    pub hearing_impaired: bool,
    pub is_default: bool,
    /// Sidecar file served separately by the server (not muxed in the container).
    pub external: bool,
    /// Provider-internal path used to fetch an external subtitle.
    #[serde(skip)]
    pub delivery_path: Option<String>,
}

/// One playable version of an item (a file, or one "version" of a movie).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct MediaSource {
    pub id: String,
    pub name: Option<String>,
    pub container: Option<String>,
    pub size_bytes: Option<u64>,
    pub bitrate: Option<u64>,
    pub duration_ms: Option<u64>,
    pub video: Vec<VideoStream>,
    pub audio: Vec<AudioStream>,
    pub subtitles: Vec<SubtitleStream>,
}

impl MediaSource {
    pub fn primary_video(&self) -> Option<&VideoStream> {
        self.video.iter().find(|v| v.is_default).or_else(|| self.video.first())
    }

    pub fn default_audio(&self) -> Option<&AudioStream> {
        self.audio.iter().find(|a| a.is_default).or_else(|| self.audio.first())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dts_profiles() {
        assert_eq!(AudioCodec::parse("dts", Some("DTS-HD MA")), AudioCodec::DtsHd);
        assert_eq!(AudioCodec::parse("dca", Some("ma")), AudioCodec::DtsHd);
        assert_eq!(AudioCodec::parse("dts", Some("DTS:X")), AudioCodec::DtsHd);
        assert_eq!(AudioCodec::parse("dts", None), AudioCodec::Dts);
        assert_eq!(AudioCodec::parse("dca", Some("dts")), AudioCodec::Dts);
    }

    #[test]
    fn spdif_bandwidth() {
        assert!(BitstreamFormat::Ac3.fits_spdif());
        assert!(!BitstreamFormat::TrueHd.fits_spdif());
        assert!(!BitstreamFormat::Eac3.fits_spdif());
    }

    #[test]
    fn scope_4k_is_4k() {
        let v = VideoStream {
            index: 0,
            codec: VideoCodec::Hevc,
            profile: None,
            level: None,
            width: 3840,
            height: 1606,
            bit_depth: Some(10),
            frame_rate: None,
            bitrate: None,
            range: DynamicRange::Hdr10,
            interlaced: false,
            title: None,
            is_default: true,
        };
        assert_eq!(v.resolution_label(), "4K");
    }
}
