//! `MediaProvider` for one Plex Media Server connection.

use std::time::Instant;

use async_trait::async_trait;
use futures::StreamExt;
use oneshot_core::ids::ItemRef;
use oneshot_core::media::{ImageRef, ImageSize, ItemKind, Marker, MediaItem};
use oneshot_core::playback::{ClientProfile, PlaybackInfo, PlaybackReport, StreamRequest, StreamTarget};
use oneshot_core::provider::{AdminProvider, Adjacent, MediaProvider};
use oneshot_core::query::{HomeRow, HomeRowKind, ItemQuery, Page, SortBy, SortOrder};
use oneshot_core::server::{Library, LibraryKind, ProviderKind, ServerDescriptor, ServerStatus};
use oneshot_core::text::normalize_name;
use oneshot_core::{Error, Result};
use oneshot_net::reqwest::{Client, Method, RequestBuilder};
use serde::de::DeserializeOwned;
use url::Url;

use crate::auth::PlexIdentity;
use crate::dto::{Container, Envelope, Metadata};
use crate::map;
use crate::watchlist::Watchlist;

#[derive(Debug)]
pub struct PlexProvider {
    pub(crate) descriptor: ServerDescriptor,
    pub(crate) http: Client,
    pub(crate) identity: PlexIdentity,
    /// Server access token (from plex.tv resources), not the account token.
    pub(crate) token: String,
    /// Whether the signed-in account owns the server (admin surface).
    pub(crate) owned: bool,
    /// The user's plex.tv Watchlist, when a plex.tv sign-in is known.
    pub(crate) watchlist: Option<Watchlist>,
}

impl PlexProvider {
    pub fn new(descriptor: ServerDescriptor, http: Client, identity: PlexIdentity, token: String, owned: bool) -> Self {
        Self { descriptor, http, identity, token, owned, watchlist: None }
    }

    /// Favourites come from this user's plex.tv Watchlist.
    pub fn with_watchlist(mut self, watchlist: Watchlist) -> Self {
        self.watchlist = Some(watchlist);
        self
    }

    /// The actor tag id of `name` on this server (its search hubs).
    async fn actor_id(&self, name: &str) -> Result<Option<i64>> {
        let c = self.container("hubs/search", &[("query", name.to_owned()), ("limit", "10".into())]).await?;
        let key = normalize_name(name);
        let same = |n: &str| normalize_name(n) == key;
        Ok(c.hubs.iter().filter(|h| is_people_hub(h)).find_map(|h| {
            h.directories
                .iter()
                .find(|d| d.tag.as_deref().is_some_and(same))
                .and_then(|d| d.id)
                .or_else(|| h.metadata.iter().find(|m| same(&m.title)).and_then(|m| m.rating_key.parse().ok()))
        }))
    }

    /// This server's copy of a Plex catalogue title, if its libraries have it.
    async fn by_guid(&self, guid: &str) -> Result<Option<Metadata>> {
        Ok(self.container("library/all", &[("guid", guid.to_owned())]).await?.metadata.into_iter().next())
    }

    pub(crate) fn server(&self) -> oneshot_core::ServerId {
        self.descriptor.id
    }

    pub(crate) fn url(&self, path: &str) -> Result<Url> {
        oneshot_net::join(&self.descriptor.base_url, path)
    }

    pub(crate) fn request(&self, method: Method, path: &str, query: &[(&str, String)]) -> Result<RequestBuilder> {
        let mut url = self.url(path)?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        }
        Ok(self.identity.apply(self.http.request(method, url), Some(&self.token)))
    }

    pub(crate) async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T> {
        let started = Instant::now();
        let resp = self.request(Method::GET, path, query)?.send().await.map_err(oneshot_net::map_err)?;
        tracing::debug!(target: "provider", server = %self.descriptor.name, path, status = resp.status().as_u16(),
            ms = started.elapsed().as_millis() as u64, "plex GET");
        oneshot_net::json(resp).await
    }

    pub(crate) async fn container(&self, path: &str, query: &[(&str, String)]) -> Result<Container> {
        Ok(self.get::<Envelope<Container>>(path, query).await?.container)
    }

    pub(crate) async fn send_empty(&self, method: Method, path: &str, query: &[(&str, String)]) -> Result<()> {
        let resp = self.request(method, path, query)?.send().await.map_err(oneshot_net::map_err)?;
        oneshot_net::ensure_ok(resp).await.map(drop)
    }

    pub(crate) fn items(&self, m: &[Metadata]) -> Vec<MediaItem> {
        m.iter().map(|m| map::item(self.server(), m)).collect()
    }

    fn check(&self, id: &ItemRef) -> Result<()> {
        if id.server == self.server() {
            Ok(())
        } else {
            Err(Error::Invalid(format!("item {id} does not belong to {}", self.descriptor.name)))
        }
    }

    pub(crate) async fn metadata(&self, key: &str) -> Result<Metadata> {
        self.container(&format!("library/metadata/{key}"), &[("includeMarkers", "1".into()), ("includeGuids", "1".into())])
            .await?
            .metadata
            .into_iter()
            .next()
            .ok_or_else(|| Error::NotFound(format!("plex item {key}")))
    }
}

