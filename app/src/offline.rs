//! The downloaded titles as a library of their own.
//!
//! A download is opened like any title of a Jellyfin or Plex server: this is a
//! [`MediaProvider`] whose items are the finished downloads, so the detail page,
//! the player and its episode panel all work unchanged. Each download keeps a
//! snapshot of the title's metadata and artwork taken when it was queued
//! (`meta/<id>.json`, `meta/<id>.poster`...), so it looks the same without the server.
//!
//! Its items are `LOCAL_SERVER:<download id>`. The provider is registered in the
//! catalogue apart from the servers: it is browsed only in offline mode.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use oneshot_core::ids::{ItemRef, ServerId};
use oneshot_core::media::{ImageKind, ImageRef, ImageSet, ImageSize, ItemKind, Marker, MediaItem};
use oneshot_core::playback::{ClientProfile, PlaybackInfo, PlaybackReport, ServerPolicy, SourceOffer, StreamRequest, StreamTarget};
use oneshot_core::provider::{Adjacent, MediaProvider};
use oneshot_core::query::{HomeRow, HomeRowKind, ItemQuery, Page};
use oneshot_core::server::{Library, LibraryKind, ProviderKind, ServerDescriptor, ServerStatus, UserProfile};
use oneshot_core::{Error, Result};
use oneshot_flickdd::{Item, Manager, State};
use url::Url;
use uuid::Uuid;

/// Id of the library of downloads (fixed: items of it are kept in history and bookmarks).
pub const LOCAL_SERVER: ServerId = ServerId(Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_f11c));

pub const IMAGE_KINDS: [(ImageKind, &str, ImageSize); 4] = [
    (ImageKind::Poster, "poster", ImageSize::Large),
    (ImageKind::Backdrop, "backdrop", ImageSize::Hero),
    (ImageKind::Thumb, "thumb", ImageSize::Large),
    (ImageKind::Logo, "logo", ImageSize::Large),
];

pub fn kind_name(kind: ImageKind) -> &'static str {
    IMAGE_KINDS.iter().find(|(k, _, _)| *k == kind).map_or("poster", |(_, n, _)| n)
}

/// Where the snapshots live.
pub fn meta_dir(downloads: &Path) -> PathBuf {
    downloads.join("meta")
}

pub fn snapshot_path(downloads: &Path, id: &str) -> PathBuf {
    meta_dir(downloads).join(format!("{id}.json"))
}

pub fn image_path(downloads: &Path, id: &str, kind: ImageKind) -> PathBuf {
    meta_dir(downloads).join(format!("{id}.{}", kind_name(kind)))
}

/// Download ids are generated here: nothing else may reach the file system through an image URL.
fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

#[derive(Clone)]
pub struct LocalLibrary {
    manager: Arc<Manager>,
    dir: PathBuf,
    descriptor: ServerDescriptor,
}

impl std::fmt::Debug for LocalLibrary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalLibrary").finish_non_exhaustive()
    }
}

/// A finished download with its title.
struct Entry {
    download: Item,
    media: MediaItem,
}

impl LocalLibrary {
    pub fn new(manager: Arc<Manager>, dir: PathBuf) -> Self {
        let file = Url::parse("file:///").expect("static url");
        let descriptor = ServerDescriptor {
            id: LOCAL_SERVER,
            // There is no local kind: this only labels where an id comes from.
            kind: ProviderKind::Jellyfin,
            name: "Local".into(),
            remote_id: "local".into(),
            base_url: file,
            alternate_urls: Vec::new(),
            version: None,
            user: UserProfile { id: "local".into(), name: "Downloads".into(), avatar: None, is_admin: false },
            disabled: false,
            home_member: false,
        };
        Self { manager, dir, descriptor }
    }

    fn me(id: &str) -> ItemRef {
        ItemRef::new(LOCAL_SERVER, id)
    }

    /// The title as the library shows it: its own ids, its own artwork, nothing that needs a server.
    fn localize(&self, mut m: MediaItem, id: &str) -> MediaItem {
        let me = Self::me(id);
        m.id = me.clone();
        m.credits.clear();
        m.alternates.clear();
        m.child_count = None;
        if let Some(e) = &mut m.episode {
            e.series = None;
            e.season = None;
        }
        let image = |kind: ImageKind| {
            image_path(&self.dir, id, kind).is_file().then(|| ImageRef { item: me.clone(), kind, tag: kind_name(kind).into(), blurhash: None })
        };
        m.images = ImageSet {
            poster: image(ImageKind::Poster),
            backdrop: image(ImageKind::Backdrop),
            thumb: image(ImageKind::Thumb),
            logo: image(ImageKind::Logo),
            banner: None,
        };
        m
    }

