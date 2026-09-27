use serde::{Deserialize, Serialize};
use url::Url;

use crate::ids::{ItemRef, ServerId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    Jellyfin,
    Plex,
}

impl ProviderKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Jellyfin => "Jellyfin",
            Self::Plex => "Plex",
        }
    }
}

/// A configured server connection, as persisted (without secrets: tokens
/// live in the OS keychain, keyed by `id`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ServerDescriptor {
    pub id: ServerId,
    pub kind: ProviderKind,
    /// Display name reported by the server (user-renamable).
    pub name: String,
    /// Server's own unique id (Jellyfin `Id`, Plex `machineIdentifier`).
    pub remote_id: String,
    pub base_url: Url,
    /// Alternative addresses (Plex exposes local, remote and relay ones).
    pub alternate_urls: Vec<Url>,
    pub version: Option<String>,
    pub user: UserProfile,
    /// Turned off by the user: kept, with its token, but left out of the
    /// catalogue (home, libraries, search) until turned back on.
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub id: String,
    pub name: String,
    pub avatar: Option<Url>,
    /// Server-side administrator. Only used to *show* admin entry points; the
    /// server still enforces every permission.
    pub is_admin: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum LibraryKind {
    Movies,
    Shows,
    Music,
    Photos,
    MusicVideos,
    HomeVideos,
    Mixed,
    LiveTv,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub id: ItemRef,
    pub name: String,
    pub kind: LibraryKind,
    pub item_count: Option<u32>,
    pub image: Option<crate::media::ImageRef>,
}

/// Connection health, surfaced in the Servers screen and diagnostics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "state")]
pub enum ServerStatus {
    Online { latency_ms: u32, url: Url },
    Unauthorized,
    Unreachable { error: String },
}
