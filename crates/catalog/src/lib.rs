//! The only layer that knows there are several servers.
//!
//! * Registry of live providers keyed by local [`ServerId`].
//! * Fan-out with a per-server timeout: one slow/offline server never blocks
//!   the Home screen; its failure is reported as a [`ServerIssue`].
//! * Merging: rows of the same semantic kind are combined; the same title
//!   present on several servers is shown once, with `alternates`.
//! * Metadata cache (TTL) in front of every read; mutations invalidate.

mod merge;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use oneshot_core::ids::ItemRef;
use oneshot_core::media::{ItemKind, MediaItem};
use oneshot_core::person::PersonInfo;
use oneshot_core::provider::{Adjacent, MediaProvider};
use oneshot_core::query::{GenreQuery, HomeRow, ItemFilter, ItemQuery, Page, SortBy, SortOrder};
use oneshot_core::server::{Library, ServerDescriptor};
use oneshot_core::{Error, Result, ServerId};
use oneshot_storage::cache::MetadataCache;
use parking_lot::RwLock;
use serde::Serialize;

pub use merge::{dedupe, merge_genres, merge_rows, recent_seeds, recommendation_rows, sort_by_title};

const SERVER_TIMEOUT: Duration = Duration::from_secs(8);
/// Recently played titles read per server and kind, to find what to base recommendations on.
const HISTORY_WINDOW: u32 = 40;
/// Watched titles that each get a "Because you watched" row.
const RECOMMENDATION_SEEDS: usize = 3;
const SIMILAR_PER_SEED: u32 = 24;

