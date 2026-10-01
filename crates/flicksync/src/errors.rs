//! Errors and the user-facing messages they map to. Raw protocol/Rust errors
//! are never shown to the user: the UI receives a [`UserMessage`] code.

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("incompatible protocol version")]
    IncompatibleVersion,
    #[error("network: {0}")]
    Network(String),
    #[error("authentication failed")]
    Unauthenticated,
    #[error("FlickSync is not configured")]
    NotConfigured,
    #[error("not in a room")]
    NoRoom,
    #[error("server error {code}")]
    Server { code: String },
}

pub type Result<T> = std::result::Result<T, Error>;

/// Stable codes the UI maps to localized text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum UserMessage {
    OnlyHostCanChooseMedia,
    ControlDenied,
    RoomNotFound,
    RoomFull,
    RoomClosed,
    InvalidMedia,
    SessionExpired,
    SlowDown,
    ChatDisabled,
    IncompatibleVersion,
    Unavailable,
    MediaUnavailable,
    NotConfigured,
    Generic,
}

impl UserMessage {
    /// English fallback shown when the UI has no translation.
    pub fn text(self) -> &'static str {
        match self {
            Self::OnlyHostCanChooseMedia => "Only the host can choose what the room watches.",
            Self::ControlDenied => "Only the host can control playback in this room.",
            Self::RoomNotFound => "This room doesn't exist anymore.",
            Self::RoomFull => "This room is full.",
            Self::RoomClosed => "The room was closed.",
            Self::InvalidMedia => "The host picked something Flick can't share.",
            Self::SessionExpired => "Your session expired. Please try again.",
            Self::SlowDown => "You're doing that too fast.",
            Self::ChatDisabled => "Chat is turned off in this room.",
            Self::IncompatibleVersion => "Your Flick version is not compatible with this FlickSync server.",
            Self::Unavailable => "Watch together isn't reachable right now.",
            Self::MediaUnavailable => "This item isn't available on your connected server.",
            Self::NotConfigured => "Watch together isn't set up.",
            Self::Generic => "Something went wrong with the watch room.",
        }
    }
}

/// Maps a server error code (protocol §13) to what the user should read.
pub fn user_message_for_code(code: &str) -> UserMessage {
    match code {
        "NOT_HOST" => UserMessage::OnlyHostCanChooseMedia,
        "CONTROL_DENIED" => UserMessage::ControlDenied,
        "ROOM_NOT_FOUND" | "NOT_MEMBER" => UserMessage::RoomNotFound,
        "ROOM_FULL" => UserMessage::RoomFull,
        "ROOM_CLOSED" => UserMessage::RoomClosed,
        "INVALID_MEDIA" => UserMessage::InvalidMedia,
        "UNAUTHENTICATED" | "UNAUTHORIZED" | "AUTHENTICATION_FAILED" | "FORBIDDEN" => UserMessage::SessionExpired,
        "RATE_LIMITED" | "TOO_MANY_ROOMS" => UserMessage::SlowDown,
        "CHAT_DISABLED" => UserMessage::ChatDisabled,
        "UNSUPPORTED_VERSION" => UserMessage::IncompatibleVersion,
        "TOO_MANY_CONNECTIONS" | "INTERNAL" => UserMessage::Unavailable,
        _ => UserMessage::Generic,
    }
}

impl Error {
    pub fn user_message(&self) -> UserMessage {
        match self {
            Self::IncompatibleVersion => UserMessage::IncompatibleVersion,
            Self::Unauthenticated => UserMessage::SessionExpired,
            Self::Network(_) => UserMessage::Unavailable,
            Self::NotConfigured => UserMessage::NotConfigured,
            Self::Server { code } => user_message_for_code(code),
            Self::Protocol(_) | Self::NoRoom => UserMessage::Generic,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_codes_map_to_friendly_messages() {
        assert_eq!(user_message_for_code("NOT_HOST"), UserMessage::OnlyHostCanChooseMedia);
        assert_eq!(user_message_for_code("ROOM_FULL"), UserMessage::RoomFull);
        assert_eq!(user_message_for_code("SOMETHING_NEW"), UserMessage::Generic);
        assert!(!UserMessage::Generic.text().contains("INTERNAL"));
    }
}
