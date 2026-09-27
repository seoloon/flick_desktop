//! Provider-agnostic catalogue entities. The UI only ever sees these types.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::ItemRef;
use crate::stream::MediaSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    Movie,
    Series,
    Season,
    Episode,
    Collection,
    Playlist,
    Person,
    Folder,
    MusicVideo,
    Video,
    TvChannel,
    Other,
}

impl ItemKind {
    pub fn is_playable(self) -> bool {
        matches!(self, Self::Movie | Self::Episode | Self::MusicVideo | Self::Video | Self::TvChannel)
    }
}

/// Artwork role. Providers map their own naming onto these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ImageKind {
    Poster,
    Backdrop,
    Thumb,
    Logo,
    Banner,
}

/// Opaque handle to a server-side image. The UI requests it through the
/// `oneshot-img://` protocol with a size hint; the Rust side resolves it to an
/// authenticated URL, so credentials never reach the WebView.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ImageRef {
    pub item: ItemRef,
    pub kind: ImageKind,
    /// Provider-specific locator (Jellyfin image tag, Plex relative path).
    pub tag: String,
    /// Optional blurhash (Jellyfin) for instant low-res placeholders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blurhash: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ImageSet {
    pub poster: Option<ImageRef>,
    pub backdrop: Option<ImageRef>,
    pub thumb: Option<ImageRef>,
    pub logo: Option<ImageRef>,
    pub banner: Option<ImageRef>,
}

/// Size buckets used everywhere artwork is requested: bounded set so the
/// on-disk cache has a high hit rate and the server can cache its resizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ImageSize {
    /// ~160 px wide: list thumbnails, blurred background source.
    Tiny,
    /// ~360 px: poster cards.
    Card,
    /// ~720 px: large cards, detail posters.
    Large,
    /// ~1920 px: hero backdrops.
    Hero,
    /// Original resolution (4K TVs in Flick Frame).
    Original,
}

impl ImageSize {
    pub fn max_width(self) -> Option<u32> {
        match self {
            Self::Tiny => Some(160),
            Self::Card => Some(360),
            Self::Large => Some(720),
            Self::Hero => Some(1920),
            Self::Original => None,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "tiny" => Self::Tiny,
            "card" => Self::Card,
            "large" => Self::Large,
            "hero" => Self::Hero,
            "original" => Self::Original,
            _ => return None,
        })
    }
}

/// Per-user state of an item, owned by the server.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct UserState {
    pub played: bool,
    pub play_count: u32,
    pub position_ms: u64,
    pub favorite: bool,
    pub last_played: Option<DateTime<Utc>>,
    /// Unplayed children (series/season).
    pub unplayed_count: Option<u32>,
}

impl UserState {
    /// Fraction watched in `[0, 1]`, if a runtime is known.
    pub fn progress(&self, runtime_ms: Option<u64>) -> Option<f32> {
        let rt = runtime_ms.filter(|r| *r > 0)?;
        (self.position_ms > 0).then(|| (self.position_ms as f32 / rt as f32).clamp(0.0, 1.0))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum PersonRole {
    Actor,
    Director,
    Writer,
    Producer,
    Composer,
    GuestStar,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Credit {
    pub person: ItemRef,
    pub name: String,
    pub role: PersonRole,
    /// Character name for actors, job title otherwise.
    pub character: Option<String>,
    pub image: Option<ImageRef>,
}

/// External database identifiers; used to merge the same title found on
/// several servers into one card.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ExternalIds {
    pub imdb: Option<String>,
    pub tmdb: Option<String>,
    pub tvdb: Option<String>,
}

impl ExternalIds {
    pub fn is_empty(&self) -> bool {
        self.imdb.is_none() && self.tmdb.is_none() && self.tvdb.is_none()
    }

    /// True when both sides share at least one identical external id.
    pub fn matches(&self, other: &Self) -> bool {
        fn eq(a: &Option<String>, b: &Option<String>) -> bool {
            matches!((a, b), (Some(x), Some(y)) if x.eq_ignore_ascii_case(y))
        }
        eq(&self.imdb, &other.imdb) || eq(&self.tmdb, &other.tmdb) || eq(&self.tvdb, &other.tvdb)
    }
}

/// Episode/season placement inside a series.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct EpisodeInfo {
    pub series: Option<ItemRef>,
    pub series_title: Option<String>,
    pub season: Option<ItemRef>,
    pub season_number: Option<u32>,
    pub episode_number: Option<u32>,
    pub episode_number_end: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    pub id: ItemRef,
    pub kind: ItemKind,
    pub title: String,
    pub sort_title: Option<String>,
    pub original_title: Option<String>,
    pub tagline: Option<String>,
    pub overview: Option<String>,
    pub year: Option<i32>,
    pub premiere_date: Option<NaiveDate>,
    pub runtime_ms: Option<u64>,
    pub official_rating: Option<String>,
    pub community_rating: Option<f32>,
    pub critic_rating: Option<f32>,
    pub genres: Vec<String>,
    pub studios: Vec<String>,
    pub credits: Vec<Credit>,
    pub images: ImageSet,
    pub user: UserState,
    pub episode: Option<EpisodeInfo>,
    pub external_ids: ExternalIds,
    pub child_count: Option<u32>,
    pub added_at: Option<DateTime<Utc>>,
    /// Technical sources; only populated on detail fetches.
    pub sources: Vec<MediaSource>,
    /// The same title on other servers (merged by external ids).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternates: Vec<ItemRef>,
}

impl MediaItem {
    /// Minimal constructor used by providers before filling optional fields.
    pub fn new(id: ItemRef, kind: ItemKind, title: impl Into<String>) -> Self {
        Self {
            id,
            kind,
            title: title.into(),
            sort_title: None,
            original_title: None,
            tagline: None,
            overview: None,
            year: None,
            premiere_date: None,
            runtime_ms: None,
            official_rating: None,
            community_rating: None,
            critic_rating: None,
            genres: Vec::new(),
            studios: Vec::new(),
            credits: Vec::new(),
            images: ImageSet::default(),
            user: UserState::default(),
            episode: None,
            external_ids: ExternalIds::default(),
            child_count: None,
            added_at: None,
            sources: Vec::new(),
            alternates: Vec::new(),
        }
    }
}

/// Intro/credits/recap markers used by "Skip intro".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Marker {
    pub kind: MarkerKind,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum MarkerKind {
    Intro,
    Credits,
    Recap,
    Preview,
    Commercial,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_clamped_and_optional() {
        let u = UserState { position_ms: 30_000, ..Default::default() };
        assert_eq!(u.progress(Some(60_000)), Some(0.5));
        assert_eq!(u.progress(None), None);
        assert_eq!(UserState::default().progress(Some(1)), None);
    }

    #[test]
    fn external_ids_match_case_insensitively() {
        let a = ExternalIds { imdb: Some("tt0133093".into()), ..Default::default() };
        let b = ExternalIds { imdb: Some("TT0133093".into()), tmdb: Some("603".into()), ..Default::default() };
        assert!(a.matches(&b));
        assert!(!a.matches(&ExternalIds::default()));
    }
}