/// A server without the feature has nothing to list: not an issue to report.
fn unsupported_as_empty<T>(r: Result<Vec<T>>) -> Result<Vec<T>> {
    match r {
        Err(Error::Unsupported(_)) => Ok(Vec::new()),
        other => other,
    }
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ServerIssue {
    pub server: ServerId,
    pub name: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Aggregated<T> {
    pub data: T,
    /// Servers that failed or timed out; the UI shows a discreet notice.
    pub issues: Vec<ServerIssue>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ServerLibraries {
    pub server: ServerDescriptor,
    pub libraries: Vec<Library>,
}

#[derive(Debug)]
pub struct Catalog {
    providers: RwLock<Vec<Arc<dyn MediaProvider>>>,
    /// The downloaded titles, as a library of their own. Always reachable by id; it is
    /// browsed (home, search) only in offline mode, when it is the one provider asked.
    local: RwLock<Option<Arc<dyn MediaProvider>>>,
    offline: AtomicBool,
    cache: Arc<MetadataCache>,
    ttl_secs: RwLock<u32>,
}

impl Catalog {
    pub fn new(cache: Arc<MetadataCache>, ttl_secs: u32) -> Self {
        Self { providers: RwLock::new(Vec::new()), local: RwLock::new(None), offline: AtomicBool::new(false), cache, ttl_secs: RwLock::new(ttl_secs) }
    }

    pub fn set_ttl(&self, ttl_secs: u32) {
        *self.ttl_secs.write() = ttl_secs;
    }

    pub fn add(&self, provider: Arc<dyn MediaProvider>) {
        let id = provider.descriptor().id;
        let mut providers = self.providers.write();
        providers.retain(|p| p.descriptor().id != id);
        providers.push(provider);
    }

    pub fn remove(&self, id: ServerId) {
        self.providers.write().retain(|p| p.descriptor().id != id);
        let _ = self.cache.invalidate_server(&id.to_string());
    }

    /// Swaps the whole set of live providers (profile switch). The metadata
    /// cache is kept: it is keyed by connection, so the next profile's
    /// screens paint from it at once.
    pub fn replace(&self, providers: Vec<Arc<dyn MediaProvider>>) {
        *self.providers.write() = providers;
    }

    /// Registers the provider of downloaded titles.
    pub fn set_local(&self, provider: Arc<dyn MediaProvider>) {
        *self.local.write() = Some(provider);
    }

    /// The provider of downloaded titles, whatever the mode.
    pub fn local(&self) -> Option<Arc<dyn MediaProvider>> {
        self.local.read().clone()
    }

    /// Offline mode: the servers are out of reach, so only downloaded titles are browsed.
    pub fn set_offline(&self, offline: bool) {
        self.offline.store(offline, Ordering::Relaxed);
    }

    pub fn is_offline(&self) -> bool {
        self.offline.load(Ordering::Relaxed)
    }

    /// The connected servers, whatever the mode.
    pub fn servers(&self) -> Vec<Arc<dyn MediaProvider>> {
        self.providers.read().clone()
    }

    /// What is browsed: the servers, or in offline mode the downloads alone.
    pub fn providers(&self) -> Vec<Arc<dyn MediaProvider>> {
        if self.is_offline() { self.local.read().iter().cloned().collect() } else { self.servers() }
    }

    pub fn provider(&self, id: ServerId) -> Result<Arc<dyn MediaProvider>> {
        if let Some(local) = self.local.read().as_ref()
            && local.descriptor().id == id
        {
            return Ok(Arc::clone(local));
        }
        self.providers
            .read()
            .iter()
            .find(|p| p.descriptor().id == id)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("server {id} is not connected")))
    }

    fn ttl(&self) -> u32 {
        *self.ttl_secs.read()
    }

    /// Runs `f` on every provider concurrently with a timeout.
    async fn fan_out<T, F, Fut>(&self, f: F) -> (Vec<(Arc<dyn MediaProvider>, T)>, Vec<ServerIssue>)
    where
        F: Fn(Arc<dyn MediaProvider>) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let providers = self.providers();
        let calls = providers.iter().map(|p| {
            let fut = f(Arc::clone(p));
            async move { (Arc::clone(p), tokio::time::timeout(SERVER_TIMEOUT, fut).await) }
        });
        let mut ok = Vec::new();
        let mut issues = Vec::new();
        for (p, res) in futures::future::join_all(calls).await {
            let d = p.descriptor();
            match res {
                Ok(Ok(v)) => ok.push((p, v)),
                Ok(Err(e)) => issues.push(ServerIssue { server: d.id, name: d.name.clone(), error: e.to_string() }),
                Err(_) => issues.push(ServerIssue { server: d.id, name: d.name.clone(), error: "timed out".into() }),
            }
        }
        for i in &issues {
            tracing::warn!(target: "catalog", server = %i.name, error = %i.error, "server unavailable");
        }
        (ok, issues)
    }

    pub async fn home(&self) -> Aggregated<Vec<HomeRow>> {
        let (results, issues) = self.fan_out(|p| async move { p.home().await }).await;
        let multi = results.len() > 1;
        let rows = merge_rows(results.into_iter().map(|(p, rows)| (p.descriptor().name.clone(), rows)).collect(), multi);
        Aggregated { data: rows, issues }
    }

    pub async fn libraries(&self) -> Aggregated<Vec<ServerLibraries>> {
        let (results, issues) = self.fan_out(|p| async move { p.libraries().await }).await;
        let data = results
            .into_iter()
            .map(|(p, libraries)| ServerLibraries { server: p.descriptor().clone(), libraries })
            .collect();
        Aggregated { data, issues }
    }

    /// Searches every server; the same title on several servers is merged.
    pub async fn search(&self, term: &str, limit: u32) -> Aggregated<Vec<MediaItem>> {
        let term = term.to_owned();
        let (results, issues) = self
            .fan_out(|p| {
                let term = term.clone();
                async move { p.search(&term, limit).await }
            })
            .await;
        let all: Vec<MediaItem> = results.into_iter().flat_map(|(_, items)| items).collect();
        Aggregated { data: dedupe(all), issues }
    }

    /// Favourites of every server that has them; the same title on several
    /// servers is merged. A server without favourites (a Plex account with
    /// no plex.tv sign-in) is left out quietly: it is not unavailable.
    pub async fn favorites(&self, limit: u32) -> Aggregated<Vec<MediaItem>> {
        let (results, issues) = self
            .fan_out(|p| async move {
                match p.favorites(limit).await {
                    Err(Error::Unsupported(_)) => Ok(Vec::new()),
                    other => other,
                }
            })
            .await;
        let all: Vec<MediaItem> = results.into_iter().flat_map(|(_, items)| items).collect();
        Aggregated { data: dedupe(all), issues }
    }

    /// Genres of the movies (or series) of every server, merged by name.
    /// A server that cannot list genres is left out quietly.
    pub async fn genres(&self, kind: ItemKind) -> Aggregated<Vec<String>> {
        let (results, issues) = self.fan_out(|p| async move { unsupported_as_empty(p.genres(kind).await) }).await;
        Aggregated { data: merge_genres(results.into_iter().map(|(_, genres)| genres).collect()), issues }
    }

    /// Titles of one genre on every server; the same title on several servers
    /// is merged. Each server pages on its own, so `start`/`limit` apply per server.
    pub async fn by_genre(&self, query: &GenreQuery) -> Aggregated<Vec<MediaItem>> {
        let (results, issues) = self
            .fan_out(|p| {
                let query = query.clone();
                async move { unsupported_as_empty(p.by_genre(&query).await) }
            })
            .await;
        let mut all: Vec<MediaItem> = results.into_iter().flat_map(|(_, items)| items).collect();
        if query.sort == SortBy::Title {
            sort_by_title(&mut all, query.order);
        }
        Aggregated { data: dedupe(all), issues }
    }

    /// "Because you watched …" rows built from what the person watched last:
    /// each server's own notion of similar titles, minus what is already seen.
    /// Empty when nothing was watched yet.
    pub async fn recommendations(&self) -> Aggregated<Vec<HomeRow>> {
        let (history, issues) = self
            .fan_out(|p| async move {
                let mut watched = Vec::new();
                // Series are found through their episodes, which carry the play date.
                for kind in [ItemKind::Movie, ItemKind::Episode] {
                    let query = ItemQuery {
                        parent: None,
                        kinds: vec![kind],
                        filter: ItemFilter::default(),
                        sort: SortBy::LastPlayed,
                        order: SortOrder::Descending,
                        start: 0,
                        limit: HISTORY_WINDOW,
                    };
                    match p.items(&query).await {
                        Ok(page) => watched.extend(page.items),
                        Err(Error::Unsupported(_)) => {}
                        Err(e) => return Err(e),
                    }
                }
                Ok(watched)
            })
            .await;
        let seeds = recent_seeds(history.into_iter().flat_map(|(_, items)| items).collect(), RECOMMENDATION_SEEDS);
        let found = futures::future::join_all(seeds.into_iter().map(|seed| async move {
            let similar = match self.provider(seed.id.server) {
                Ok(p) => tokio::time::timeout(SERVER_TIMEOUT, p.similar(&seed.id, SIMILAR_PER_SEED)).await.ok().and_then(Result::ok),
                Err(_) => None,
            };
            (seed, similar.unwrap_or_default())
        }))
        .await;
        Aggregated { data: recommendation_rows(found, SIMILAR_PER_SEED as usize), issues }
    }

    /// What the person's own server knows about them (none if it cannot say).
    pub async fn person(&self, id: &ItemRef) -> Option<PersonInfo> {
        let provider = self.provider(id.server).ok()?;
        match provider.person(id).await {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::debug!(target: "catalog", "person {id}: {e}");
                None
            }
        }
    }

    /// A person's movies and series on every server; the same title on
    /// several servers is merged. Servers that cannot search people are
    /// left out quietly.
    pub async fn person_items(&self, name: &str, hint: Option<&ItemRef>) -> Aggregated<Vec<MediaItem>> {
        let (results, issues) = self
            .fan_out(|p| {
                let name = name.to_owned();
                let hint = hint.cloned();
                async move {
                    match p.person_items(&name, hint.as_ref()).await {
                        Err(Error::Unsupported(_)) => Ok(Vec::new()),
                        other => other,
                    }
                }
            })
            .await;
        let all: Vec<MediaItem> = results.into_iter().flat_map(|(_, items)| items).collect();
        Aggregated { data: dedupe(all), issues }
    }

    pub async fn items(&self, query: &ItemQuery) -> Result<Page<MediaItem>> {
        let parent = query.parent.as_ref().ok_or_else(|| Error::Invalid("a library or parent is required".into()))?;
        self.provider(parent.server)?.items(query).await
    }

    /// Item detail with cache: fresh cache hits avoid a round-trip. Only for
    /// a live connection: the cache also holds other profiles' items.
    pub async fn item(&self, id: &ItemRef) -> Result<MediaItem> {
        let provider = self.provider(id.server)?;
        let key = format!("{}:item:{}", id.server, id.key);
        if let Ok(Some(c)) = self.cache.get::<MediaItem>(&key)
            && c.fresh
        {
            return Ok(c.value);
        }
        let item = provider.item(id).await?;
        let _ = self.cache.put(&id.server.to_string(), &key, &item, self.ttl());
        Ok(item)
    }

    /// Cached copy regardless of freshness (for instant display before
    /// refresh), of a live connection only.
    pub fn cached_item(&self, id: &ItemRef) -> Option<MediaItem> {
        self.provider(id.server).ok()?;
        self.cache.get::<MediaItem>(&format!("{}:item:{}", id.server, id.key)).ok().flatten().map(|c| c.value)
    }

    pub async fn children(&self, id: &ItemRef, kind: ItemKind) -> Result<Vec<MediaItem>> {
        self.provider(id.server)?.children(id, kind).await
    }

    pub async fn similar(&self, id: &ItemRef, limit: u32) -> Result<Vec<MediaItem>> {
        self.provider(id.server)?.similar(id, limit).await
    }

    pub async fn prerolls(&self, id: &ItemRef) -> Result<Vec<ItemRef>> {
        self.provider(id.server)?.prerolls(id).await
    }

    pub async fn adjacent(&self, id: &ItemRef) -> Result<Adjacent> {
        self.provider(id.server)?.adjacent_episodes(id).await
    }

    pub async fn set_played(&self, id: &ItemRef, played: bool) -> Result<()> {
        self.provider(id.server)?.set_played(id, played).await?;
        self.invalidate_item(id);
        Ok(())
    }

    pub async fn set_favorite(&self, id: &ItemRef, favorite: bool) -> Result<()> {
        self.provider(id.server)?.set_favorite(id, favorite).await?;
        self.invalidate_item(id);
        Ok(())
    }

    /// Called after playback reports too: resume points changed.
    pub fn invalidate_item(&self, id: &ItemRef) {
        let _ = self.cache.invalidate_prefix(&format!("{}:item:{}", id.server, id.key));
    }
}

