//! Catalogue reads and user-state mutations.

use std::sync::Arc;

use oneshot_catalog::{Aggregated, ServerLibraries};
use oneshot_core::ids::ItemRef;
use oneshot_core::media::{ItemKind, Marker, MediaItem};
use oneshot_core::provider::Adjacent;
use oneshot_core::query::{GenreQuery, HomeRow, ItemQuery, Page};
use oneshot_core::Result;
use tauri::State;

use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

#[tauri::command]
pub async fn home(state: St<'_>) -> Result<Aggregated<Vec<HomeRow>>> {
    Ok(state.catalog.home().await)
}

#[tauri::command]
pub async fn libraries(state: St<'_>) -> Result<Aggregated<Vec<ServerLibraries>>> {
    let mut all = state.catalog.libraries().await;
    // Downloads are a block of their own, shown only when there is something in it.
    // (Offline, the downloads already are the catalogue.)
    if !state.catalog.is_offline()
        && let Some(local) = state.catalog.local()
        && let Ok(libraries) = local.libraries().await
        && !libraries.is_empty()
    {
        all.data.push(ServerLibraries { server: local.descriptor().clone(), libraries });
    }
    Ok(all)
}

#[tauri::command]
pub async fn items(state: St<'_>, query: ItemQuery) -> Result<Page<MediaItem>> {
    state.catalog.items(&query).await
}

#[tauri::command]
pub async fn item(state: St<'_>, id: ItemRef) -> Result<MediaItem> {
    state.catalog.item(&id).await
}

/// Instant, possibly stale copy (for first paint before `item` returns).
#[tauri::command(async)]
pub fn item_cached(state: St<'_>, id: ItemRef) -> Option<MediaItem> {
    state.catalog.cached_item(&id)
}

#[tauri::command]
pub async fn children(state: St<'_>, id: ItemRef, kind: ItemKind) -> Result<Vec<MediaItem>> {
    state.catalog.children(&id, kind).await
}

#[tauri::command]
pub async fn similar(state: St<'_>, id: ItemRef) -> Result<Vec<MediaItem>> {
    state.catalog.similar(&id, 20).await
}

#[tauri::command]
pub async fn adjacent(state: St<'_>, id: ItemRef) -> Result<Adjacent> {
    state.catalog.adjacent(&id).await
}

#[tauri::command]
pub async fn prerolls(state: St<'_>, id: ItemRef) -> Result<Vec<ItemRef>> {
    state.catalog.prerolls(&id).await
}

#[tauri::command]
pub async fn markers(state: St<'_>, id: ItemRef) -> Result<Vec<Marker>> {
    state.catalog.provider(id.server)?.markers(&id).await
}

#[tauri::command]
pub async fn search(state: St<'_>, term: String) -> Result<Aggregated<Vec<MediaItem>>> {
    Ok(state.catalog.search(&term, 40).await)
}

/// Genres of the movies or series of every server (Search page).
#[tauri::command]
pub async fn genres(state: St<'_>, kind: ItemKind) -> Result<Aggregated<Vec<String>>> {
    Ok(state.catalog.genres(kind).await)
}

#[tauri::command]
pub async fn by_genre(state: St<'_>, query: GenreQuery) -> Result<Aggregated<Vec<MediaItem>>> {
    Ok(state.catalog.by_genre(&query).await)
}

/// "Because you watched …" rows from the person's own history.
#[tauri::command]
pub async fn recommendations(state: St<'_>) -> Result<Aggregated<Vec<HomeRow>>> {
    Ok(state.catalog.recommendations().await)
}

/// The active profile's favourites on every server that has them.
#[tauri::command]
pub async fn favorites(state: St<'_>) -> Result<Aggregated<Vec<MediaItem>>> {
    Ok(state.catalog.favorites(200).await)
}

#[tauri::command]
pub async fn set_played(state: St<'_>, id: ItemRef, played: bool) -> Result<()> {
    state.catalog.set_played(&id, played).await
}

#[tauri::command]
pub async fn set_favorite(state: St<'_>, id: ItemRef, favorite: bool) -> Result<()> {
    state.catalog.set_favorite(&id, favorite).await
}
