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
use std::time::Duration;

use oneshot_core::ids::ItemRef;
use oneshot_core::media::{ItemKind, MediaItem};
use oneshot_core::provider::{Adjacent, MediaProvider};
use oneshot_core::query::{HomeRow, ItemQuery, Page};
use oneshot_core::server::{Library, ServerDescriptor};
use oneshot_core::{Error, Result, ServerId};
use oneshot_storage::cache::MetadataCache;
use parking_lot::RwLock;
use serde::Serialize;

pub use merge::{dedupe, merge_rows};

const SERVER_TIMEOUT: Duration = Duration::from_secs(8);

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
    cache: Arc<MetadataCache>,
    ttl_secs: RwLock<u32>,
}

impl Catalog {
    pub fn new(cache: Arc<MetadataCache>, ttl_secs: u32) -> Self {
        Self { providers: RwLock::new(Vec::new()), cache, ttl_secs: RwLock::new(ttl_secs) }
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

    pub fn providers(&self) -> Vec<Arc<dyn MediaProvider>> {
        self.providers.read().clone()
    }

    pub fn provider(&self, id: ServerId) -> Result<Arc<dyn MediaProvider>> {
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

    pub async fn items(&self, query: &ItemQuery) -> Result<Page<MediaItem>> {
        let parent = query.parent.as_ref().ok_or_else(|| Error::Invalid("a library or parent is required".into()))?;
        self.provider(parent.server)?.items(query).await
    }

    /// Item detail with cache: fresh cache hits avoid a round-trip.
    pub async fn item(&self, id: &ItemRef) -> Result<MediaItem> {
        let key = format!("{}:item:{}", id.server, id.key);
        if let Ok(Some(c)) = self.cache.get::<MediaItem>(&key)
            && c.fresh
        {
            return Ok(c.value);
        }
        let item = self.provider(id.server)?.item(id).await?;
        let _ = self.cache.put(&id.server.to_string(), &key, &item, self.ttl());
        Ok(item)
    }

    /// Cached copy regardless of freshness (for instant display before refresh).
    pub fn cached_item(&self, id: &ItemRef) -> Option<MediaItem> {
        self.cache.get::<MediaItem>(&format!("{}:item:{}", id.server, id.key)).ok().flatten().map(|c| c.value)
    }

    pub async fn children(&self, id: &ItemRef, kind: ItemKind) -> Result<Vec<MediaItem>> {
        self.provider(id.server)?.children(id, kind).await
    }

    pub async fn similar(&self, id: &ItemRef, limit: u32) -> Result<Vec<MediaItem>> {
        self.provider(id.server)?.similar(id, limit).await
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