    fn entry(&self, id: &str) -> Option<Entry> {
        if !safe_id(id) {
            return None;
        }
        let download = self.manager.item(id).filter(|d| d.state == State::Done && d.final_path.as_deref().is_some_and(Path::is_file))?;
        let raw = std::fs::read(snapshot_path(&self.dir, id)).ok()?;
        let media: MediaItem = serde_json::from_slice(&raw).ok()?;
        Some(Entry { media: self.localize(media, id), download })
    }

    /// Every title available offline, movies by title then series by episode.
    fn entries(&self) -> Vec<Entry> {
        let mut out: Vec<Entry> = self.manager.list().iter().filter(|d| d.state == State::Done && !d.missing).filter_map(|d| self.entry(&d.id)).collect();
        out.sort_by_key(|e| order_key(&e.media));
        out
    }
}

/// Series first by name then by season and episode; movies by title.
fn order_key(m: &MediaItem) -> (u8, String, u32, u32) {
    match (&m.kind, &m.episode) {
        (ItemKind::Episode, Some(e)) => (1, e.series_title.clone().unwrap_or_default().to_lowercase(), e.season_number.unwrap_or(0), e.episode_number.unwrap_or(0)),
        _ => (0, m.title.to_lowercase(), 0, 0),
    }
}

fn series_of(m: &MediaItem) -> Option<String> {
    m.episode.as_ref().and_then(|e| e.series_title.clone())
}

#[async_trait]
impl MediaProvider for LocalLibrary {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Jellyfin
    }

    fn descriptor(&self) -> &ServerDescriptor {
        &self.descriptor
    }

    async fn status(&self) -> ServerStatus {
        ServerStatus::Online { latency_ms: 0, url: self.descriptor.base_url.clone() }
    }

    async fn libraries(&self) -> Result<Vec<Library>> {
        let count = self.entries().len() as u32;
        if count == 0 {
            return Ok(Vec::new());
        }
        Ok(vec![Library { id: Self::me("all"), name: "Downloads".into(), kind: LibraryKind::Mixed, item_count: Some(count), image: None }])
    }

    async fn home(&self) -> Result<Vec<HomeRow>> {
        let entries = self.entries();
        let mut rows: Vec<HomeRow> = Vec::new();
        let movies: Vec<MediaItem> = entries.iter().filter(|e| e.media.kind != ItemKind::Episode).map(|e| e.media.clone()).collect();
        if !movies.is_empty() {
            rows.push(HomeRow { kind: HomeRowKind::Custom { key: "downloads-movies".into() }, title: "Downloaded movies".into(), items: movies });
        }
        // One row per series, episodes in order.
        let mut names: Vec<String> = Vec::new();
        for e in entries.iter().filter(|e| e.media.kind == ItemKind::Episode) {
            let name = series_of(&e.media).unwrap_or_else(|| "Episodes".into());
            if !names.contains(&name) {
                names.push(name);
            }
        }
        for name in names {
            let items: Vec<MediaItem> = entries
                .iter()
                .filter(|e| e.media.kind == ItemKind::Episode && series_of(&e.media).unwrap_or_else(|| "Episodes".into()) == name)
                .map(|e| e.media.clone())
                .collect();
            rows.push(HomeRow { kind: HomeRowKind::Custom { key: format!("downloads-series-{name}") }, title: name, items });
        }
        Ok(rows)
    }

    async fn items(&self, query: &ItemQuery) -> Result<Page<MediaItem>> {
        let all: Vec<MediaItem> = self.entries().into_iter().map(|e| e.media).filter(|m| query.kinds.is_empty() || query.kinds.contains(&m.kind)).collect();
        let total = all.len() as u32;
        let items = all.into_iter().skip(query.start as usize).take(query.limit as usize).collect();
        Ok(Page { items, start: query.start, total: Some(total) })
    }

    async fn item(&self, id: &ItemRef) -> Result<MediaItem> {
        self.entry(&id.key).map(|e| e.media).ok_or_else(|| Error::NotFound("This download is no longer on this computer.".into()))
    }

    async fn children(&self, _id: &ItemRef, _kind: ItemKind) -> Result<Vec<MediaItem>> {
        Ok(Vec::new())
    }

    async fn search(&self, term: &str, limit: u32) -> Result<Vec<MediaItem>> {
        let needle = term.trim().to_lowercase();
        Ok(self
            .entries()
            .into_iter()
            .map(|e| e.media)
            .filter(|m| m.title.to_lowercase().contains(&needle) || series_of(m).is_some_and(|s| s.to_lowercase().contains(&needle)))
            .take(limit as usize)
            .collect())
    }

    async fn similar(&self, _id: &ItemRef, _limit: u32) -> Result<Vec<MediaItem>> {
        Ok(Vec::new())
    }

    /// The downloaded episodes just before and after this one, in the same series.
    async fn adjacent_episodes(&self, id: &ItemRef) -> Result<Adjacent> {
        let entries = self.entries();
        let Some(pos) = entries.iter().position(|e| e.media.id == *id) else { return Ok(Adjacent::default()) };
        let here = &entries[pos].media;
        let same_series = |m: &MediaItem| m.kind == ItemKind::Episode && series_of(m) == series_of(here);
        if here.kind != ItemKind::Episode {
            return Ok(Adjacent::default());
        }
        let previous = entries[..pos].iter().rev().map(|e| &e.media).find(|m| same_series(m)).cloned();
        let next = entries[pos + 1..].iter().map(|e| &e.media).find(|m| same_series(m)).cloned();
        Ok(Adjacent { previous, next })
    }

    async fn markers(&self, _id: &ItemRef) -> Result<Vec<Marker>> {
        Ok(Vec::new())
    }

    async fn set_played(&self, _id: &ItemRef, _played: bool) -> Result<()> {
        Err(Error::Unsupported("marking a download as watched".into()))
    }

    async fn set_favorite(&self, _id: &ItemRef, _favorite: bool) -> Result<()> {
        Err(Error::Unsupported("favourites of a download".into()))
    }

    async fn playback_info(&self, id: &ItemRef, _profile: &ClientProfile) -> Result<PlaybackInfo> {
        let entry = self.entry(&id.key).ok_or_else(|| Error::NotFound("This download is no longer on this computer.".into()))?;
        let source = entry.media.sources.first().cloned().ok_or_else(|| Error::Playback("this download has no technical details".into()))?;
        // The file is here, untouched: only direct play makes sense.
        let policy = ServerPolicy { direct_play_allowed: true, direct_stream_allowed: false, transcode_allowed: false, server_reasons: Vec::new() };
        Ok(PlaybackInfo { item: id.clone(), offers: vec![SourceOffer { source, policy }], play_session_id: None })
    }

    async fn stream(&self, request: &StreamRequest) -> Result<StreamTarget> {
        let entry = self.entry(&request.item.key).ok_or_else(|| Error::NotFound("This download is no longer on this computer.".into()))?;
        let path = entry.download.final_path.ok_or_else(|| Error::NotFound("file".into()))?;
        let url = Url::from_file_path(&path).map_err(|()| Error::Playback("the file path is not usable".into()))?;
        Ok(StreamTarget { url, headers: Vec::new(), external_subtitles: Vec::new() })
    }

    /// Nothing to tell: there is no server. (Resume points of downloads are not kept.)
    async fn report(&self, _report: &PlaybackReport) -> Result<()> {
        Ok(())
    }

    fn image_url(&self, _image: &ImageRef, _size: ImageSize) -> Result<Url> {
        Err(Error::Unsupported("remote artwork of a download".into()))
    }

    fn auth_headers(&self) -> Vec<(String, String)> {
        Vec::new()
    }
}

