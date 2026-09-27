//! Catalogue reads and user-state mutations.

use std::sync::Arc;

use oneshot_catalog::{Aggregated, ServerLibraries};
use oneshot_core::ids::ItemRef;
use oneshot_core::media::{ItemKind, Marker, MediaItem};
use oneshot_core::provider::Adjacent;
use oneshot_core::query::{HomeRow, ItemQuery, Page};
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
    Ok(state.catalog.libraries().await)
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
#[tauri::command]
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
pub async fn markers(state: St<'_>, id: ItemRef) -> Result<Vec<Marker>> {
    state.catalog.provider(id.server)?.markers(&id).await
}

#[tauri::command]
pub async fn search(state: St<'_>, term: String) -> Result<Aggregated<Vec<MediaItem>>> {
    Ok(state.catalog.search(&term, 40).await)
}

#[tauri::command]
pub async fn set_played(state: St<'_>, id: ItemRef, played: bool) -> Result<()> {
    state.catalog.set_played(&id, played).await
}

#[tauri::command]
pub async fn set_favorite(state: St<'_>, id: ItemRef, favorite: bool) -> Result<()> {
    state.catalog.set_favorite(&id, favorite).await
}
