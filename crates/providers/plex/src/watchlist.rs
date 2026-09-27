//! The plex.tv Watchlist: the Plex side of Flick's favourites. It belongs to
//! a plex.tv account, not to a server; titles are matched to a server's
//! library by their `plex://` guid (see `PlexProvider::favorites`).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use futures::lock::Mutex as AsyncMutex;
use oneshot_core::{Error, Result};
use oneshot_net::reqwest::{Client, Method};
use url::Url;

use crate::auth::{PlexAuth, PlexIdentity};
use serde::Deserialize;

use crate::dto::Envelope;

const DISCOVER: &str = "https://discover.provider.plex.tv/";
const PLEX_TV: &str = "https://plex.tv/";
/// The Watchlist changes from other apps too; one minute keeps screens
/// fast without hiding those changes for long.
const CACHE_TTL: Duration = Duration::from_secs(60);
/// plex.tv answers 400 ("Invalid value provided for x-plex-container-size")
/// to larger pages.
const PAGE_SIZE: u32 = 100;
/// A Watchlist longer than this is cut (favourites are capped lower anyway).
const MAX_PAGES: u32 = 10;

/// Only what the Watchlist needs: Discover items differ from library items
/// (string tag ids, no media), so the library `Metadata` is not reused.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    total_size: Option<u32>,
    #[serde(rename = "Metadata", default)]
    metadata: Vec<PageItem>,
}

#[derive(Debug, Deserialize)]
struct PageItem {
    guid: Option<String>,
    title: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WatchlistEntry {
    /// `plex://movie/…`, `plex://show/…`
    pub guid: String,
    pub title: String,
}

#[derive(Debug)]
pub struct Watchlist {
    http: Client,
    identity: PlexIdentity,
    /// plex.tv token of the account whose Watchlist this is.
    token: String,
    /// Set when the token's owner is not known for sure: it is checked once
    /// against plex.tv before any use.
    expected_user: Option<String>,
    discover: Url,
    plex_tv: Url,
    owner_ok: Mutex<Option<bool>>,
    cache: Mutex<Option<(Instant, Vec<WatchlistEntry>)>>,
    /// One fetch at a time: concurrent screens share it.
    fetching: AsyncMutex<()>,
}

/// The Discover id of a Plex catalogue guid (`plex://movie/5d77…` → `5d77…`).
pub fn discover_key(guid: &str) -> Option<&str> {
    guid.strip_prefix("plex://")?.rsplit('/').next().filter(|k| !k.is_empty())
}

impl Watchlist {
    pub fn new(http: Client, identity: PlexIdentity, token: String) -> Self {
        Self {
            http,
            identity,
            token,
            expected_user: None,
            discover: Url::parse(DISCOVER).expect("static url"),
            plex_tv: Url::parse(PLEX_TV).expect("static url"),
            owner_ok: Mutex::new(None),
            cache: Mutex::new(None),
            fetching: AsyncMutex::new(()),
        }
    }

    /// Use the token only if it signs in as `user_id` (a token whose owner
    /// is not recorded, e.g. from before per-user tokens were kept).
    pub fn for_user(mut self, user_id: impl Into<String>) -> Self {
        self.expected_user = Some(user_id.into());
        self
    }

    /// For tests: point at mock Discover and plex.tv servers.
    pub fn with_bases(mut self, discover: Url, plex_tv: Url) -> Self {
        self.discover = discover;
        self.plex_tv = plex_tv;
        self
    }

    async fn check_owner(&self) -> Result<()> {
        let Some(expected) = &self.expected_user else { return Ok(()) };
        let known = *self.owner_ok.lock().expect("owner lock");
        let ok = match known {
            Some(ok) => ok,
            None => {
                let auth = PlexAuth::new(self.http.clone(), self.identity.clone()).with_base(self.plex_tv.clone());
                let ok = auth.account(&self.token).await?.user_id == *expected;
                *self.owner_ok.lock().expect("owner lock") = Some(ok);
                ok
            }
        };
        if ok { Ok(()) } else { Err(Error::Unsupported("Watchlist (the stored plex.tv sign-in is another account's)".into())) }
    }

    fn cached(&self) -> Option<Vec<WatchlistEntry>> {
        let cache = self.cache.lock().expect("cache lock");
        cache.as_ref().filter(|(at, _)| at.elapsed() < CACHE_TTL).map(|(_, e)| e.clone())
    }

    /// Titles on the Watchlist, most recently added first.
    pub async fn entries(&self) -> Result<Vec<WatchlistEntry>> {
        if let Some(hit) = self.cached() {
            return Ok(hit);
        }
        let _one = self.fetching.lock().await;
        if let Some(hit) = self.cached() {
            return Ok(hit);
        }
        self.check_owner().await?;
        let mut entries = Vec::new();
        for n in 0..MAX_PAGES {
            let start = n * PAGE_SIZE;
            let page = self.page(start).await?;
            let fetched = page.metadata.len();
            entries.extend(page.metadata.into_iter().filter_map(|m| Some(WatchlistEntry { guid: m.guid?, title: m.title })));
            let total = page.total_size.unwrap_or(0);
            if fetched == 0 || start + PAGE_SIZE >= total {
                break;
            }
        }
        *self.cache.lock().expect("cache lock") = Some((Instant::now(), entries.clone()));
        Ok(entries)
    }

    async fn page(&self, start: u32) -> Result<Page> {
        let mut url = oneshot_net::join(&self.discover, "library/sections/watchlist/all")?;
        url.query_pairs_mut()
            .append_pair("sort", "watchlistedAt:desc")
            .append_pair("X-Plex-Container-Start", &start.to_string())
            .append_pair("X-Plex-Container-Size", &PAGE_SIZE.to_string());
        let rb = self.identity.apply(self.http.get(url), Some(&self.token));
        let page: Envelope<Page> = oneshot_net::json(rb.send().await.map_err(oneshot_net::map_err)?).await?;
        Ok(page.container)
    }

    pub async fn contains(&self, guid: &str) -> Result<bool> {
        Ok(self.entries().await?.iter().any(|e| e.guid == guid))
    }

    /// Adds `guid` to the Watchlist (`on`) or removes it.
    pub async fn set(&self, guid: &str, on: bool) -> Result<()> {
        let key = discover_key(guid).ok_or_else(|| Error::Unsupported("Watchlist (not a Plex catalogue title)".into()))?;
        self.check_owner().await?;
        let action = if on { "actions/addToWatchlist" } else { "actions/removeFromWatchlist" };
        let mut url = oneshot_net::join(&self.discover, action)?;
        url.query_pairs_mut().append_pair("ratingKey", key);
        let rb = self.identity.apply(self.http.request(Method::PUT, url), Some(&self.token));
        oneshot_net::ensure_ok(rb.send().await.map_err(oneshot_net::map_err)?).await?;
        *self.cache.lock().expect("cache lock") = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_keys_come_from_plex_guids_only() {
        assert_eq!(discover_key("plex://movie/5d776b59ad5437001f79c6f8"), Some("5d776b59ad5437001f79c6f8"));
        assert_eq!(discover_key("plex://episode/abc"), Some("abc"));
        assert_eq!(discover_key("com.plexapp.agents.imdb://tt123"), None);
        assert_eq!(discover_key("plex://movie/"), None);
    }
}
