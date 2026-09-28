//! Playback commands. The player itself lives in `oneshot-player`; this only
//! wires the window (display under the window, DPI) and the catalogue.

use std::sync::Arc;

use oneshot_core::Result;
use oneshot_core::playback::PlaybackDecision;
use oneshot_player::presenter::Viewport;
use oneshot_player::state::{LiveStats, PlayerSnapshot};
use oneshot_player::{PlayRequest, PlayerCommand};
use serde::Deserialize;
use tauri::{State, WebviewWindow};

use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

/// OS id of the monitor the window is on (`\\.\DISPLAY1` on Windows), which
/// is how the capability report identifies displays.
pub fn current_display(window: &WebviewWindow) -> Option<String> {
    window.current_monitor().ok().flatten().and_then(|m| m.name().cloned())
}

#[tauri::command]
pub async fn play(window: WebviewWindow, state: St<'_>, request: PlayRequest) -> Result<PlaybackDecision> {
    start(&window, &state, request).await
}

/// Restarts the current title at its position with the current settings
/// (quality change from the player menu), keeping version and tracks.
#[tauri::command]
pub async fn player_reload(window: WebviewWindow, state: St<'_>) -> Result<PlaybackDecision> {
    let request = state.player.resume_request().ok_or_else(|| oneshot_core::Error::Invalid("nothing is playing".into()))?;
    start(&window, &state, request).await
}

async fn start(window: &WebviewWindow, state: &AppState, request: PlayRequest) -> Result<PlaybackDecision> {
    let item = request.item.clone().ok_or_else(|| oneshot_core::Error::Invalid("no item".into()))?;
    let provider = state.catalog.provider(item.server)?;
    let caps = state.caps.report();
    let display = current_display(window);
    let decision = state.player.play(provider, caps, state.settings(), display, request).await;
    // Resume points and "now playing" change server-side.
    state.catalog.invalidate_item(&item);
    decision
}

#[tauri::command]
pub fn player_command(state: St<'_>, command: PlayerCommand) -> Result<()> {
    state.player.command(command)
}

/// Video rectangle in CSS pixels; converted to physical pixels here.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CssRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[tauri::command]
pub fn player_viewport(window: WebviewWindow, state: St<'_>, rect: CssRect) -> Result<()> {
    let scale = window.scale_factor().unwrap_or(1.0);
    state.player.set_viewport(Viewport {
        x: (rect.x * scale).round() as i32,
        y: (rect.y * scale).round() as i32,
        width: (rect.width * scale).round().max(1.0) as u32,
        height: (rect.height * scale).round().max(1.0) as u32,
    });
    Ok(())
}

#[tauri::command(async)]
pub fn player_snapshot(state: St<'_>) -> PlayerSnapshot {
    state.player.snapshot()
}

#[tauri::command(async)]
pub fn player_stats(state: St<'_>) -> LiveStats {
    state.player.stats()
}
