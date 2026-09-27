//! Player state and events exposed to the UI.

use oneshot_core::ids::ItemRef;
use oneshot_core::playback::PlaybackDecision;
use oneshot_mpv::Node;
use serde::Serialize;

use crate::presenter::PresenterKind;
use crate::tracks::Track;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    #[default]
    Idle,
    Loading,
    Playing,
    Paused,
    Buffering,
    Ended,
    Error,
}

/// What mpv is *actually* doing (vs. what the decision planned).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LiveStats {
    pub hwdec: Option<String>,
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub video_params: Option<Node>,
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub video_target: Option<Node>,
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub audio_params: Option<Node>,
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub audio_out: Option<Node>,
    pub current_ao: Option<String>,
    pub container_fps: Option<f64>,
    pub display_fps: Option<f64>,
    pub dropped_frames: Option<i64>,
    pub decoder_dropped_frames: Option<i64>,
    pub avsync: Option<f64>,
    pub video_bitrate: Option<f64>,
    pub audio_bitrate: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    pub title: Option<String>,
    pub start_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PlayerSnapshot {
    pub phase: Phase,
    pub item: Option<ItemRef>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub buffered_ms: Option<u64>,
    pub volume: f64,
    pub muted: bool,
    pub tracks: Vec<Track>,
    pub chapters: Vec<Chapter>,
    pub decision: Option<PlaybackDecision>,
    pub presenter: Option<PresenterKind>,
    /// Effective mpv properties applied for this file (Debug panel).
    pub applied_options: Vec<(String, String)>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "type")]
pub enum PlayerEvent {
    /// Throttled state (4 Hz while playing, immediate on transitions).
    State {
        phase: Phase,
        position_ms: u64,
        duration_ms: Option<u64>,
        buffered_ms: Option<u64>,
        volume: f64,
        muted: bool,
    },
    Tracks { tracks: Vec<Track> },
    Chapters { chapters: Vec<Chapter> },
    Decision { decision: PlaybackDecision },
    Ended { item: Option<ItemRef>, natural: bool },
    Error { message: String },
}