/// Library filters that take a person's tag id.
const PERSON_FILTERS: [&str; 3] = ["actor", "director", "writer"];
/// Filmography queries in flight at once on one server.
const PERSON_QUERY_CONCURRENCY: usize = 6;

/// A search hub listing people, however the server labels it.
fn is_people_hub(h: &crate::dto::Hub) -> bool {
    let people = |s: &str| matches!(s, "actor" | "director" | "writer" | "person" | "people");
    h.r#type.as_deref().is_some_and(people) || h.hub_identifier.as_deref().is_some_and(|id| id.split('.').any(people))
}

fn sort(s: SortBy, o: SortOrder) -> String {
    let field = match s {
        SortBy::Title => "titleSort",
        SortBy::DateAdded => "addedAt",
        SortBy::ReleaseDate => "originallyAvailableAt",
        SortBy::Rating => "audienceRating",
        SortBy::LastPlayed => "lastViewedAt",
        SortBy::Random => return "random".into(),
    };
    format!("{field}:{}", if o == SortOrder::Descending { "desc" } else { "asc" })
}

fn hub_kind(identifier: Option<&str>) -> HomeRowKind {
    let id = identifier.unwrap_or_default();
    if id.ends_with(".continue") || id == "continueWatching" {
        HomeRowKind::ContinueWatching
    } else if id.ends_with(".ondeck") {
        HomeRowKind::NextUp
    } else if id.ends_with(".recent") || id.contains("recentlyAdded") {
        HomeRowKind::RecentlyAdded { library: None }
    } else if id.contains("playlists") {
        HomeRowKind::Playlists
    } else {
        HomeRowKind::Custom { key: id.to_owned() }
    }
}

