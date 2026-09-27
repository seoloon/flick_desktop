//! Mapping between provider stream indexes and mpv track ids.
//!
//! Providers identify streams by container index (ffprobe order); mpv numbers
//! tracks per type from 1 and exposes the container index as `ff-index`.
//! External subtitles are matched by the URL they were added with.

use oneshot_mpv::Node;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum TrackType {
    Video,
    Audio,
    Sub,
}

impl TrackType {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "video" => Some(Self::Video),
            "audio" => Some(Self::Audio),
            "sub" => Some(Self::Sub),
            _ => None,
        }
    }

    /// mpv property selecting a track of this type.
    pub fn property(self) -> &'static str {
        match self {
            Self::Video => "vid",
            Self::Audio => "aid",
            Self::Sub => "sid",
        }
    }
}

/// A track as mpv sees it, enriched for the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub kind: TrackType,
    pub mpv_id: i64,
    /// Container stream index (provider numbering), if embedded.
    pub stream_index: Option<u32>,
    pub external_url: Option<String>,
    pub title: Option<String>,
    pub language: Option<String>,
    pub codec: Option<String>,
    pub channels: Option<i64>,
    pub default: bool,
    pub forced: bool,
    pub selected: bool,
    pub width: Option<i64>,
    pub height: Option<i64>,
}

pub fn parse_track_list(node: &Node) -> Vec<Track> {
    node.as_array()
        .unwrap_or_default()
        .iter()
        .filter_map(|t| {
            let kind = TrackType::parse(t.get("type")?.as_str()?)?;
            let external = t.get("external").and_then(Node::as_bool).unwrap_or(false);
            let flag = |k: &str| t.get(k).and_then(Node::as_bool).unwrap_or(false);
            let text = |k: &str| t.get(k).and_then(Node::as_str).map(str::to_owned);
            Some(Track {
                kind,
                mpv_id: t.get("id")?.as_i64()?,
                stream_index: if external { None } else { t.get("ff-index").and_then(Node::as_i64).map(|i| i as u32) },
                external_url: if external { text("external-filename") } else { None },
                title: text("title"),
                language: text("lang"),
                codec: text("codec"),
                channels: t.get("demux-channel-count").and_then(Node::as_i64),
                default: flag("default"),
                forced: flag("forced"),
                selected: flag("selected"),
                width: t.get("demux-w").and_then(Node::as_i64),
                height: t.get("demux-h").and_then(Node::as_i64),
            })
        })
        .collect()
}

/// Finds the mpv id for a provider stream index (embedded) or external URL.
pub fn mpv_id(tracks: &[Track], kind: TrackType, stream_index: Option<u32>, external_url: Option<&str>) -> Option<i64> {
    tracks
        .iter()
        .filter(|t| t.kind == kind)
        .find(|t| match (external_url, stream_index) {
            (Some(url), _) => t.external_url.as_deref() == Some(url),
            (None, Some(i)) => t.stream_index == Some(i),
            (None, None) => false,
        })
        .map(|t| t.mpv_id)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn track(kind: &str, id: i64, ff: Option<i64>, ext: Option<&str>) -> Node {
        let mut m = BTreeMap::from([
            ("type".to_owned(), Node::from(kind)),
            ("id".to_owned(), Node::Int64(id)),
            ("external".to_owned(), Node::Flag(ext.is_some())),
        ]);
        if let Some(ff) = ff {
            m.insert("ff-index".into(), Node::Int64(ff));
        }
        if let Some(e) = ext {
            m.insert("external-filename".into(), Node::from(e));
        }
        Node::Map(m)
    }

    #[test]
    fn maps_container_indexes_and_external_urls() {
        let list = Node::Array(vec![
            track("video", 1, Some(0), None),
            track("audio", 1, Some(1), None),
            track("audio", 2, Some(2), None),
            track("sub", 1, Some(4), None),
            track("sub", 2, None, Some("http://srv/sub.srt")),
        ]);
        let tracks = parse_track_list(&list);
        assert_eq!(mpv_id(&tracks, TrackType::Audio, Some(2), None), Some(2));
        assert_eq!(mpv_id(&tracks, TrackType::Sub, Some(4), None), Some(1));
        assert_eq!(mpv_id(&tracks, TrackType::Sub, None, Some("http://srv/sub.srt")), Some(2));
        assert_eq!(mpv_id(&tracks, TrackType::Audio, Some(9), None), None);
    }
}