#[cfg(test)]
mod tests {
    use oneshot_core::media::{ImageRef, ImageSize, Marker};
    use oneshot_core::playback::{ClientProfile, PlaybackInfo, PlaybackReport, StreamRequest, StreamTarget};
    use oneshot_core::server::{ProviderKind, ServerStatus, UserProfile};
    use url::Url;

    use super::*;

    /// A live connection that never answers (a hit proves the cache served
    /// it), except for its favourites when it has any (`None` = no support).
    #[derive(Debug)]
    struct Silent(ServerDescriptor, Option<Vec<MediaItem>>);

    #[async_trait::async_trait]
    impl MediaProvider for Silent {
        fn kind(&self) -> ProviderKind {
            self.0.kind
        }
        fn descriptor(&self) -> &ServerDescriptor {
            &self.0
        }
        async fn status(&self) -> ServerStatus {
            ServerStatus::Unauthorized
        }
        async fn libraries(&self) -> Result<Vec<Library>> {
            Err(Error::Unsupported("test".into()))
        }
        async fn home(&self) -> Result<Vec<HomeRow>> {
            Err(Error::Unsupported("test".into()))
        }
        async fn items(&self, _: &ItemQuery) -> Result<Page<MediaItem>> {
            Err(Error::Unsupported("test".into()))
        }
        async fn item(&self, _: &ItemRef) -> Result<MediaItem> {
            Err(Error::Unsupported("test".into()))
        }
        async fn children(&self, _: &ItemRef, _: ItemKind) -> Result<Vec<MediaItem>> {
            Err(Error::Unsupported("test".into()))
        }
        async fn search(&self, _: &str, _: u32) -> Result<Vec<MediaItem>> {
            Err(Error::Unsupported("test".into()))
        }
        async fn similar(&self, _: &ItemRef, _: u32) -> Result<Vec<MediaItem>> {
            Err(Error::Unsupported("test".into()))
        }
        async fn adjacent_episodes(&self, _: &ItemRef) -> Result<Adjacent> {
            Err(Error::Unsupported("test".into()))
        }
        async fn markers(&self, _: &ItemRef) -> Result<Vec<Marker>> {
            Err(Error::Unsupported("test".into()))
        }
        async fn set_played(&self, _: &ItemRef, _: bool) -> Result<()> {
            Err(Error::Unsupported("test".into()))
        }
        async fn set_favorite(&self, _: &ItemRef, _: bool) -> Result<()> {
            Err(Error::Unsupported("test".into()))
        }
        async fn playback_info(&self, _: &ItemRef, _: &ClientProfile) -> Result<PlaybackInfo> {
            Err(Error::Unsupported("test".into()))
        }
        async fn stream(&self, _: &StreamRequest) -> Result<StreamTarget> {
            Err(Error::Unsupported("test".into()))
        }
        async fn report(&self, _: &PlaybackReport) -> Result<()> {
            Err(Error::Unsupported("test".into()))
        }
        fn image_url(&self, _: &ImageRef, _: ImageSize) -> Result<Url> {
            Err(Error::Unsupported("test".into()))
        }
        fn auth_headers(&self) -> Vec<(String, String)> {
            Vec::new()
        }
        async fn favorites(&self, _: u32) -> Result<Vec<MediaItem>> {
            self.1.clone().ok_or_else(|| Error::Unsupported("test".into()))
        }
        async fn person_items(&self, _: &str, _: Option<&ItemRef>) -> Result<Vec<MediaItem>> {
            self.1.clone().ok_or_else(|| Error::Unsupported("test".into()))
        }
    }