#[async_trait]
impl MediaProvider for PlexProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Plex
    }

    fn descriptor(&self) -> &ServerDescriptor {
        &self.descriptor
    }

    async fn status(&self) -> ServerStatus {
        let started = Instant::now();
        match self.container("identity", &[]).await {
            Ok(_) => ServerStatus::Online {
                latency_ms: started.elapsed().as_millis() as u32,
                url: self.descriptor.base_url.clone(),
            },
            Err(Error::Unauthorized) => ServerStatus::Unauthorized,
            Err(e) => ServerStatus::Unreachable { error: e.to_string() },
        }
    }

    async fn libraries(&self) -> Result<Vec<Library>> {
        let c = self.container("library/sections", &[]).await?;
        Ok(c.directories.iter().map(|d| map::library(self.server(), d)).collect())
    }

    async fn home(&self) -> Result<Vec<HomeRow>> {
        let c = self.container("hubs", &[("count", "20".into()), ("includeGuids", "1".into())]).await?;
        Ok(c.hubs
            .iter()
            .filter(|h| !h.metadata.is_empty())
            .map(|h| HomeRow { kind: hub_kind(h.hub_identifier.as_deref()), title: h.title.clone(), items: self.items(&h.metadata) })
            .collect())
    }

    async fn items(&self, query: &ItemQuery) -> Result<Page<MediaItem>> {
        let mut q: Vec<(&str, String)> = vec![
            ("sort", sort(query.sort, query.order)),
            ("X-Plex-Container-Start", query.start.to_string()),
            ("X-Plex-Container-Size", query.limit.to_string()),
            ("includeGuids", "1".into()),
        ];
        if let Some(t) = query.kinds.iter().find_map(|k| map::type_number(*k)) {
            q.push(("type", t.to_string()));
        }
        let f = &query.filter;
        if f.unplayed_only {
            q.push(("unwatched", "1".into()));
        }
        if !f.years.is_empty() {
            q.push(("year", f.years.iter().map(i32::to_string).collect::<Vec<_>>().join(",")));
        }
        if !f.genres.is_empty() {
            q.push(("genre", f.genres.join(",")));
        }
        if let Some(p) = &f.person {
            q.push(("actor", p.key.clone()));
        }
        if f.favorites_only {
            return Err(Error::Unsupported("favourites filter (Plex has no favourites)".into()));
        }
        let path = match &query.parent {
            Some(parent) => {
                self.check(parent)?;
                match parent.key.strip_prefix("section:") {
                    Some(section) => format!("library/sections/{section}/all"),
                    None => format!("library/metadata/{}/children", parent.key),
                }
            }
            None => "library/all".into(),
        };
        let c = self.container(&path, &q).await?;
        Ok(Page { items: self.items(&c.metadata), start: c.offset.unwrap_or(query.start), total: c.total_size.or(c.size) })
    }

    async fn item(&self, id: &ItemRef) -> Result<MediaItem> {
        self.check(id)?;
        let m = self.metadata(&id.key).await?;
        let mut item = map::item(self.server(), &m);
        // The heart on the detail page shows Watchlist membership; the
        // detail still loads when plex.tv does not answer.
        if let (Some(w), Some(guid)) = (&self.watchlist, &m.guid) {
            item.user.favorite = w.contains(guid).await.unwrap_or(false);
        }
        Ok(item)
    }

    async fn children(&self, id: &ItemRef, kind: ItemKind) -> Result<Vec<MediaItem>> {
        self.check(id)?;
        let path = match kind {
            ItemKind::Collection => format!("library/collections/{}/children", id.key),
            ItemKind::Playlist => format!("playlists/{}/items", id.key),
            _ => format!("library/metadata/{}/children", id.key),
        };
        Ok(self.items(&self.container(&path, &[("includeGuids", "1".into())]).await?.metadata))
    }

    async fn search(&self, term: &str, limit: u32) -> Result<Vec<MediaItem>> {
        let c = self.container("hubs/search", &[("query", term.to_owned()), ("limit", limit.to_string())]).await?;
        Ok(c.hubs
            .iter()
            .filter(|h| matches!(h.r#type.as_deref(), Some("movie" | "show" | "episode" | "collection")))
            .flat_map(|h| self.items(&h.metadata))
            .take(limit as usize)
            .collect())
    }

    async fn similar(&self, id: &ItemRef, limit: u32) -> Result<Vec<MediaItem>> {
        self.check(id)?;
        let c = self.container(&format!("library/metadata/{}/similar", id.key), &[("count", limit.to_string())]).await?;
        Ok(self.items(&c.metadata))
    }

    async fn adjacent_episodes(&self, id: &ItemRef) -> Result<Adjacent> {
        self.check(id)?;
        let m = self.metadata(&id.key).await?;
        let Some(show) = m.grandparent_rating_key else { return Ok(Adjacent::default()) };
        let leaves = self.container(&format!("library/metadata/{show}/allLeaves"), &[]).await?.metadata;
        let Some(pos) = leaves.iter().position(|l| l.rating_key == id.key) else { return Ok(Adjacent::default()) };
        let at = |i: Option<usize>| i.and_then(|i| leaves.get(i)).map(|m| map::item(self.server(), m));
        Ok(Adjacent { previous: at(pos.checked_sub(1)), next: at(Some(pos + 1)) })
    }

    async fn markers(&self, id: &ItemRef) -> Result<Vec<Marker>> {
        self.check(id)?;
        Ok(map::markers(&self.metadata(&id.key).await?))
    }

    async fn set_played(&self, id: &ItemRef, played: bool) -> Result<()> {
        self.check(id)?;
        let path = if played { ":/scrobble" } else { ":/unscrobble" };
        self.send_empty(Method::GET, path, &[("identifier", "com.plexapp.plugins.library".into()), ("key", id.key.clone())]).await
    }

    async fn set_favorite(&self, id: &ItemRef, favorite: bool) -> Result<()> {
        self.check(id)?;
        let w = self.watchlist.as_ref().ok_or_else(|| Error::Unsupported("favourites (sign in to plex.tv for this account)".into()))?;
        let guid = self.metadata(&id.key).await?.guid.ok_or_else(|| Error::Unsupported("favourites (this title is not in the Plex catalogue)".into()))?;
        w.set(&guid, favorite).await
    }

    async fn person_items(&self, name: &str, hint: Option<&ItemRef>) -> Result<Vec<MediaItem>> {
        let own = hint.filter(|h| h.server == self.server()).and_then(|h| h.key.parse::<i64>().ok());
        let actor = match own {
            Some(id) => Some(id),
            None => self.actor_id(name).await?,
        };
        let Some(actor) = actor else { return Ok(Vec::new()) };
        let sections: Vec<String> = self
            .libraries()
            .await?
            .into_iter()
            .filter(|l| matches!(l.kind, LibraryKind::Movies | LibraryKind::Shows))
            .filter_map(|l| l.id.key.strip_prefix("section:").map(str::to_owned))
            .collect();
        // Actors, directors and writers are separate tags: a person opened
        // from the crew is found under their own filter. The queries run a
        // few at a time; `buffered` keeps their order, so the result is the
        // same as asking one after the other.
        let mut queries = Vec::with_capacity(sections.len() * PERSON_FILTERS.len());
        for section in &sections {
            for filter in PERSON_FILTERS {
                let path = format!("library/sections/{section}/all");
                let query = [(filter, actor.to_string()), ("includeGuids", "1".to_owned())];
                queries.push(async move { self.container(&path, &query).await });
            }
        }
        let pages: Vec<Result<Container>> = futures::stream::iter(queries).buffered(PERSON_QUERY_CONCURRENCY).collect().await;
        let mut out: Vec<MediaItem> = Vec::new();
        for page in pages {
            for item in self.items(&page?.metadata) {
                if !out.iter().any(|o| o.id == item.id) {
                    out.push(item);
                }
            }
        }
        Ok(out)
    }

    /// Watchlist titles this server has, in Watchlist order (titles it
    /// does not have are left out).
    async fn favorites(&self, limit: u32) -> Result<Vec<MediaItem>> {
        let w = self.watchlist.as_ref().ok_or_else(|| Error::Unsupported("favourites (no plex.tv sign-in for this account)".into()))?;
        let entries = w.entries().await?;
        let guids: Vec<String> = entries.into_iter().take(limit as usize).map(|e| e.guid).collect();
        let found: Vec<Result<Option<Metadata>>> =
            futures::stream::iter(guids.into_iter().map(|guid| async move { self.by_guid(&guid).await })).buffered(8).collect().await;
        Ok(found
            .into_iter()
            .filter_map(|r| r.ok().flatten())
            .map(|m| {
                let mut item = map::item(self.server(), &m);
                item.user.favorite = true;
                item
            })
            .collect())
    }

    async fn playback_info(&self, id: &ItemRef, profile: &ClientProfile) -> Result<PlaybackInfo> {
        self.check(id)?;
        crate::playback::playback_info(self, id, profile).await
    }

    async fn stream(&self, request: &StreamRequest) -> Result<StreamTarget> {
        self.check(&request.item)?;
        crate::playback::stream(self, request).await
    }

    async fn report(&self, report: &PlaybackReport) -> Result<()> {
        crate::playback::report(self, report).await
    }

    fn image_url(&self, image: &ImageRef, size: ImageSize) -> Result<Url> {
        let Some(width) = size.max_width() else {
            // Original: absolute URLs (metadata agents) are used as-is.
            return Url::parse(&image.tag).or_else(|_| self.url(&image.tag));
        };
        let mut url = self.url("photo/:/transcode")?;
        url.query_pairs_mut()
            .append_pair("url", &image.tag)
            .append_pair("width", &width.to_string())
            .append_pair("height", &(width * 3 / 2).to_string())
            .append_pair("minSize", "1")
            .append_pair("upscale", "0");
        Ok(url)
    }

    fn auth_headers(&self) -> Vec<(String, String)> {
        self.identity.headers(&self.token)
    }

    fn admin(&self) -> Option<&dyn AdminProvider> {
        self.owned.then_some(self as &dyn AdminProvider)
    }
}
