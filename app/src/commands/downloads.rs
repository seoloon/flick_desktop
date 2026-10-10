//! Offline downloads. The transfer lives in `oneshot-flickdd`; these only wire it to the app.

use std::sync::Arc;

use oneshot_core::ids::ItemRef;
use oneshot_core::{Error, Result};
use oneshot_flickdd::Item;
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::flickserver::stored_invitation;
use crate::offline::LOCAL_SERVER;
use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadsStatus {
    /// A Flick Server invitation link is saved: downloads can be tried.
    pub configured: bool,
    /// Where downloads are written.
    pub directory: String,
    /// The library the finished downloads belong to: `<local server>:<download id>` is a title.
    pub local_server: String,
}

#[tauri::command(async)]
pub fn downloads_status(state: St<'_>) -> Result<DownloadsStatus> {
    Ok(DownloadsStatus {
        configured: stored_invitation()?.is_some(),
        directory: state.downloads.directory().to_string_lossy().into_owned(),
        local_server: LOCAL_SERVER.to_string(),
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
        return Err(Error::Invalid(oneshot_core::codes::DL_NOT_CONFIGURED.tag("Downloads need a Flick Server invitation link: add one in Settings › Flick Server.")));
    }
    state.downloads.enqueue(state.inner(), &item).await
}

#[tauri::command(async)]
pub fn downloads_pause(state: St<'_>, id: String) {
    state.downloads.manager().pause(&id);
}

#[tauri::command(async)]
pub fn downloads_resume(state: St<'_>, id: String) {
    state.downloads.manager().resume(&id);
}

/// Cancels a download (its partial file is deleted), or deletes a finished one.
#[tauri::command(async)]
pub fn downloads_remove(state: St<'_>, id: String) {
    state.downloads.remove(&state, &id, true);
}

/// Deletes every download, finished or not.
#[tauri::command(async)]
pub fn downloads_clear(state: St<'_>) {
    state.downloads.clear(&state);
}

/// Opens the downloads folder in the file manager.
#[tauri::command(async)]
pub fn downloads_open_folder(app: AppHandle, state: St<'_>) -> Result<()> {
    let dir = state.downloads.directory();
    std::fs::create_dir_all(dir).map_err(|e| Error::Other(oneshot_core::codes::DL_OPEN_FOLDER.tag(format!("The downloads folder could not be created ({e})."))))?;
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|e| Error::Other(oneshot_core::codes::DL_OPEN_FOLDER.tag(format!("The downloads folder could not be opened ({e})."))))
}
