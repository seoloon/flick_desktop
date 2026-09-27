//! People (actors, directors…): what a server knows, and the page the UI
//! shows (server data completed by TMDB).

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::ids::ItemRef;
use crate::media::{ExternalIds, ImageRef, ItemKind};

/// A person as a media server describes them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonInfo {
    pub id: ItemRef,
    pub name: String,
    pub overview: Option<String>,
    pub birth: Option<NaiveDate>,
    pub death: Option<NaiveDate>,
    pub birthplace: Option<String>,
    pub image: Option<ImageRef>,
    pub external_ids: ExternalIds,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum PersonPhoto {
    /// From the person's server (served by `oneshot-img`).
    Server { image: ImageRef },
    /// A TMDB file name (`/abc.jpg`), served by `oneshot-img` too.
    Tmdb { path: String },
}

/// A title of the person's TMDB filmography.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct KnownFor {
    /// `Movie` or `Series`.
    pub kind: ItemKind,
    pub title: String,
    pub year: Option<i32>,
    /// Character played, or job for crew.
    pub role: Option<String>,
    /// TMDB poster file (`/abc.jpg`).
    pub poster: Option<String>,
    pub tmdb_id: String,
    pub vote_count: u32,
}

/// Whether TMDB contributed to the page, and if not, why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum TmdbUse {
    Used,
    NoKey,
    NotFound,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PersonDetails {
    pub name: String,
    pub department: Option<String>,
    pub biography: Option<String>,
    pub birth: Option<NaiveDate>,
    pub death: Option<NaiveDate>,
    pub birthplace: Option<String>,
    pub photo: Option<PersonPhoto>,
    /// TMDB filmography, most voted first (the UI removes titles on the servers).
    pub known_for: Vec<KnownFor>,
    pub tmdb: TmdbUse,
}
