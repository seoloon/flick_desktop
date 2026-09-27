//! Settings, capabilities, diagnostics, window mode, artwork palette.

use std::sync::Arc;

use oneshot_core::capabilities::CapabilityReport;
use oneshot_core::ids::ItemRef;
use oneshot_core::media::{ImageKind, ImageRef, ImageSize};
use oneshot_core::settings::Settings;
use oneshot_core::{Error, Result};
use serde::Serialize;
use tauri::{State, WebviewWindow};

use crate::diagnostics::LogEntry;
use crate::images::{self, Palette};
use crate::state::{AppState, VERSION};

type St<'a> = State<'a, Arc<AppState>>;

#[tauri::command]
pub fn settings_get(state: St<'_>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn settings_set(state: St<'_>, settings: Settings) -> Result<()> {
    state.store.save_settings(&settings)?;
    let network_changed = state.settings.read().network != settings.network;
    state.catalog.set_ttl(settings.cache.metadata_ttl_secs);
    state.player.apply_settings(&settings);
    (state.log_reload)(&settings.advanced.log_level);
    state.images.set_max_bytes(u64::from(settings.cache.image_cache_mib) * 1024 * 1024);
    *state.settings.write() = settings;
    if network_changed {
        *state.http.write() = oneshot_net::client(&state.settings().network)?;
        // Providers hold their own client: reconnect them with the new policy.
        state.restore_servers();
        tracing::info!(target: "net", "network settings applied to all servers");
    }
    Ok(())
}

#[tauri::command]
pub async fn capabilities(state: St<'_>, refresh: bool) -> Result<Arc<CapabilityReport>> {
    let state = Arc::clone(&state);
    // Probing activates audio endpoints and D3D devices: keep it off the UI thread.
    tokio::task::spawn_blocking(move || if refresh { state.caps.refresh() } else { state.caps.report() })
        .await
        .map_err(|e| Error::Other(e.to_string()))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AboutInfo {
    pub version: String,
    pub libmpv: Result<(String, (u32, u32)), String>,
    pub credential_store: bool,
    pub config_dir: String,
    pub cache_dir: String,
    pub display: Option<String>,
}

#[tauri::command]
pub fn about(window: WebviewWindow, state: St<'_>) -> AboutInfo {
    let paths = state.store.paths();
    AboutInfo {
        version: VERSION.into(),
        libmpv: state.player.engine_info(&state.settings()).map_err(|e| e.to_string()),
        credential_store: oneshot_storage::secrets::available(),
        config_dir: paths.config.display().to_string(),
        cache_dir: paths.cache.display().to_string(),
        display: super::playback::current_display(&window),
    }
}

#[tauri::command]
pub fn diagnostics(state: St<'_>, since: u64, target: Option<String>) -> Vec<LogEntry> {
    state.diagnostics.entries(since, target.as_deref())
}

#[tauri::command]
pub fn set_fullscreen(window: WebviewWindow, fullscreen: bool) -> Result<()> {
    window.set_fullscreen(fullscreen).map_err(|e| Error::Other(e.to_string()))
}

#[tauri::command]
pub async fn palette(state: St<'_>, item: ItemRef, kind: ImageKind, tag: String) -> Result<Palette> {
    let image = ImageRef { item, kind, tag, blurhash: None };
    let bytes = images::load(&state, &image, ImageSize::Tiny).await?;
    tokio::task::spawn_blocking(move || images::palette(&bytes)).await.map_err(|e| Error::Other(e.to_string()))?
}

#[tauri::command]
pub fn cache_clear(state: St<'_>) -> Result<()> {
    state.images.clear()
}
