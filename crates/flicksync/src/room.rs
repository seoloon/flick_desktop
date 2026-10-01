//! The one canonical client-side room state. UI components only ever read
//! copies of it (emitted as events); nothing else keeps its own.

use serde::Serialize;

use crate::protocol::{ChatMessage, ControlMode, MediaRef, Participant, PlayState, PlaybackSnapshot, Presence, RoomView, ServerMessage};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    AuthenticationFailed,
    Unavailable,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PlaybackView {
    pub state: PlayState,
    pub position: f64,
    pub rate: f64,
    pub sequence: u64,
}

impl From<&PlaybackSnapshot> for PlaybackView {
    fn from(s: &PlaybackSnapshot) -> Self {
        Self { state: s.state, position: s.position, rate: s.rate, sequence: s.sequence }
    }
}

/// Keep at most this many chat messages in memory (never persisted).
pub const MAX_CHAT: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct RoomState {
    pub room_id: String,
    pub share_code: String,
    pub host_id: String,
    /// Our own participant id.
    pub you: String,
    pub participants: Vec<Participant>,
    pub media: Option<MediaRef>,
    pub playback: Option<PlaybackView>,
    pub control_mode: ControlMode,
    pub chat_enabled: bool,
    pub connection: ConnectionState,
    pub chat: Vec<ChatMessage>,
    /// Why the room ended, once it did (`host_closed`, `expired`, …).
    pub closed_reason: Option<String>,
}

/// What changed: lets the session react (load media, apply playback…).
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// Full state received (join or reconnect).
    Snapshot,
    Participants,
    Host {
        became_host: bool,
        lost_host: bool,
    },
    Media,
    Playback,
    Settings,
    Chat,
    Closed,
    None,
}

impl RoomState {
    pub fn new(room_id: &str, share_code: &str, you: &str) -> Self {
        Self { room_id: room_id.into(), share_code: share_code.into(), you: you.into(), ..Self::default() }
    }

    pub fn is_host(&self) -> bool {
        !self.you.is_empty() && self.host_id == self.you
    }

    /// Whether we may select media / change room settings. Never trusted for
    /// security: the server decides.
    pub fn can_control_playback(&self) -> bool {
        self.is_host() || self.control_mode == ControlMode::Everyone
    }

    fn mark_host(&mut self) {
        let host = self.host_id.clone();
        for p in &mut self.participants {
            p.is_host = p.participant_id == host;
        }
    }

    pub fn apply_view(&mut self, room: &RoomView, you: &str) {
        self.room_id = room.room_id.clone();
        self.host_id = room.host_id.clone();
        self.you = you.to_owned();
        self.participants = room.participants.clone();
        self.media = room.media.clone();
        self.playback = Some(PlaybackView::from(&room.playback));
        self.control_mode = room.control_mode;
        self.chat_enabled = room.chat_enabled;
        self.mark_host();
    }

    /// Reduces a server message. Playback snapshots are *not* applied here:
    /// the sync engine owns ordering (stale sequences) and calls
    /// [`RoomState::set_playback`] for the ones it accepted.
    pub fn apply(&mut self, msg: &ServerMessage) -> Change {
        match msg {
            ServerMessage::RoomState { room, you, .. } => {
                self.apply_view(room, you);
                self.closed_reason = None;
                Change::Snapshot
            }
            ServerMessage::ParticipantJoined(p) => {
                match self.participants.iter_mut().find(|x| x.participant_id == p.participant_id) {
                    Some(existing) => *existing = p.clone(),
                    None => self.participants.push(p.clone()),
                }
                self.mark_host();
                Change::Participants
            }
            ServerMessage::ParticipantLeft { participant_id, .. } => {
                self.participants.retain(|p| &p.participant_id != participant_id);
                Change::Participants
            }
            ServerMessage::PresenceChanged { participant_id, presence } => {
                if let Some(p) = self.participants.iter_mut().find(|p| &p.participant_id == participant_id) {
                    p.presence = *presence;
                }
                Change::Participants
            }
            ServerMessage::MediaSelected { media, .. } => {
                self.media = Some(media.clone());
                Change::Media
            }
            ServerMessage::RoomUpdated { host_id, control_mode, chat_enabled, .. } => {
                let was_host = self.is_host();
                if let Some(h) = host_id {
                    self.host_id = h.clone();
                    self.mark_host();
                }
                if let Some(m) = control_mode {
                    self.control_mode = *m;
                }
                if let Some(c) = chat_enabled {
                    self.chat_enabled = *c;
                }
                let now_host = self.is_host();
                if was_host != now_host {
                    Change::Host { became_host: now_host, lost_host: was_host }
                } else if host_id.is_some() {
                    Change::Host { became_host: false, lost_host: false }
                } else {
                    Change::Settings
                }
            }
            ServerMessage::RoomClosed { reason } => {
                self.closed_reason = Some(reason.clone());
                Change::Closed
            }
            ServerMessage::Chat(m) => {
                self.push_chat(m.clone());
                Change::Chat
            }
            ServerMessage::ChatHistory(h) => {
                self.chat = h.iter().rev().take(MAX_CHAT).rev().cloned().collect();
                Change::Chat
            }
            _ => Change::None,
        }
    }

    pub fn set_playback(&mut self, snap: &PlaybackSnapshot) {
        self.playback = Some(PlaybackView::from(snap));
    }