    fn descriptor() -> ServerDescriptor {
        ServerDescriptor {
            id: ServerId::new(),
            kind: ProviderKind::Jellyfin,
            name: "jf".into(),
            remote_id: "jf".into(),
            base_url: Url::parse("http://jf.local/").unwrap(),
            alternate_urls: vec![],
            version: None,
            user: UserProfile { id: "u".into(), name: "Kid".into(), avatar: None, is_admin: false },
            disabled: false,
            home_member: false,
        }
    }

    #[tokio::test]
    async fn cached_items_are_served_for_live_connections_only() {
        let d = descriptor();
        let catalog = Catalog::new(Arc::new(MetadataCache::in_memory().unwrap()), 3600);
        let id = ItemRef { server: d.id, key: "42".into() };
        let item = MediaItem::new(id.clone(), ItemKind::Movie, "Cached");
        catalog.cache.put(&d.id.to_string(), &format!("{}:item:{}", d.id, id.key), &item, 3600).unwrap();

        assert!(catalog.item(&id).await.is_err(), "another profile's connection: nothing served");
        assert!(catalog.cached_item(&id).is_none());

        catalog.add(Arc::new(Silent(d, None)));
        assert_eq!(catalog.item(&id).await.unwrap().title, "Cached");
        assert_eq!(catalog.cached_item(&id).unwrap().title, "Cached");
    }

