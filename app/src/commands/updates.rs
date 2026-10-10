//! Application updates, published as GitHub releases of the repository.
//!
//! The endpoint (`tauri.conf.json` › `plugins.updater`) is the `latest.json`
//! of the latest release, written by `tools/release.mjs`. Every package it
//! points to is signed with the updater key; the plugin refuses anything
//! else. The UI checks at launch and from Settings › General; the update
//! found by the last check is kept here until it is installed.

use std::sync::Arc;
use std::time::Duration;

use oneshot_core::{Error, Result};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::state::{AppState, VERSION};

/// The update found by the last check, ready to install.
#[derive(Default)]
pub struct Updates {
    pending: Mutex<Option<Update>>,
}

impl std::fmt::Debug for Updates {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Updates").field("pending", &self.pending.lock().as_ref().map(|u| u.version.clone())).finish()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    /// Release notes (Markdown), as written on the GitHub release.
    pub notes: Option<String>,
    /// RFC 3339.
    pub date: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "event")]
pub enum InstallProgress {
    /// `total`: bytes to download, when the server says.
    Started { total: Option<u64> },
    Progress { downloaded: u64 },
    /// Downloaded and verified; the installer runs, then Flick restarts.
    Installing,
}

fn err(e: tauri_plugin_updater::Error) -> Error {
    use tauri_plugin_updater::Error as E;
    match e {
        E::Reqwest(e) => Error::Network(oneshot_core::codes::UPD_NETWORK.tag(format!("The update server could not be reached ({e}). Check your connection."))),
        // The endpoint answered with an error: nothing published yet, or GitHub is down.
        E::ReleaseNotFound => Error::NotFound(oneshot_core::codes::UPD_NO_RELEASE.tag("No published release was found to update from. Try again later.")),
        e => Error::Other(oneshot_core::codes::UPD_OTHER.tag(format!("The update failed ({e}). Try again, or download the new version by hand."))),
    }
}

/// `Ok(None)`: this is the latest version.
#[tauri::command]
pub async fn update_check(app: AppHandle, updates: State<'_, Updates>) -> Result<Option<UpdateInfo>> {
    let state = Arc::clone(&app.state::<Arc<AppState>>());
    let mut builder = app
        .updater_builder()
        .timeout(Duration::from_secs(20))
        // Windows: the installer runs silently (`installMode: quiet`, then
        // relaunches Flick) and this process exits right away, without the
        // window's teardown: stop playback first.
        .on_before_exit(move || state.player.shutdown());
    if let Some(proxy) = app.state::<Arc<AppState>>().settings().network.proxy.as_deref().and_then(|p| p.parse().ok()) {
        builder = builder.proxy(proxy);
    }
    let found = builder.build().map_err(err)?.check().await;
    let found = match found {
        // A release published for one platform before the other.
        Err(e @ (tauri_plugin_updater::Error::TargetNotFound(_) | tauri_plugin_updater::Error::TargetsNotFound(_))) => {
            tracing::info!(target: "update", "{e}");
            None
        }
        r => r.map_err(err)?,
    };
    let info = found.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        current_version: VERSION.into(),
        notes: u.body.clone().filter(|n| !n.trim().is_empty()),
        date: u.raw_json.get("pub_date").and_then(|d| d.as_str()).map(Into::into),
    });
    match &info {
        Some(i) => tracing::info!(target: "update", "Flick {} is available (running {VERSION})", i.version),
        None => tracing::info!(target: "update", "Flick {VERSION} is up to date"),
    }
    *updates.pending.lock() = found;
    Ok(info)
}

/// Downloads, verifies and installs the update found by the last check,
/// then restarts Flick. On Windows the installer takes over and relaunches
/// the app, so this never returns there on success.
#[tauri::command]
pub async fn update_install(app: AppHandle, updates: State<'_, Updates>, on_progress: Channel<InstallProgress>) -> Result<()> {
    if cfg!(debug_assertions) {
        return Err(Error::Unsupported(oneshot_core::codes::UPD_DEV_BUILD.tag("Updates are not installed in a development build.")));
    }
    let update = updates.pending.lock().clone().ok_or_else(|| Error::NotFound(oneshot_core::codes::UPD_NO_PENDING.tag("There is no update waiting. Check for updates again.")))?;
    tracing::info!(target: "update", "installing Flick {}", update.version);
    let mut started = false;
    let mut downloaded = 0u64;
    let mut reported = 0u64;
    let bytes = update
        .download(
            |chunk, total| {
                if !started {
                    started = true;
                    let _ = on_progress.send(InstallProgress::Started { total });
                }
                downloaded += chunk as u64;
                // Chunks are a few KiB: one message per 256 KiB is plenty.
                if downloaded - reported >= 256 * 1024 {
                    reported = downloaded;
                    let _ = on_progress.send(InstallProgress::Progress { downloaded });
                }
            },
            || {},
        )
        .await
        .map_err(err)?;
    let _ = on_progress.send(InstallProgress::Installing);
    // Give the UI a moment to say so before the window goes away.
    tokio::time::sleep(Duration::from_millis(400)).await;
    update.install(bytes).map_err(err)?;
    tracing::info!(target: "update", "Flick {} installed, restarting", update.version);
    app.restart()
}
