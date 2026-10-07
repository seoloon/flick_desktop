//! Offline downloads. The transfer lives in `oneshot-flickdd`; these only wire it to the app.

use std::sync::Arc;

use oneshot_core::ids::ItemRef;
use oneshot_core::{Error, Result};
use oneshot_flickdd::Item;
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::downloads::default_directory;
use crate::flicksync::stored_invitation;
use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadsStatus {
    /// A Flick Server invitation link is saved: downloads can be tried.
    pub configured: bool,
    /// Where new downloads are written.
    pub directory: String,
}

#[tauri::command(async)]
pub fn downloads_status(app: AppHandle) -> Result<DownloadsStatus> {
    Ok(DownloadsStatus {
        configured: stored_invitation()?.is_some(),
        directory: default_directory(&app).to_string_lossy().into_owned(),
    })
}

#[tauri::command(async)]
pub fn downloads_list(state: St<'_>) -> Vec<Item> {
    state.downloads.manager().list()
}

/// Queues a movie or an episode (every episode of a season or a series).
#[tauri::command]
pub async fn downloads_enqueue(state: St<'_>, item: ItemRef) -> Result<Vec<Item>> {
    if stored_invitation()?.is_none() {
        return Err(Error::Invalid("Downloads need a Flick Server invitation link: add one in Settings › Watch Together.".into()));
    }
    state.downloads.enqueue(&state, &item).await
}

#[tauri::command(async)]
pub fn downloads_pause(state: St<'_>, id: String) {
    state.downloads.manager().pause(&id);
}

#[tauri::command(async)]
pub fn downloads_resume(state: St<'_>, id: String) {
    state.downloads.manager().resume(&id);
}

/// Cancels a download, or forgets a finished one (and deletes its file when asked).
#[tauri::command(async)]
pub fn downloads_remove(state: St<'_>, id: String, delete_file: bool) {
    state.downloads.manager().remove(&id, delete_file);
}

/// Opens the finished file with the system's player.
#[tauri::command(async)]
pub fn downloads_open(app: AppHandle, state: St<'_>, id: String) -> Result<()> {
    let path = finished_file(&state, &id)?;
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(|e| Error::Other(e.to_string()))
}

/// Shows the finished file in the file manager.
#[tauri::command(async)]
pub fn downloads_reveal(app: AppHandle, state: St<'_>, id: String) -> Result<()> {
    let path = finished_file(&state, &id)?;
    app.opener().reveal_item_in_dir(path).map_err(|e| Error::Other(e.to_string()))
}

fn finished_file(state: &AppState, id: &str) -> Result<std::path::PathBuf> {
    let item = state.downloads.manager().item(id).ok_or_else(|| Error::NotFound("download".into()))?;
    item.final_path.filter(|p| p.exists()).ok_or_else(|| Error::NotFound("The file is no longer there.".into()))
}