    fn push_chat(&mut self, m: ChatMessage) {
        if self.chat.last().is_some_and(|l| l.id >= m.id) {
            return;
        }
        self.chat.push(m);
        if self.chat.len() > MAX_CHAT {
            self.chat.remove(0);
        }
    }

    /// Everyone but us is disconnected and the host is gone: nothing to follow.
    pub fn presence_of(&self, id: &str) -> Option<Presence> {
        self.participants.iter().find(|p| p.participant_id == id).map(|p| p.presence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::parse_server_message;

    fn msg(kind: &str, payload: &str) -> ServerMessage {
        parse_server_message(&format!(r#"{{"protocol_version":1,"type":"{kind}","payload":{payload}}}"#)).unwrap()
    }

    fn joined(id: &str, host: bool) -> ServerMessage {
        msg(
            "participant_joined",
            &format!(r#"{{"participant":{{"participant_id":"{id}","display_name":"{id}","presence":"connected","is_host":{host}}}}}"#),
        )
    }

    fn room_state(you: &str) -> ServerMessage {
        msg(
            "room_state",
            &format!(
                r#"{{"room":{{"room_id":"R","state":"waiting","host_id":"alice","control_mode":"everyone","chat_enabled":true,
            "participants":[{{"participant_id":"alice","display_name":"Alice","presence":"connected","is_host":true}}],
            "media":null,"playback":{{"state":"paused","position":0,"rate":1.0,"server_time":1,"sequence":0}}}},"you":"{you}","server_time":1}}"#
            ),
        )
    }

    #[test]
    fn join_gives_one_canonical_state() {
        let mut s = RoomState::new("R", "R", "");
        assert_eq!(s.apply(&room_state("bob")), Change::Snapshot);
        assert_eq!(s.you, "bob");
        assert!(!s.is_host());
        assert!(s.participants[0].is_host);
        assert!(s.chat_enabled);
    }

    #[test]
    fn participants_come_and_go() {
        let mut s = RoomState::default();
        s.apply(&room_state("bob"));
        s.apply(&joined("bob", false));
        s.apply(&joined("bob", false)); // idempotent
        assert_eq!(s.participants.len(), 2);
        s.apply(&msg("presence_changed", r#"{"participant_id":"bob","presence":"reconnecting"}"#));
        assert_eq!(s.presence_of("bob"), Some(Presence::Reconnecting));
        s.apply(&msg("participant_left", r#"{"participant_id":"bob","reason":"left"}"#));
        assert_eq!(s.participants.len(), 1);
    }

    #[test]
    fn host_transfer_updates_permissions_immediately() {
        let mut s = RoomState::default();
        s.apply(&room_state("bob"));
        s.apply(&joined("bob", false));
        let c = s.apply(&msg(
            "room_updated",
            r#"{"host_id":"bob","control_mode":"host_only","chat_enabled":true,"state":"waiting","reason":"host_changed"}"#,
        ));
        assert_eq!(c, Change::Host { became_host: true, lost_host: false });
        assert!(s.is_host());
        assert!(s.participants.iter().find(|p| p.participant_id == "bob").unwrap().is_host);
        assert!(!s.participants.iter().find(|p| p.participant_id == "alice").unwrap().is_host);
        // And back: the former host loses controls.
        let c = s.apply(&msg("room_updated", r#"{"host_id":"alice","reason":"host_changed"}"#));
        assert_eq!(c, Change::Host { became_host: false, lost_host: true });
        assert!(!s.is_host());
    }

    #[test]
    fn control_mode_decides_whether_guests_may_control_playback() {
        let mut s = RoomState::default();
        s.apply(&room_state("bob"));
        assert!(s.can_control_playback());
        s.apply(&msg("room_updated", r#"{"control_mode":"host_only","reason":"settings_changed"}"#));
        assert!(!s.can_control_playback());
    }

    #[test]
    fn room_closure_is_recorded() {
        let mut s = RoomState::default();
        s.apply(&room_state("bob"));
        assert_eq!(s.apply(&msg("room_closed", r#"{"reason":"host_closed"}"#)), Change::Closed);
        assert_eq!(s.closed_reason.as_deref(), Some("host_closed"));
    }

    #[test]
    fn media_selection_replaces_the_media() {
        let mut s = RoomState::default();
        s.apply(&room_state("bob"));
        let m = msg(
            "media_selected",
            r#"{"media":{"provider":"plex","server_id":"s","media_id":"42","media_type":"episode","season_id":"7","episode_id":"42"},
            "playback":{"state":"paused","position":0,"rate":1.0,"server_time":1,"sequence":1},"by":"alice"}"#,
        );
        assert_eq!(s.apply(&m), Change::Media);
        assert_eq!(s.media.as_ref().unwrap().media_id, "42");
    }

    #[test]
    fn chat_is_bounded_and_deduplicated() {
        let mut s = RoomState::default();
        for id in 1..=(MAX_CHAT as u64 + 20) {
            s.apply(&msg("chat_message", &format!(r#"{{"id":{id},"sender_id":"a","sender_name":"A","text":"hi","timestamp":1}}"#)));
        }
        assert_eq!(s.chat.len(), MAX_CHAT);
        let before = s.chat.len();
        s.apply(&msg("chat_message", r#"{"id":5,"sender_id":"a","sender_name":"A","text":"old","timestamp":1}"#));
        assert_eq!(s.chat.len(), before);
    }
}
