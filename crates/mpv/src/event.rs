use serde::Serialize;

use crate::node::Node;

/// Why a file stopped playing (`end-file` event).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    Eof,
    Stop,
    Quit,
    Error,
    Redirect,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Shutdown,
    Log { prefix: String, level: String, text: String },
    StartFile,
    FileLoaded,
    EndFile { reason: EndReason, error: Option<String> },
    VideoReconfig,
    AudioReconfig,
    Seek,
    PlaybackRestart,
    PropertyChange { id: u64, name: String, value: Node },
    CommandReply { id: u64, error: Option<String>, result: Node },
    SetPropertyReply { id: u64, error: Option<String> },
    QueueOverflow,
    Other { name: String },
}

impl Event {
    /// Decodes the map produced by `mpv_event_to_node`.
    pub(crate) fn from_node(reply_id: u64, node: Node) -> Self {
        let s = |k: &str| node.get(k).and_then(Node::as_str).unwrap_or_default().to_owned();
        let error = node.get("error").and_then(Node::as_str).map(str::to_owned);
        match node.get("event").and_then(Node::as_str).unwrap_or_default() {
            "shutdown" => Self::Shutdown,
            "log-message" => Self::Log { prefix: s("prefix"), level: s("level"), text: s("text") },
            "start-file" => Self::StartFile,
            "file-loaded" => Self::FileLoaded,
            "end-file" => Self::EndFile {
                reason: match node.get("reason").and_then(Node::as_str) {
                    Some("eof") => EndReason::Eof,
                    Some("stop") => EndReason::Stop,
                    Some("quit") => EndReason::Quit,
                    Some("error") => EndReason::Error,
                    Some("redirect") => EndReason::Redirect,
                    _ => EndReason::Unknown,
                },
                error: node.get("file_error").and_then(Node::as_str).map(str::to_owned),
            },
            "video-reconfig" => Self::VideoReconfig,
            "audio-reconfig" => Self::AudioReconfig,
            "seek" => Self::Seek,
            "playback-restart" => Self::PlaybackRestart,
            "property-change" => Self::PropertyChange {
                id: reply_id,
                name: s("name"),
                value: node.get("data").cloned().unwrap_or(Node::None),
            },
            "command-reply" => Self::CommandReply {
                id: reply_id,
                error,
                result: node.get("result").cloned().unwrap_or(Node::None),
            },
            "set-property-reply" => Self::SetPropertyReply { id: reply_id, error },
            "queue-overflow" => Self::QueueOverflow,
            other => Self::Other { name: other.to_owned() },
        }
    }
}
