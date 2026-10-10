//! Server administration. Each section is fetched independently so that a
//! permission error on one (e.g. users) does not hide the others.

use std::sync::Arc;

use oneshot_core::ids::ItemRef;
use oneshot_core::provider::{AdminServerInfo, AdminSession, AdminTask, AdminUser};
use oneshot_core::{Error, Result, ServerId};
use serde::Serialize;
use tauri::State;

use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminOverview {
    pub info: Result<AdminServerInfo, Error>,
    pub sessions: Result<Vec<AdminSession>, Error>,
    pub tasks: Result<Vec<AdminTask>, Error>,
    pub users: Result<Vec<AdminUser>, Error>,
    pub logs: Result<Vec<String>, Error>,
}

fn not_admin() -> Error {
    Error::Forbidden(oneshot_core::codes::AUTH_NOT_ADMIN.tag("Your account is not an administrator of this server."))
}

#[tauri::command]
pub async fn admin_overview(state: St<'_>, server: ServerId) -> Result<AdminOverview> {
    let provider = state.catalog.provider(server)?;
    let admin = provider.admin().ok_or_else(not_admin)?;
    let (info, sessions, tasks, users, logs) =
        tokio::join!(admin.server_info(), admin.sessions(), admin.tasks(), admin.users(), admin.logs());
    Ok(AdminOverview { info, sessions, tasks, users, logs })
}

#[tauri::command]
pub async fn admin_run_task(state: St<'_>, server: ServerId, task: String) -> Result<()> {
    let provider = state.catalog.provider(server)?;
    provider.admin().ok_or_else(not_admin)?.run_task(&task).await
}

#[tauri::command]
pub async fn admin_scan_library(state: St<'_>, library: ItemRef) -> Result<()> {
    let provider = state.catalog.provider(library.server)?;
    provider.admin().ok_or_else(not_admin)?.scan_library(&library).await
}
