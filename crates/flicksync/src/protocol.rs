//! FlickSync wire protocol, version 1 (see FlickServer `docs/protocol.md`).
//!
//! Everything received from the server is untrusted: it is parsed into these
//! types and then validated ([`validate_snapshot`], [`MediaRef::validate`])
//! before the rest of the client looks at it. Unknown message types and
//! unknown fields are ignored so the server can evolve.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::Error;

pub const PROTOCOL_VERSION: u32 = 1;
/// Positions above this are refused (the server's own limit: 7 days).
pub const MAX_POSITION_SECS: f64 = 7.0 * 24.0 * 3600.0;
pub const RATE_MIN: f64 = 0.25;
pub const RATE_MAX: f64 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Jellyfin,
    Plex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
    Movie,
    Episode,
}

/// Identity of a title. FlickSync only carries it; every client resolves it
/// through its own Jellyfin/Plex connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct MediaRef {
    pub provider: Provider,
    pub server_id: String,
    pub media_id: String,
    pub media_type: MediaType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub season_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_id: Option<String>,
    /// Display only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
}

/// `[A-Za-z0-9._:-]{1,128}`: what the server accepts as an id.
pub fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

impl MediaRef {
    pub fn validate(&self) -> Result<(), Error> {
        let ids = [Some(&self.server_id), Some(&self.media_id), self.season_id.as_ref(), self.episode_id.as_ref()];
        if ids.into_iter().flatten().any(|id| !valid_id(id)) {
            return Err(Error::Protocol("invalid media identifier".into()));
        }
        if self.duration_secs.is_some_and(|d| !d.is_finite() || !(0.0..=MAX_POSITION_SECS).contains(&d)) {
            return Err(Error::Protocol("invalid media duration".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum PlayState {
    Playing,
    #[default]
    Paused,
}

/// Canonical playback: a state with a reference timestamp, not a stream of
/// positions. `server_time` is milliseconds since the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlaybackSnapshot {
    pub state: PlayState,
    pub position: f64,
    pub rate: f64,
    pub server_time: f64,
    pub sequence: u64,
}

/// Rejects NaN/inf/out-of-range values: the server is not trusted blindly.
pub fn validate_snapshot(s: &PlaybackSnapshot) -> Result<(), Error> {
    let ok = s.position.is_finite()
        && (0.0..=MAX_POSITION_SECS).contains(&s.position)
        && s.rate.is_finite()
        && (RATE_MIN..=RATE_MAX).contains(&s.rate)
        && s.server_time.is_finite();
    if ok { Ok(()) } else { Err(Error::Protocol("invalid playback state".into())) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    #[default]
    Connected,
    Reconnecting,
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Participant {
    pub participant_id: String,
    pub display_name: String,
    #[serde(default)]
    pub presence: Presence,
    #[serde(default)]
    pub is_host: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ControlMode {
    #[default]
    Everyone,
    HostOnly,
}

/// `room_state.room`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RoomView {
    pub room_id: String,
    pub host_id: String,
    #[serde(default)]
    pub control_mode: ControlMode,
    #[serde(default)]
    pub chat_enabled: bool,
    #[serde(default)]
    pub participants: Vec<Participant>,
    #[serde(default)]
    pub media: Option<MediaRef>,
    pub playback: PlaybackSnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ChatMessage {
    pub id: u64,
    pub sender_id: String,
    pub sender_name: String,
    pub text: String,
    /// Milliseconds since the Unix epoch.
    pub timestamp: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Correction {
    AdjustRate { rate: f64, duration_ms: f64, sequence: u64 },
    Seek { position: f64, sequence: u64 },
}

/// Server → client messages that matter to the client. Anything else is
/// [`ServerMessage::Unknown`] and ignored.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerMessage {
    RoomState { room: Box<RoomView>, you: String, server_time: f64 },
    ParticipantJoined(Participant),
    ParticipantLeft { participant_id: String, reason: String },
    PresenceChanged { participant_id: String, presence: Presence },
    MediaSelected { media: MediaRef, playback: PlaybackSnapshot, by: String },
    Playback { kind: PlaybackKind, by: Option<String>, snapshot: PlaybackSnapshot },
    SyncState { snapshot: PlaybackSnapshot },
    SyncCorrection(Correction),
    RoomUpdated { host_id: Option<String>, control_mode: Option<ControlMode>, chat_enabled: Option<bool>, reason: String },
    RoomClosed { reason: String },
    Chat(ChatMessage),
    ChatHistory(Vec<ChatMessage>),
    Pong { client_time: f64, server_time: f64 },
    Error { code: String, message: String },
    Unknown(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackKind {
    Play,
    Pause,
    Seek,
    RateChanged,
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    protocol_version: Option<u32>,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    payload: Value,
}

#[derive(Deserialize)]
struct SnapshotPayload {
    #[serde(default)]
    by: Option<String>,
    #[serde(flatten)]
    snapshot: PlaybackSnapshot,
}

fn payload<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, Error> {
    serde_json::from_value(v).map_err(|e| Error::Protocol(format!("malformed message: {e}")))
}

/// Parses one text frame. Malformed frames are an error (the caller logs and
/// drops them; they never terminate the connection). Unknown types are fine.
pub fn parse_server_message(text: &str) -> Result<ServerMessage, Error> {
    let env: Envelope = serde_json::from_str(text).map_err(|e| Error::Protocol(format!("not a valid frame: {e}")))?;
    if let Some(v) = env.protocol_version
        && v != PROTOCOL_VERSION
    {
        return Err(Error::IncompatibleVersion);
    }
    let p = env.payload;
    let msg = match env.kind.as_str() {
        "room_state" => {
            #[derive(Deserialize)]
            struct P {
                room: RoomView,
                you: String,
                #[serde(default)]
                server_time: f64,
            }
            let P { room, you, server_time } = payload(p)?;
            validate_snapshot(&room.playback)?;
            if let Some(m) = &room.media {
                m.validate()?;
            }
            ServerMessage::RoomState { room: Box::new(room), you, server_time }
        }
        "participant_joined" => {
            #[derive(Deserialize)]
            struct P {
                participant: Participant,
            }
            ServerMessage::ParticipantJoined(payload::<P>(p)?.participant)
        }
        "participant_left" => {
            #[derive(Deserialize)]
            struct P {
                participant_id: String,
                #[serde(default)]
                reason: String,
            }
            let P { participant_id, reason } = payload(p)?;
            ServerMessage::ParticipantLeft { participant_id, reason }
        }
        "presence_changed" => {
            #[derive(Deserialize)]
            struct P {
                participant_id: String,
                presence: Presence,
            }
            let P { participant_id, presence } = payload(p)?;
            ServerMessage::PresenceChanged { participant_id, presence }
        }
        "media_selected" => {
            #[derive(Deserialize)]
            struct P {
                media: MediaRef,
                playback: PlaybackSnapshot,
                #[serde(default)]
                by: String,
            }
            let P { media, playback, by } = payload(p)?;
            media.validate()?;
            validate_snapshot(&playback)?;
            ServerMessage::MediaSelected { media, playback, by }
        }
        k @ ("playback_play" | "playback_pause" | "playback_seek" | "playback_rate_changed") => {
            let SnapshotPayload { by, snapshot } = payload(p)?;
            validate_snapshot(&snapshot)?;
            let kind = match k {
                "playback_play" => PlaybackKind::Play,
                "playback_pause" => PlaybackKind::Pause,
                "playback_seek" => PlaybackKind::Seek,
                _ => PlaybackKind::RateChanged,
            };
            ServerMessage::Playback { kind, by, snapshot }
        }
        "sync_state" => {
            let SnapshotPayload { snapshot, .. } = payload(p)?;
            validate_snapshot(&snapshot)?;
            ServerMessage::SyncState { snapshot }
        }
        "sync_correction" => {
            let c: Correction = payload(p)?;
            match &c {
                Correction::AdjustRate { rate, duration_ms, .. }
                    if !(rate.is_finite() && (RATE_MIN..=RATE_MAX).contains(rate) && duration_ms.is_finite() && *duration_ms >= 0.0) =>
                {
                    return Err(Error::Protocol("invalid correction".into()));
                }
                Correction::Seek { position, .. } if !(position.is_finite() && (0.0..=MAX_POSITION_SECS).contains(position)) => {
                    return Err(Error::Protocol("invalid correction".into()));
                }
                _ => {}
            }
            ServerMessage::SyncCorrection(c)
        }
        "room_updated" => {
            #[derive(Deserialize)]
            struct P {
                #[serde(default)]
                host_id: Option<String>,
                #[serde(default)]
                control_mode: Option<ControlMode>,
                #[serde(default)]
                chat_enabled: Option<bool>,
                #[serde(default)]
                reason: String,
            }
            let P { host_id, control_mode, chat_enabled, reason } = payload(p)?;
            ServerMessage::RoomUpdated { host_id, control_mode, chat_enabled, reason }
        }
        "room_closed" => {
            #[derive(Deserialize)]
            struct P {
                #[serde(default)]
                reason: String,
            }
            ServerMessage::RoomClosed { reason: payload::<P>(p)?.reason }
        }
        "chat_message" => ServerMessage::Chat(payload(p)?),
        "chat_history" => {
            #[derive(Deserialize)]
            struct P {
                #[serde(default)]
                messages: Vec<ChatMessage>,
            }
            ServerMessage::ChatHistory(payload::<P>(p)?.messages)
        }
        "pong" => {
            #[derive(Deserialize)]
            struct P {
                client_time: f64,
                server_time: f64,
            }
            let P { client_time, server_time } = payload(p)?;
            if !client_time.is_finite() || !server_time.is_finite() {
                return Err(Error::Protocol("invalid pong".into()));
            }
            ServerMessage::Pong { client_time, server_time }
        }
        "error" => {
            #[derive(Deserialize)]
            struct P {
                code: String,
                #[serde(default)]
                message: String,
            }
            let P { code, message } = payload(p)?;
            ServerMessage::Error { code, message }
        }
        other => ServerMessage::Unknown(other.chars().take(64).collect()),
    };
    Ok(msg)
}

/// Client → server messages.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientMessage {
    Ping { client_time: f64, rtt_ms: Option<f64> },
    SelectMedia(MediaRef),
    Play { position: Option<f64>, sequence: Option<u64> },
    Pause { position: Option<f64>, sequence: Option<u64> },
    Seek { position: f64, sequence: Option<u64> },
    Rate { rate: f64, sequence: Option<u64> },
    SyncRequest,
    SyncReport { position: f64, sequence: u64, state: PlayState, buffering: bool, rtt_ms: Option<f64> },
    Chat { text: String },
    UpdateRoom { control_mode: Option<ControlMode>, chat_enabled: Option<bool> },
    CloseRoom,
    Leave,
}

impl ClientMessage {
    pub fn to_json(&self) -> String {
        use serde_json::json;
        let (kind, payload) = match self {
            Self::Ping { client_time, rtt_ms } => ("ping", json!({ "client_time": client_time, "rtt_ms": rtt_ms })),
            Self::SelectMedia(m) => ("select_media", json!({ "media": m })),
            Self::Play { position, sequence } => ("playback_play", json!({ "position": position, "sequence": sequence })),
            Self::Pause { position, sequence } => ("playback_pause", json!({ "position": position, "sequence": sequence })),
            Self::Seek { position, sequence } => ("playback_seek", json!({ "position": position, "sequence": sequence })),
            Self::Rate { rate, sequence } => ("playback_rate_changed", json!({ "rate": rate, "sequence": sequence })),
            Self::SyncRequest => ("sync_request", json!({})),
            Self::SyncReport { position, sequence, state, buffering, rtt_ms } => (
                "sync_report",
                json!({ "position": position, "sequence": sequence, "state": state, "buffering": buffering, "rtt_ms": rtt_ms }),
            ),
            Self::Chat { text } => ("chat_message", json!({ "text": text })),
            Self::UpdateRoom { control_mode, chat_enabled } => {
                ("update_room", json!({ "control_mode": control_mode, "chat_enabled": chat_enabled }))
            }
            Self::CloseRoom => ("close_room", json!({})),
            Self::Leave => ("leave_room", json!({})),
        };
        // `null` optionals are dropped: the server treats a missing field as "not provided".
        let mut payload = payload;
        if let Some(o) = payload.as_object_mut() {
            o.retain(|_, v| !v.is_null());
        }
        json!({ "protocol_version": PROTOCOL_VERSION, "type": kind, "payload": payload }).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(kind: &str, payload: &str) -> String {
        format!(r#"{{"protocol_version":1,"type":"{kind}","payload":{payload}}}"#)
    }

    #[test]
    fn parses_a_pause_broadcast() {
        let f = frame(
            "playback_pause",
            r#"{"by":"alice","state":"paused","position":123.42,"rate":1.0,"server_time":1790887546683,"sequence":42}"#,
        );
        let ServerMessage::Playback { kind, by, snapshot } = parse_server_message(&f).unwrap() else { panic!() };
        assert_eq!(kind, PlaybackKind::Pause);
        assert_eq!(by.as_deref(), Some("alice"));
        assert_eq!(snapshot.sequence, 42);
        assert_eq!(snapshot.state, PlayState::Paused);
    }

    #[test]
    fn parses_room_state_and_ignores_unknown_fields() {
        let f = frame(
            "room_state",
            r#"{"room":{"room_id":"R","state":"playing","host_id":"alice","control_mode":"host_only","chat_enabled":true,"future":1,
            "participants":[{"participant_id":"alice","display_name":"Alice","presence":"connected","is_host":true,"joined_at":1}],
            "media":{"provider":"jellyfin","server_id":"s1","media_id":"m1","media_type":"movie","title":"T"},
            "playback":{"state":"playing","position":102.0,"rate":1.0,"server_time":1790887546683,"sequence":3}},
            "you":"bob","server_time":1790887546683}"#,
        );
        let ServerMessage::RoomState { room, you, .. } = parse_server_message(&f).unwrap() else { panic!() };
        assert_eq!(you, "bob");
        assert_eq!(room.control_mode, ControlMode::HostOnly);
        assert!(room.participants[0].is_host);
        assert_eq!(room.media.unwrap().provider, Provider::Jellyfin);
    }

    #[test]
    fn unknown_types_are_ignored_not_errors() {
        assert_eq!(parse_server_message(&frame("brand_new", "{}")).unwrap(), ServerMessage::Unknown("brand_new".into()));
    }

    #[test]
    fn rejects_malformed_frames() {
        assert!(parse_server_message("not json").is_err());
        assert!(parse_server_message(&frame("playback_pause", r#"{"state":"paused"}"#)).is_err());
        assert!(parse_server_message(&frame("playback_pause", "42")).is_err());
    }

    #[test]
    fn rejects_hostile_values() {
        let bad_pos = frame("sync_state", r#"{"state":"playing","position":-1,"rate":1.0,"server_time":1,"sequence":1}"#);
        assert!(parse_server_message(&bad_pos).is_err());
        let bad_rate = frame("sync_state", r#"{"state":"playing","position":1,"rate":99.0,"server_time":1,"sequence":1}"#);
        assert!(parse_server_message(&bad_rate).is_err());
        let huge = frame("sync_state", r#"{"state":"playing","position":1e300,"rate":1.0,"server_time":1,"sequence":1}"#);
        assert!(parse_server_message(&huge).is_err());
        let bad_media = frame(
            "media_selected",
            r#"{"media":{"provider":"plex","server_id":"s","media_id":"http://evil/x","media_type":"movie"},
            "playback":{"state":"paused","position":0,"rate":1.0,"server_time":1,"sequence":1},"by":"a"}"#,
        );
        assert!(parse_server_message(&bad_media).is_err());
    }

    #[test]
    fn rejects_other_protocol_versions() {
        let f = r#"{"protocol_version":2,"type":"pong","payload":{"client_time":1,"server_time":2}}"#;
        assert!(matches!(parse_server_message(f), Err(Error::IncompatibleVersion)));
    }

    #[test]
    fn corrections_are_validated() {
        let ok = frame(
            "sync_correction",
            r#"{"action":"adjust_rate","rate":1.03,"duration_ms":10000,"drift_ms":-300.0,"sequence":7,"server_time":1}"#,
        );
        assert!(matches!(parse_server_message(&ok).unwrap(), ServerMessage::SyncCorrection(Correction::AdjustRate { .. })));
        let bad = frame("sync_correction", r#"{"action":"seek","position":-5,"sequence":7}"#);
        assert!(parse_server_message(&bad).is_err());
    }

    #[test]
    fn client_messages_drop_null_options_and_carry_the_version() {
        let v: Value = serde_json::from_str(&ClientMessage::Play { position: None, sequence: Some(4) }.to_json()).unwrap();
        assert_eq!(v["protocol_version"], 1);
        assert_eq!(v["type"], "playback_play");
        assert!(v["payload"].get("position").is_none());
        assert_eq!(v["payload"]["sequence"], 4);
    }

    #[test]
    fn media_ids_are_checked() {
        assert!(valid_id("abc-1.2_3:x"));
        assert!(!valid_id(""));
        assert!(!valid_id("a/b"));
        assert!(!valid_id(&"a".repeat(129)));
    }
}
