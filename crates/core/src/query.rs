use serde::{Deserialize, Serialize};

use crate::ids::ItemRef;
use crate::media::{ItemKind, MediaItem};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum SortBy {
    #[default]
    Title,
    DateAdded,
    ReleaseDate,
    Rating,
    LastPlayed,
    Random,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum SortOrder {
    #[default]
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct ItemFilter {
    pub genres: Vec<String>,
    pub years: Vec<i32>,
    pub person: Option<ItemRef>,
    pub unplayed_only: bool,
    pub favorites_only: bool,
}

/// Titles of one kind in one genre, across every library of a server. Paged
/// per server: each server answers `start`/`limit` on its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct GenreQuery {
    /// `Movie` or `Series`: they are browsed separately.
    pub kind: ItemKind,
    pub genre: String,
    #[serde(default)]
    pub sort: SortBy,
    #[serde(default)]
    pub order: SortOrder,
    #[serde(default)]
    pub start: u32,
    pub limit: u32,
}

/// Paged catalogue query. `parent` is a library, collection, playlist, etc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ItemQuery {
    pub parent: Option<ItemRef>,
    pub kinds: Vec<ItemKind>,
    #[serde(default)]
    pub filter: ItemFilter,
    #[serde(default)]
    pub sort: SortBy,
    #[serde(default)]
    pub order: SortOrder,
    #[serde(default)]
    pub start: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub start: u32,
    pub total: Option<u32>,
}

impl<T> Page<T> {
    pub fn empty() -> Self {
        Self { items: Vec::new(), start: 0, total: Some(0) }
    }
}

/// Semantic home rows. Providers only return the rows they can back with real
/// data; the aggregator merges rows of the same kind across servers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "type")]
pub enum HomeRowKind {
    ContinueWatching,
    NextUp,
    RecentlyAdded { library: Option<ItemRef> },
    Recommended,
    Popular,
    Collections,
    Playlists,
    LiveTvOnNow,
    /// Provider-curated hub without a common equivalent (Plex hubs).
    Custom { key: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct HomeRow {
    pub kind: HomeRowKind,
    pub title: String,
    pub items: Vec<MediaItem>,
}