    #[tokio::test]
    async fn favourites_merge_servers_and_skip_those_without() {
        let catalog = Catalog::new(Arc::new(MetadataCache::in_memory().unwrap()), 3600);
        let jf = descriptor();
        let mut px = descriptor();
        px.kind = ProviderKind::Plex;
        let fav = MediaItem::new(ItemRef { server: jf.id, key: "1".into() }, ItemKind::Movie, "Fav");
        catalog.add(Arc::new(Silent(jf, Some(vec![fav]))));
        catalog.add(Arc::new(Silent(px, None)));

        let r = catalog.favorites(100).await;
        assert_eq!(r.data.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), ["Fav"]);
        assert!(r.issues.is_empty(), "a server without favourites is not an error");
    }

    #[tokio::test]
    async fn person_items_merge_servers_and_skip_those_without() {
        let catalog = Catalog::new(Arc::new(MetadataCache::in_memory().unwrap()), 3600);
        let jf = descriptor();
        let px = descriptor();
        let movie = MediaItem::new(ItemRef { server: jf.id, key: "1".into() }, ItemKind::Movie, "Forrest Gump");
        catalog.add(Arc::new(Silent(jf, Some(vec![movie]))));
        catalog.add(Arc::new(Silent(px, None)));
        let r = catalog.person_items("Tom Hanks", None).await;
        assert_eq!(r.data.len(), 1);
        assert!(r.issues.is_empty(), "a server that cannot search people is not an error");
    }
}
