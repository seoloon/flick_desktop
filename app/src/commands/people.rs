//! Person pages: server data completed by TMDB (key in the keychain, TMDB
//! answers cached for a week), and a person's titles on every server.

use std::sync::Arc;

use oneshot_catalog::Aggregated;
use oneshot_core::media::{ItemKind, MediaItem};
use oneshot_core::person::{PersonDetails, TmdbUse};
use oneshot_core::{ItemRef, Result};
use oneshot_storage::cache::MetadataCache;
use oneshot_storage::secrets;
use oneshot_tmdb::{TitleKind, Tmdb, TmdbPerson, merge_details};
use tauri::State;

use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

pub(crate) const TMDB_KEY: &str = "tmdb-key";
const WEEK: u32 = 7 * 24 * 3600;
/// Someone TMDB does not know is asked again after a day, not a week.
const DAY: u32 = 24 * 3600;

/// The TMDB client from the keychain's key, if one is stored and valid.
pub(crate) fn tmdb_from_keychain(http: &oneshot_net::reqwest::Client) -> Option<Tmdb> {
    let key = secrets::load_secret(TMDB_KEY).ok().flatten()?;
    Tmdb::new(http.clone(), &key).ok()
}

#[tauri::command]
pub fn tmdb_status(state: St<'_>) -> bool {
    state.tmdb.read().is_some()
}

#[tauri::command]
pub async fn tmdb_set_key(state: St<'_>, key: String) -> Result<()> {
    let tmdb = Tmdb::new(state.http(), &key)?;
    tmdb.check().await?;
    secrets::store_secret(TMDB_KEY, key.trim())?;
    *state.tmdb.write() = Some(tmdb);
    Ok(())
}

#[tauri::command(async)]
pub fn tmdb_remove_key(state: St<'_>) -> Result<()> {
    secrets::delete_secret(TMDB_KEY)?;
    *state.tmdb.write() = None;
    Ok(())
}

/// A cached TMDB answer and whether it is still within its TTL.
fn cached<T: serde::de::DeserializeOwned>(cache: &MetadataCache, key: &str) -> Option<(T, bool)> {
    cache.get::<T>(key).ok().flatten().map(|c| (c.value, c.fresh))
}

fn remember<T: serde::Serialize>(cache: &MetadataCache, key: &str, value: &T, ttl: u32) {
    if let Err(e) = cache.put("tmdb", key, value, ttl) {
        tracing::debug!(target: "cache", "tmdb cache write failed: {e}");
    }
}

/// The title whose TMDB cast tells namesakes apart: a movie or series
/// itself; for an episode or season, its series (their own TMDB ids are
/// episode and season ids, not series ids).
fn cast_source(from: &MediaItem, series: Option<&MediaItem>) -> Option<(TitleKind, String)> {
    match from.kind {
        ItemKind::Movie => from.external_ids.tmdb.clone().map(|id| (TitleKind::Movie, id)),
        ItemKind::Series => from.external_ids.tmdb.clone().map(|id| (TitleKind::Tv, id)),
        ItemKind::Season | ItemKind::Episode => series.and_then(|s| s.external_ids.tmdb.clone()).map(|id| (TitleKind::Tv, id)),
        _ => None,
    }
}

/// Where a match is remembered. Servers keep one record per name, so a
/// match found through a title's cast belongs to that title only.
fn match_key(person: &ItemRef, title: Option<&(TitleKind, String)>) -> String {
    match title {
        Some((TitleKind::Movie, id)) => format!("tmdb:match:{person}:movie:{id}"),
        Some((TitleKind::Tv, id)) => format!("tmdb:match:{person}:tv:{id}"),
        None => format!("tmdb:match:{person}"),
    }
}

/// The person on TMDB. Fresh answers come from the cache; when TMDB does
/// not answer, an older answer is better than none. "Not on TMDB" is
/// remembered too (for a day), so a missing person is not searched on
/// every visit.
async fn tmdb_person(cache: &MetadataCache, tmdb: &Tmdb, person: &ItemRef, name: &str, title: Option<(TitleKind, String)>, known: Option<&str>, language: &str) -> Result<Option<TmdbPerson>> {
    let match_key = match_key(person, title.as_ref());
    let id = match cached::<Option<u64>>(cache, &match_key) {
        Some((id, true)) => id,
        stale => match tmdb.identify(name, title.as_ref().map(|(k, id)| (*k, id.as_str())), known).await {
            Ok(id) => {
                remember(cache, &match_key, &id, if id.is_some() { WEEK } else { DAY });
                id
            }
            Err(e) => match stale {
                Some((id, _)) => id,
                None => return Err(e),
            },
        },
    };
    let Some(id) = id else { return Ok(None) };
    let person_key = format!("tmdb:person:{id}:{language}");
    match cached::<TmdbPerson>(cache, &person_key) {
        Some((p, true)) => Ok(Some(p)),
        stale => match tmdb.person(id, language).await {
            Ok(p) => {
                remember(cache, &person_key, &p, WEEK);
                Ok(Some(p))
            }
            Err(e) => match stale {
                Some((p, _)) => Ok(Some(p)),
                None => Err(e),
            },
        },
    }
}

