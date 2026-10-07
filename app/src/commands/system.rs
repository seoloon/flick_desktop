//! Settings, capabilities, diagnostics, window mode, artwork palette.

use std::sync::Arc;

use oneshot_core::capabilities::CapabilityReport;
use oneshot_core::ids::ItemRef;
use oneshot_core::media::{ImageKind, ImageRef, ImageSize};
use oneshot_core::settings::Settings;
use oneshot_core::{Error, Result};
use serde::Serialize;
use tauri::{LogicalSize, PhysicalPosition, PhysicalSize, State, WebviewWindow};

use crate::diagnostics::LogEntry;
use crate::images::{self, Palette};
use crate::state::{AppState, VERSION, WindowRestore};

type St<'a> = State<'a, Arc<AppState>>;

#[tauri::command]
pub fn settings_get(state: St<'_>) -> Settings {
    state.settings()
}

#[tauri::command(async)]
pub fn settings_set(state: St<'_>, settings: Settings) -> Result<()> {
    // Off the main thread two saves could overlap: one at a time, so the
    // last one sent is the one kept.
    let _one = state.settings_io.lock();
    // A profile is active: its part of the settings goes to the profile,
    // the rest to the shared settings file.
    let active = state.profiles.read().enabled.then(|| *state.active_profile.read()).flatten();
    match active {
        Some(id) => {
            let (shared, prefs) = oneshot_core::settings::split_settings(&settings, &state.store.settings());
            state.store.save_settings(&shared)?;
            state.update_profile(id, |p| p.prefs = prefs)?;
        }
        None => state.store.save_settings(&settings)?,
    }
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
        *state.tmdb.write() = super::people::tmdb_from_keychain(&state.http());
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

#[tauri::command(async)]
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

#[tauri::command(async)]
pub fn diagnostics(state: St<'_>, since: u64, target: Option<String>) -> Vec<LogEntry> {
    state.diagnostics.entries(since, target.as_deref())
}

#[tauri::command]
pub fn set_fullscreen(window: WebviewWindow, fullscreen: bool) -> Result<()> {
    window.set_fullscreen(fullscreen).map_err(|e| Error::Other(e.to_string()))
}

/// Picture-in-picture: the main window shrinks to a small always-on-top
/// video in a corner of its screen, and gets its size, place and state back
/// on exit. The video layer follows the window, so nothing is re-created.
#[tauri::command]
pub fn window_pip(window: WebviewWindow, state: St<'_>, enter: bool) -> Result<()> {
    let err = |e: tauri::Error| Error::Other(e.to_string());
    let mut saved = state.pip_restore.lock();
    // Subtitles follow the window size: bigger in the small window, back to normal after.
    state.player.set_pip(enter, &state.settings());
    if enter {
        if saved.is_some() {
            return Ok(());
        }
        *saved = Some(WindowRestore {
            position: window.outer_position().map_err(err)?,
            size: window.inner_size().map_err(err)?,
            maximized: window.is_maximized().map_err(err)?,
            fullscreen: window.is_fullscreen().map_err(err)?,
        });
        let monitor = window.current_monitor().map_err(err)?;
        window.set_fullscreen(false).map_err(err)?;
        window.unmaximize().map_err(err)?;
        window.set_min_size(Some(LogicalSize::new(PIP_MIN_WIDTH, PIP_MIN_WIDTH * 9.0 / 16.0))).map_err(err)?;
        window.set_always_on_top(true).map_err(err)?;
        if let Some(m) = monitor {
            let area = m.work_area();
            let scale = m.scale_factor();
            let margin = (PIP_MARGIN * scale).round() as i32;
            let width = (f64::from(area.size.width) * 0.24).clamp(PIP_MIN_WIDTH * scale, 640.0 * scale).round() as u32;
            let height = width * 9 / 16;
            window.set_size(PhysicalSize::new(width, height)).map_err(err)?;
            window
                .set_position(PhysicalPosition::new(
                    area.position.x + area.size.width as i32 - width as i32 - margin,
                    area.position.y + area.size.height as i32 - height as i32 - margin,
                ))
                .map_err(err)?;
        }
    } else if let Some(r) = saved.take() {
        window.set_always_on_top(false).map_err(err)?;
        window.set_min_size(Some(LogicalSize::new(MIN_WIDTH, MIN_HEIGHT))).map_err(err)?;
        if cfg!(target_os = "macos") && r.maximized {
            // Zoom straight from the small window: setting the old size first leaves
            // the window a few points short of the zoomed frame, and macOS then
            // ignores the zoom (the window came back smaller, in the PiP corner).
            window.maximize().map_err(err)?;
        } else {
            window.set_size(r.size).map_err(err)?;
            window.set_position(r.position).map_err(err)?;
            if r.maximized {
                window.maximize().map_err(err)?;
            }
        }
        if r.fullscreen {
            window.set_fullscreen(true).map_err(err)?;
        }
        window.set_focus().map_err(err)?;
    }
    Ok(())
}

/// Must match `minWidth` / `minHeight` in tauri.conf.json.
const MIN_WIDTH: f64 = 960.0;
const MIN_HEIGHT: f64 = 600.0;
const PIP_MIN_WIDTH: f64 = 256.0;
const PIP_MARGIN: f64 = 24.0;

#[tauri::command]
pub async fn palette(state: St<'_>, item: ItemRef, kind: ImageKind, tag: String) -> Result<Palette> {
    let image = ImageRef { item, kind, tag, blurhash: None };
    let bytes = images::load(&state, &image, ImageSize::Tiny).await?;
    tokio::task::spawn_blocking(move || images::palette(&bytes)).await.map_err(|e| Error::Other(e.to_string()))?
}

/// Palette of a TMDB photo (`/abc.jpg`): a person page lights the
/// background with it, like artwork does.
#[tauri::command]
pub async fn tmdb_palette(state: St<'_>, path: String) -> Result<Palette> {
    let bytes = images::load_tmdb(&state, &format!("w185{path}")).await?;
    tokio::task::spawn_blocking(move || images::palette(&bytes)).await.map_err(|e| Error::Other(e.to_string()))?
}

/// Deleting a large image cache takes seconds: done on a blocking thread.
#[tauri::command]
pub async fn cache_clear(state: St<'_>) -> Result<()> {
    let images = state.images.clone();
    tokio::task::spawn_blocking(move || images.clear()).await.map_err(|e| Error::Other(e.to_string()))?
}
