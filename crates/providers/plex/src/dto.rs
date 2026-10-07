//! Plex JSON payloads (requested with `Accept: application/json`). Plex wraps
//! everything in `MediaContainer`; fields are camelCase and mostly optional.

use serde::{Deserialize, Deserializer};

/// PMS serialises numeric ids as numbers in library responses and as strings
/// in decision responses.
fn id_from_any<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Id {
        N(i64),
        S(String),
    }
    match Id::deserialize(d)? {
        Id::N(n) => Ok(n),
        Id::S(s) => s.parse().map_err(serde::de::Error::custom),
    }
}

fn opt_id_from_any<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Id {
        N(i64),
        S(String),
    }
    match Option::<Id>::deserialize(d)? {
        None => Ok(None),
        Some(Id::N(n)) => Ok(Some(n)),
        Some(Id::S(s)) => Ok(s.parse().ok()),
    }
}

#[derive(Debug, Deserialize)]
pub struct Envelope<T> {
    #[serde(rename = "MediaContainer")]
    pub container: T,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    pub size: Option<u32>,
    pub total_size: Option<u32>,
    pub offset: Option<u32>,
    #[serde(rename = "Metadata", default)]
    pub metadata: Vec<Metadata>,
    #[serde(rename = "Directory", default)]
    pub directories: Vec<Directory>,
    #[serde(rename = "Hub", default)]
    pub hubs: Vec<Hub>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Directory {
    pub key: String,
    pub title: String,
    pub r#type: Option<String>,
    pub thumb: Option<String>,
    pub composite: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hub {
    pub hub_identifier: Option<String>,
    pub title: String,
    pub r#type: Option<String>,
    #[serde(rename = "Metadata", default)]
    pub metadata: Vec<Metadata>,
    /// Tag hubs (actors, genres…) list their entries as directories.
    #[serde(rename = "Directory", default)]
    pub directories: Vec<HubTag>,
}

/// An entry of a tag hub in search results (an actor).
#[derive(Debug, Clone, Deserialize)]
pub struct HubTag {
    pub tag: Option<String>,
    #[serde(default, deserialize_with = "opt_id_from_any")]
    pub id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tag {
    pub tag: String,
    pub id: Option<i64>,
    pub role: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Guid {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Marker {
    pub r#type: String,
    pub start_time_offset: u64,
    pub end_time_offset: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub rating_key: String,
    pub r#type: String,
    pub title: String,
    /// Plex catalogue id (`plex://movie/…`); matches Watchlist entries.
    pub guid: Option<String>,
    pub title_sort: Option<String>,
    pub original_title: Option<String>,
    pub summary: Option<String>,
    pub tagline: Option<String>,
    pub year: Option<i32>,
    pub originally_available_at: Option<String>,
    pub added_at: Option<i64>,
    pub duration: Option<u64>,
    pub content_rating: Option<String>,
    pub rating: Option<f32>,
    pub audience_rating: Option<f32>,
    pub studio: Option<String>,
    pub view_offset: Option<u64>,
    pub view_count: Option<u32>,
    pub last_viewed_at: Option<i64>,
    pub leaf_count: Option<u32>,
    pub viewed_leaf_count: Option<u32>,
    pub child_count: Option<u32>,
    pub index: Option<u32>,
    pub parent_index: Option<u32>,
    pub parent_rating_key: Option<String>,
    pub parent_title: Option<String>,
    pub grandparent_rating_key: Option<String>,
    pub grandparent_title: Option<String>,
    pub thumb: Option<String>,
    pub art: Option<String>,
    pub parent_thumb: Option<String>,
    pub grandparent_thumb: Option<String>,
    pub grandparent_art: Option<String>,
    #[serde(rename = "Genre", default)]
    pub genres: Vec<Tag>,
    #[serde(rename = "Country", default)]
    pub countries: Vec<Tag>,
    #[serde(rename = "Director", default)]
    pub directors: Vec<Tag>,
    #[serde(rename = "Writer", default)]
    pub writers: Vec<Tag>,
    #[serde(rename = "Role", default)]
    pub roles: Vec<Tag>,
    #[serde(rename = "Guid", default)]
    pub guids: Vec<Guid>,
    #[serde(rename = "Media", default)]
    pub media: Vec<Media>,
    #[serde(rename = "Marker", default)]
    pub markers: Vec<Marker>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    #[serde(deserialize_with = "id_from_any")]
    pub id: i64,
    pub duration: Option<u64>,
    pub bitrate: Option<u64>,
    pub container: Option<String>,
    pub title: Option<String>,
    #[serde(rename = "Part", default)]
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    #[serde(deserialize_with = "id_from_any")]
    pub id: i64,
    pub key: String,
    pub size: Option<u64>,
    pub container: Option<String>,
    pub duration: Option<u64>,
    #[serde(rename = "Stream", default)]
    pub streams: Vec<Stream>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
    #[serde(deserialize_with = "id_from_any")]
    pub id: i64,
    pub stream_type: u8,
    pub index: Option<u32>,
    pub codec: Option<String>,
    pub profile: Option<String>,
    pub level: Option<f32>,
    pub bitrate: Option<u64>,
    pub bit_depth: Option<u8>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<f32>,
    pub scan_type: Option<String>,
    pub color_trc: Option<String>,
    #[serde(rename = "DOVIPresent")]
    pub dovi_present: Option<bool>,
    #[serde(rename = "DOVIProfile")]
    pub dovi_profile: Option<u8>,
    #[serde(rename = "DOVIBLCompatID")]
    pub dovi_bl_compat_id: Option<u8>,
    #[serde(rename = "DOVIELPresent")]
    pub dovi_el_present: Option<bool>,
    pub channels: Option<u8>,
    pub audio_channel_layout: Option<String>,
    pub sampling_rate: Option<u32>,
    pub language_code: Option<String>,
    pub language_tag: Option<String>,
    pub title: Option<String>,
    pub display_title: Option<String>,
    pub extended_display_title: Option<String>,
    pub default: Option<bool>,
    pub forced: Option<bool>,
    pub hearing_impaired: Option<bool>,
    pub selected: Option<bool>,
    /// Present for sidecar subtitles: `/library/streams/{id}`.
    pub key: Option<String>,
}

// ---------------------------------------------------------------- plex.tv

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pin {
    pub id: i64,
    pub code: String,
    pub auth_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlexUser {
    pub id: i64,
    pub uuid: String,
    pub username: Option<String>,
    pub title: Option<String>,
    pub thumb: Option<String>,
}

/// `GET plex.tv/api/v2/home/users`.
#[derive(Debug, Deserialize)]
pub struct HomeUsers {
    #[serde(default)]
    pub users: Vec<HomeUser>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeUser {
    pub id: i64,
    pub uuid: String,
    pub title: Option<String>,
    pub username: Option<String>,
    pub thumb: Option<String>,
    #[serde(default)]
    pub admin: bool,
    #[serde(default)]
    pub protected: bool,
}

/// `POST plex.tv/api/v2/home/users/{uuid}/switch`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchedUser {
    pub auth_token: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub name: String,
    pub product_version: Option<String>,
    pub provides: String,
    pub client_identifier: String,
    pub access_token: Option<String>,
    #[serde(default)]
    pub owned: bool,
    #[serde(default)]
    pub connections: Vec<Connection>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub uri: String,
    #[serde(default)]
    pub local: bool,
    #[serde(default)]
    pub relay: bool,
    #[serde(rename = "IPv6", default)]
    pub ipv6: bool,
}

// ------------------------------------------------------------------ admin

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionsContainer {
    #[serde(rename = "Metadata", default)]
    pub metadata: Vec<SessionMetadata>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetadata {
    pub title: Option<String>,
    pub grandparent_title: Option<String>,
    #[serde(rename = "User")]
    pub user: Option<Tag>,
    #[serde(rename = "Player")]
    pub player: Option<Player>,
    #[serde(rename = "Session")]
    pub session: Option<SessionId>,
    #[serde(rename = "TranscodeSession")]
    pub transcode: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub product: Option<String>,
    pub title: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SessionId {
    pub id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ButlerContainer {
    #[serde(rename = "ButlerTasks")]
    pub tasks: Option<ButlerTasks>,
}

#[derive(Debug, Deserialize)]
pub struct ButlerTasks {
    #[serde(rename = "ButlerTask", default)]
    pub tasks: Vec<ButlerTask>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ButlerTask {
    pub name: String,
    pub title: Option<String>,
    pub enabled: Option<bool>,
}