/// What the person's server knows, at once: the page shows it while TMDB
/// answers (or does not).
#[tauri::command]
pub async fn person_server(state: St<'_>, person: ItemRef, name: String) -> Result<PersonDetails> {
    let server = state.catalog.person(&person).await;
    Ok(merge_details(&name, server.as_ref(), None, TmdbUse::Unavailable))
}

#[tauri::command]
pub async fn person_details(state: St<'_>, person: ItemRef, name: String, from: Option<ItemRef>, language: String) -> Result<PersonDetails> {
    // Independent lookups: side by side, not one after the other.
    let (server, origin) = tokio::join!(state.catalog.person(&person), async {
        match &from {
            Some(f) => state.catalog.item(f).await.ok(),
            None => None,
        }
    });
    let series = match origin.as_ref().and_then(|o| o.episode.as_ref()).and_then(|e| e.series.clone()) {
        Some(s) => state.catalog.item(&s).await.ok(),
        None => None,
    };
    let title = origin.as_ref().and_then(|o| cast_source(o, series.as_ref()));
    let tmdb = state.tmdb.read().clone();
    let (found, used) = match tmdb {
        None => (None, TmdbUse::NoKey),
        Some(t) => {
            let known = server.as_ref().and_then(|s| s.external_ids.tmdb.clone());
            match tmdb_person(&state.metadata, &t, &person, &name, title, known.as_deref(), &language).await {
                Ok(Some(p)) => (Some(p), TmdbUse::Used),
                Ok(None) => (None, TmdbUse::NotFound),
                Err(e) => {
                    tracing::warn!(target: "provider", "TMDB unavailable for {name}: {e}");
                    (None, TmdbUse::Unavailable)
                }
            }
        }
    };
    Ok(merge_details(&name, server.as_ref(), found.as_ref(), used))
}

#[tauri::command]
pub async fn person_items(state: St<'_>, person: ItemRef, name: String) -> Result<Aggregated<Vec<MediaItem>>> {
    Ok(state.catalog.person_items(&name, Some(&person)).await)
}

#[cfg(test)]
mod tests {
    use oneshot_core::ServerId;

    use super::*;

    fn titled(kind: ItemKind, tmdb: Option<&str>) -> MediaItem {
        let mut item = MediaItem::new(ItemRef::new(ServerId::new(), "1"), kind, "T");
        item.external_ids.tmdb = tmdb.map(str::to_owned);
        item
    }

    #[test]
    fn an_episode_is_told_apart_by_its_series_cast() {
        let episode = titled(ItemKind::Episode, Some("999"));
        let series = titled(ItemKind::Series, Some("1399"));
        assert_eq!(cast_source(&episode, Some(&series)), Some((TitleKind::Tv, "1399".to_owned())), "never the episode's own id");
        assert_eq!(cast_source(&episode, None), None);
        assert_eq!(cast_source(&titled(ItemKind::Movie, Some("13")), None), Some((TitleKind::Movie, "13".to_owned())));
    }

    const V3: &str = "0123456789abcdef0123456789abcdef";

    async fn tmdb(server: &wiremock::MockServer) -> Tmdb {
        Tmdb::new(oneshot_net::reqwest::Client::new(), V3).unwrap().with_base(url::Url::parse(&format!("{}/3/", server.uri())).unwrap())
    }

    #[tokio::test]
    async fn someone_tmdb_does_not_know_is_not_searched_again_and_again() {
        use wiremock::matchers::{method, path};
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(method("GET"))
            .and(path("/3/search/person"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({ "results": [] })))
            .expect(1)
            .mount(&server)
            .await;
        let cache = MetadataCache::in_memory().unwrap();
        let person = ItemRef::new(ServerId::new(), "p1");
        let t = tmdb(&server).await;
        assert_eq!(tmdb_person(&cache, &t, &person, "Nobody", None, None, "en-US").await.unwrap(), None);
        assert_eq!(tmdb_person(&cache, &t, &person, "Nobody", None, None, "en-US").await.unwrap(), None);
    }

    #[tokio::test]
    async fn an_old_answer_is_served_when_tmdb_does_not_answer() {
        use wiremock::matchers::any;
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(any()).respond_with(wiremock::ResponseTemplate::new(503)).mount(&server).await;
        let cache = MetadataCache::in_memory().unwrap();
        let person = ItemRef::new(ServerId::new(), "p1");
        let old = TmdbPerson {
            id: 31,
            name: "Tom Hanks".into(),
            biography: Some("Bio".into()),
            birthday: None,
            deathday: None,
            place_of_birth: None,
            department: None,
            profile_path: None,
            known_for: vec![],
        };
        // Both entries already past their week.
        cache.put("tmdb", &match_key(&person, None), &Some(31u64), 0).unwrap();
        cache.put("tmdb", "tmdb:person:31:en-US", &old, 0).unwrap();
        let found = tmdb_person(&cache, &tmdb(&server).await, &person, "Tom Hanks", None, None, "en-US").await.unwrap();
        assert_eq!(found, Some(old));
    }

    #[test]
    fn a_match_is_remembered_per_title_it_was_found_from() {
        let person = ItemRef::new(ServerId::new(), "p1");
        let a = match_key(&person, Some(&(TitleKind::Movie, "100".to_owned())));
        let b = match_key(&person, Some(&(TitleKind::Movie, "200".to_owned())));
        assert_ne!(a, b, "a namesake found from another title is not reused");
        assert_ne!(a, match_key(&person, None));
    }
}