/// The stored picture of a download.
pub async fn local_image(dir: &Path, id: &str, kind: ImageKind) -> Result<Vec<u8>> {
    if !safe_id(id) {
        return Err(Error::Invalid("download id".into()));
    }
    tokio::fs::read(image_path(dir, id, kind)).await.map_err(|_| Error::NotFound("artwork".into()))
}

#[cfg(test)]
mod tests {
    use oneshot_core::media::EpisodeInfo;

    use super::*;

    fn episode(series: &str, season: u32, n: u32) -> MediaItem {
        let mut m = MediaItem::new(ItemRef::new(LOCAL_SERVER, "x"), ItemKind::Episode, "t");
        m.episode = Some(EpisodeInfo { series_title: Some(series.into()), season_number: Some(season), episode_number: Some(n), ..EpisodeInfo::default() });
        m
    }

    #[test]
    fn download_ids_cannot_reach_other_files() {
        assert!(safe_id("a1b2c3d4e5f6"));
        assert!(!safe_id(""));
        assert!(!safe_id("../secret"));
        assert!(!safe_id("a/b"));
        assert!(!safe_id("a.b"));
    }

    #[test]
    fn the_library_lists_movies_then_series_in_order() {
        let movie = MediaItem::new(ItemRef::new(LOCAL_SERVER, "m"), ItemKind::Movie, "Zulu");
        let mut items = [episode("Show", 1, 10), episode("Show", 2, 1), episode("Show", 1, 2), movie.clone()];
        items.sort_by_key(order_key);
        assert_eq!(items[0].id, movie.id, "movies come first");
        let order: Vec<(u32, u32)> = items[1..].iter().map(|m| m.episode.as_ref().map(|e| (e.season_number.unwrap(), e.episode_number.unwrap())).unwrap()).collect();
        assert_eq!(order, vec![(1, 2), (1, 10), (2, 1)]);
    }
}
