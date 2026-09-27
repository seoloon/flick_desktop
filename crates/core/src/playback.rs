//! Playback contracts between providers, the decision engine and the player.

use serde::{Deserialize, Serialize};
use url::Url;

use crate::ids::ItemRef;
use crate::stream::{AudioCodec, BitstreamFormat, MediaSource, SubtitleFormat, VideoCodec};

/// What the client declares it can decode *locally*. Sent to the server so it
/// never transcodes merely because it does not know the client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ClientProfile {
    pub name: String,
    /// `None` = unlimited (LAN). Remote limits are applied by the server.
    pub max_bitrate: Option<u64>,
    pub video_codecs: Vec<VideoCodec>,
    pub audio_codecs: Vec<AudioCodec>,
    /// Containers the demuxer handles (FFmpeg: effectively all).
    pub containers: Vec<String>,
    pub subtitle_formats: Vec<SubtitleFormat>,
    /// Maximum decodable resolution (bounded by HW decoder limits when SW
    /// decode would be too slow; see decision engine).
    pub max_width: u32,
    pub max_height: u32,
    pub max_audio_channels: u8,
}

/// Server-side view of a source: what the server allows for this user/network.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ServerPolicy {
    pub direct_play_allowed: bool,
    pub direct_stream_allowed: bool,
    pub transcode_allowed: bool,
    /// Raw reasons reported by the server when it would not direct play.
    pub server_reasons: Vec<String>,
}

impl ServerPolicy {
    pub fn permissive() -> Self {
        Self { direct_play_allowed: true, direct_stream_allowed: true, transcode_allowed: true, server_reasons: vec![] }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct SourceOffer {
    pub source: MediaSource,
    pub policy: ServerPolicy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PlaybackInfo {
    pub item: ItemRef,
    pub offers: Vec<SourceOffer>,
    /// Session id the server wants echoed back in reports (Jellyfin).
    pub play_session_id: Option<String>,
}

/// How bytes travel from the server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "type")]
pub enum DeliveryRequest {
    /// Original file, untouched.
    Direct,
    /// Streams copied into another container (no re-encode).
    Remux { container: String },
    /// Server re-encodes. `None` fields are copied.
    Transcode {
        video: Option<VideoCodec>,
        audio: Option<AudioCodec>,
        max_bitrate: Option<u64>,
        max_width: Option<u32>,
        audio_channels: Option<u8>,
        /// Burn this subtitle stream into the video (only when unavoidable).
        burn_subtitle: Option<u32>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct StreamRequest {
    pub item: ItemRef,
    pub source_id: String,
    pub play_session_id: Option<String>,
    pub delivery: DeliveryRequest,
    pub audio_index: Option<u32>,
    pub subtitle_index: Option<u32>,
    pub start_ms: u64,
}

/// A URL the player can open. Headers carry auth so tokens stay out of URLs
/// where the server supports it.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamTarget {
    pub url: Url,
    pub headers: Vec<(String, String)>,
    /// External subtitle files to side-load (`sub-add`).
    pub external_subtitles: Vec<ExternalSubtitle>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExternalSubtitle {
    pub stream_index: u32,
    pub url: Url,
    pub title: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum PlaybackState {
    Playing,
    Paused,
    Buffering,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ReportKind {
    Start,
    Progress,
    Stop,
}

/// Progress report sent to the server (resume point, "now playing", scrobble).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PlaybackReport {
    pub kind: ReportKind,
    pub item: ItemRef,
    pub source_id: String,
    pub play_session_id: Option<String>,
    pub state: PlaybackState,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub delivery: DeliveryKind,
    pub audio_index: Option<u32>,
    pub subtitle_index: Option<u32>,
    pub volume: u8,
    pub muted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum DeliveryKind {
    DirectPlay,
    DirectStream,
    Transcode,
}

// ---------------------------------------------------------------------------
// Decision (output of the PlaybackDecisionEngine, shown in the tech overlay)
// ---------------------------------------------------------------------------

/// User-facing summary of the strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum StrategyLabel {
    /// File played untouched; local output needs no conversion.
    DirectPlay,
    /// Server remuxes (container change only).
    DirectStream,
    /// File played untouched but converted locally (tone-map, downmix,
    /// audio decode instead of bitstream).
    LocalDecode,
    /// Server re-encodes.
    ServerTranscode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "mode")]
pub enum VideoOutputPlan {
    /// SDR content to an SDR (or HDR-composited) output.
    Sdr,
    /// HDR metadata signalled to an HDR-active display.
    HdrPassthrough { format: String },
    /// HDR content tone-mapped by libplacebo for an SDR display.
    ToneMapToSdr { reason: String },
    /// Dolby Vision RPU applied by libplacebo, output as HDR10/PQ or SDR.
    /// This is *not* Dolby Vision signalling to the display.
    DolbyVisionReshape { output: String },
    /// Server re-encodes; output depends on server capabilities.
    ServerDetermined,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "mode")]
pub enum AudioOutputPlan {
    /// Compressed bitstream sent to the receiver: the source track untouched,
    /// or (`reencoded`) multichannel PCM encoded to AC3 for S/PDIF (lossy).
    Bitstream { format: BitstreamFormat, device: String, reencoded: bool },
    /// Decoded to PCM locally.
    Pcm { source_channels: u8, output_channels: u8, downmix: bool, spatial_lost: bool },
    ServerDetermined,
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "mode")]
pub enum SubtitlePlan {
    None,
    /// Rendered locally by libass / the bitmap renderer, over the video.
    Local { index: u32, external: bool },
    /// Burned by the server (forces a video transcode).
    BurnIn { index: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ReasonSeverity {
    Info,
    Degraded,
    Blocking,
}

/// One explainable step of the decision, shown in diagnostics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DecisionReason {
    pub code: String,
    pub severity: ReasonSeverity,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PlaybackDecision {
    pub label: StrategyLabel,
    pub source_id: String,
    pub delivery: DeliveryRequest,
    pub video_stream: Option<u32>,
    pub audio_stream: Option<u32>,
    pub video: VideoOutputPlan,
    pub audio: AudioOutputPlan,
    pub subtitles: SubtitlePlan,
    /// Whether video decoding is expected to be hardware-accelerated.
    pub hardware_decode: Option<bool>,
    pub reasons: Vec<DecisionReason>,
}
