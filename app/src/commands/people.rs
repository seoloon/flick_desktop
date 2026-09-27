//! Person pages: server data completed by TMDB (key in the keychain, TMDB
//! answers cached for a week), and a person's titles on every server.

use std::sync::Arc;

use oneshot_catalog::Aggregated;
use oneshot_core::media::{ItemKind, MediaItem};
use oneshot_core::person::{PersonDetails, TmdbUse};
use oneshot_core::{ItemRef, Result};
use oneshot_storage::secrets;
use oneshot_tmdb::{TitleKind, Tmdb, TmdbPerson, merge_details};
use tauri::State;

use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

pub(crate) const TMDB_KEY: &str = "tmdb-key";
const WEEK: u32 = 7 * 24 * 3600;

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

#[tauri::command]
pub fn tmdb_remove_key(state: St<'_>) -> Result<()> {
    secrets::delete_secret(TMDB_KEY)?;
    *state.tmdb.write() = None;
    Ok(())
}

fn cached<T: serde::de::DeserializeOwned>(state: &AppState, key: &str) -> Option<T> {
    state.metadata.get::<T>(key).ok().flatten().filter(|c| c.fresh).map(|c| c.value)
}

fn remember<T: serde::Serialize>(state: &AppState, key: &str, value: &T) {
    if let Err(e) = state.metadata.put("tmdb", key, value, WEEK) {
        tracing::debug!(target: "cache", "tmdb cache write failed: {e}");
    }
}

async fn tmdb_person(state: &AppState, tmdb: &Tmdb, person: &ItemRef, name: &str, from: Option<&MediaItem>, known: Option<&str>, language: &str) -> Result<Option<TmdbPerson>> {
    let match_key = format!("tmdb:match:{person}");
    let id = match cached::<u64>(state, &match_key) {
        Some(id) => Some(id),
        None => {
            let title = from.and_then(|f| {
                let kind = match f.kind {
                    ItemKind::Movie => TitleKind::Movie,
                    ItemKind::Series | ItemKind::Season | ItemKind::Episode => TitleKind::Tv,
                    _ => return None,
                };
                f.external_ids.tmdb.as_deref().map(|id| (kind, id))
            });
            let id = tmdb.identify(name, title, known).await?;
            if let Some(id) = id {
                remember(state, &match_key, &id);
            }
            id
        }
    };
    let Some(id) = id else { return Ok(None) };
    let person_key = format!("tmdb:person:{id}:{language}");
    if let Some(p) = cached::<TmdbPerson>(state, &person_key) {
        return Ok(Some(p));
    }
    let p = tmdb.person(id, language).await?;
    remember(state, &person_key, &p);
    Ok(Some(p))
}

#[tauri::command]
pub async fn person_details(state: St<'_>, person: ItemRef, name: String, from: Option<ItemRef>, language: String) -> Result<PersonDetails> {
    let server = state.catalog.person(&person).await;
    let origin = match &from {
        Some(f) => state.catalog.item(f).await.ok(),
        None => None,
    };
    let tmdb = state.tmdb.read().clone();
    let (found, used) = match tmdb {
        None => (None, TmdbUse::NoKey),
        Some(t) => {
            let known = server.as_ref().and_then(|s| s.external_ids.tmdb.clone());
            match tmdb_person(&state, &t, &person, &name, origin.as_ref(), known.as_deref(), &language).await {
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
