//! The contract every media server backend implements.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::Result;
use crate::ids::ItemRef;
use crate::media::{ImageRef, ImageSize, ItemKind, Marker, MediaItem};
use crate::playback::{ClientProfile, PlaybackInfo, PlaybackReport, StreamRequest, StreamTarget};
use crate::query::{HomeRow, ItemFilter, ItemQuery, Page, SortBy, SortOrder};
use crate::server::{Library, ProviderKind, ServerDescriptor, ServerStatus};

/// Previous/next items around an episode, for autoplay and player buttons.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Adjacent {
    pub previous: Option<MediaItem>,
    pub next: Option<MediaItem>,
}

/// Catalogue + playback operations of one server connection.
///
/// Implementations must map *their* semantics onto the common model and must
/// not fake features: return [`crate::Error::Unsupported`] instead.
#[async_trait]
pub trait MediaProvider: Send + Sync + std::fmt::Debug {
    fn kind(&self) -> ProviderKind;
    fn descriptor(&self) -> &ServerDescriptor;

    async fn status(&self) -> ServerStatus;
    async fn libraries(&self) -> Result<Vec<Library>>;
    async fn home(&self) -> Result<Vec<HomeRow>>;
    async fn items(&self, query: &ItemQuery) -> Result<Page<MediaItem>>;
    /// Full detail, including technical sources.
    async fn item(&self, id: &ItemRef) -> Result<MediaItem>;
    /// Seasons of a series, episodes of a season, items of a collection or
    /// playlist. `kind` is the parent's kind (ordering rules differ).
    async fn children(&self, id: &ItemRef, kind: ItemKind) -> Result<Vec<MediaItem>>;
    async fn search(&self, term: &str, limit: u32) -> Result<Vec<MediaItem>>;
    async fn similar(&self, id: &ItemRef, limit: u32) -> Result<Vec<MediaItem>>;
    async fn adjacent_episodes(&self, id: &ItemRef) -> Result<Adjacent>;
    async fn markers(&self, id: &ItemRef) -> Result<Vec<Marker>>;

    async fn set_played(&self, id: &ItemRef, played: bool) -> Result<()>;
    async fn set_favorite(&self, id: &ItemRef, favorite: bool) -> Result<()>;

    /// The signed-in user's favourites across the whole server, newest
    /// first. Default: the favourites filter of `items`, so a server whose
    /// `items` refuses that filter reports `Unsupported`.
    async fn favorites(&self, limit: u32) -> Result<Vec<MediaItem>> {
        let query = ItemQuery {
            parent: None,
            kinds: vec![ItemKind::Movie, ItemKind::Series, ItemKind::Episode, ItemKind::Collection],
            filter: ItemFilter { favorites_only: true, ..ItemFilter::default() },
            sort: SortBy::DateAdded,
            order: SortOrder::Descending,
            start: 0,
            limit,
        };
        Ok(self.items(&query).await?.items)
    }

    /// What this server knows about a person of its own.
    async fn person(&self, _id: &ItemRef) -> Result<crate::person::PersonInfo> {
        Err(crate::Error::Unsupported("person details".into()))
    }

    /// Movies and series featuring a person, found by `hint` when it is a
    /// person of this server, else by name.
    async fn person_items(&self, _name: &str, _hint: Option<&ItemRef>) -> Result<Vec<MediaItem>> {
        Err(crate::Error::Unsupported("person search".into()))
    }

    async fn playback_info(&self, id: &ItemRef, profile: &ClientProfile) -> Result<PlaybackInfo>;
    async fn stream(&self, request: &StreamRequest) -> Result<StreamTarget>;
    async fn report(&self, report: &PlaybackReport) -> Result<()>;

    /// Authenticated URL for an image. Internal: never sent to the WebView.
    fn image_url(&self, image: &ImageRef, size: ImageSize) -> Result<Url>;
    /// Headers to attach when fetching `image_url` (auth).
    fn auth_headers(&self) -> Vec<(String, String)>;

    /// Administration surface, if the signed-in user may administer the server.
    fn admin(&self) -> Option<&dyn AdminProvider> {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AdminSession {
    pub id: String,
    pub user: String,
    pub client: String,
    pub device: String,
    pub title: Option<String>,
    pub state: Option<String>,
    pub transcoding: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AdminTask {
    pub id: String,
    pub name: String,
    pub category: Option<String>,
    pub state: String,
    pub progress: Option<f32>,
    pub last_result: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AdminUser {
    pub id: String,
    pub name: String,
    pub is_admin: bool,
    pub is_disabled: bool,
    pub last_activity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AdminServerInfo {
    pub name: String,
    pub version: String,
    pub os: Option<String>,
    pub update_available: Option<bool>,
    pub transcode_hw_accel: Option<String>,
    /// Arbitrary provider-specific facts shown as a key/value list.
    pub extra: Vec<(String, String)>,
}

/// Admin features. Every call is subject to server-side authorization; a
/// 401/403 surfaces as [`crate::Error::Forbidden`], never worked around.
#[async_trait]
pub trait AdminProvider: Send + Sync {
    async fn server_info(&self) -> Result<AdminServerInfo>;
    async fn sessions(&self) -> Result<Vec<AdminSession>>;
    async fn users(&self) -> Result<Vec<AdminUser>>;
    async fn tasks(&self) -> Result<Vec<AdminTask>>;
    async fn run_task(&self, id: &str) -> Result<()>;
    async fn scan_library(&self, library: &ItemRef) -> Result<()>;
    async fn logs(&self) -> Result<Vec<String>>;
}
