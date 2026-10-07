//! Offline mode: when no server answers, Flick shows the downloaded titles only.

use std::sync::Arc;
use std::time::Duration;

use oneshot_core::Result;
use oneshot_core::server::ServerStatus;
use tauri::{AppHandle, Emitter, State};

use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

const PROBE_TIMEOUT: Duration = Duration::from_secs(4);

/// Whether every connected server is out of reach (and there is at least one). Switches the
/// catalogue to the downloads when so, and back when a server answers again.
/// A server that answers "unauthorized" is reachable: that is not being offline.
#[tauri::command]
pub async fn offline_check(app: AppHandle, state: St<'_>) -> Result<bool> {
    let servers = state.catalog.servers();
    let probes = servers.iter().map(|p| async move { tokio::time::timeout(PROBE_TIMEOUT, p.status()).await });
    let results = futures::future::join_all(probes).await;
    let offline = !servers.is_empty() && results.iter().all(|r| !matches!(r, Ok(ServerStatus::Online { .. } | ServerStatus::Unauthorized)));
    if state.catalog.is_offline() != offline {
        state.catalog.set_offline(offline);
        tracing::info!(target: "offline", offline, "mode changed");
        let _ = app.emit("offline", offline);
    }
    Ok(offline)
}
