//! Watch-together commands. All room logic lives in `oneshot-flicksync`; these
//! only wire it to the app (settings, keychain, catalogue).

use std::sync::Arc;

use oneshot_core::ids::ItemRef;
use oneshot_core::{Error, Result};
use oneshot_flicksync::DebugInfo;
use oneshot_flicksync::protocol::ControlMode;
use oneshot_flicksync::room::RoomState;
use tauri::State;

use crate::flicksync::{Status, into_core_error};
use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

/// Whether to offer watch-together at all: configured and reachable.
#[tauri::command]
pub async fn flicksync_status(state: St<'_>) -> Result<Status> {
    Ok(state.flicksync.status(&state).await)
}

#[tauri::command]
pub async fn flicksync_create(state: St<'_>, host_only: Option<bool>) -> Result<RoomState> {
    let client = state.flicksync.client(&state).await?;
    let mode = if host_only.unwrap_or(false) { ControlMode::HostOnly } else { ControlMode::Everyone };
    client.create_room(mode).await.map_err(into_core_error)
}

#[tauri::command]
pub async fn flicksync_join(state: St<'_>, code: String) -> Result<RoomState> {
    let client = state.flicksync.client(&state).await?;
    client.join_room(&code).await.map_err(into_core_error)
}

/// Leaves for good. Navigating away from the room screen does not call this.
#[tauri::command]
pub async fn flicksync_leave(state: St<'_>) -> Result<()> {
    if let Some(client) = state.flicksync.room_client() {
        client.leave().await;
    }
    Ok(())
}

#[tauri::command(async)]
pub fn flicksync_state(state: St<'_>) -> Option<RoomState> {
    state.flicksync.room_client().map(|c| c.state())
}

/// Host: choose the title the room watches (without opening the player first).
#[tauri::command]
pub async fn flicksync_select_media(state: St<'_>, item: ItemRef) -> Result<()> {
    state.flicksync.select_media(&state, &item).await
}

#[tauri::command(async)]
pub fn flicksync_chat(state: St<'_>, text: String) -> Result<()> {
    let client = state.flicksync.room_client().ok_or_else(|| Error::Invalid("not in a room".into()))?;
    client.send_chat(&text).map_err(into_core_error)
}

#[tauri::command(async)]
pub fn flicksync_update_room(state: St<'_>, host_only: Option<bool>, chat_enabled: Option<bool>) -> Result<()> {
    let client = state.flicksync.room_client().ok_or_else(|| Error::Invalid("not in a room".into()))?;
    let mode = host_only.map(|h| if h { ControlMode::HostOnly } else { ControlMode::Everyone });
    client.update_room(mode, chat_enabled).map_err(into_core_error)
}

#[tauri::command(async)]
pub fn flicksync_close_room(state: St<'_>) -> Result<()> {
    let client = state.flicksync.room_client().ok_or_else(|| Error::Invalid("not in a room".into()))?;
    client.close_room().map_err(into_core_error)
}

/// The player was closed: open the room's title again and catch up.
#[tauri::command(async)]
pub fn flicksync_resync_media(state: St<'_>) -> Result<()> {
    let client = state.flicksync.room_client().ok_or_else(|| Error::Invalid("not in a room".into()))?;
    client.resync_media().map_err(into_core_error)
}

#[tauri::command(async)]
pub fn flicksync_debug(state: St<'_>) -> Option<DebugInfo> {
    state.flicksync.room_client().map(|c| c.debug())
}

/// The room's title as an item of ours, once resolved (to open the player on it).
#[tauri::command(async)]
pub fn flicksync_current_item(state: St<'_>) -> Option<ItemRef> {
    state.flicksync.current_item()
}
