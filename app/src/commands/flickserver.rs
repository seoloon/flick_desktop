//! The Flick Server's invitation link. FlickSync and FlickDD both use it, so it is
//! managed here and not under either of them.

use std::sync::Arc;

use oneshot_core::{Error, Result};
use oneshot_flickserver::Invitation;
use oneshot_flicksync::diagnose::Report;
use oneshot_storage::secrets;
use serde::Serialize;
use tauri::State;

use crate::flickserver::{INVITE_ENTRY, clear_invitation, stored_invitation};
use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

/// What the UI may know about the saved invitation: never the key.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvitationInfo {
    /// `host[:port][/prefix]`.
    pub address: String,
    pub tls: bool,
    /// Plain HTTP across the Internet: tokens would travel in the clear.
    pub insecure_remote: bool,
}

impl From<&Invitation> for InvitationInfo {
    fn from(i: &Invitation) -> Self {
        Self { address: i.address(), tls: i.tls(), insecure_remote: i.is_insecure_remote() }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvitationAdded {
    pub info: InvitationInfo,
    /// Kept only when the server answers and is ready; otherwise nothing was saved.
    pub saved: bool,
    pub report: Report,
}

/// Settings › Flick Server › "Test connection": which step fails, and why.
#[tauri::command]
pub async fn flickserver_diagnose(state: St<'_>) -> Result<Report> {
    Ok(state.flicksync.diagnose(&state).await)
}

#[tauri::command(async)]
pub fn flickserver_invitation() -> Result<Option<InvitationInfo>> {
    Ok(stored_invitation()?.as_ref().map(InvitationInfo::from))
}

/// Checks a pasted link against its server, and keeps it (in the keychain) when
/// the server answers and is ready. A bad link is an error with a sentence for the user.
#[tauri::command]
pub async fn flickserver_add_invitation(state: St<'_>, link: String) -> Result<InvitationAdded> {
    let invitation = Invitation::parse(&link).map_err(|e| Error::Invalid(crate::flickserver::invite_code(e).tag(e.message())))?;
    let report = state.flicksync.check_invitation(&state, &invitation).await;
    let saved = report.reachable_and_ready();
    if saved {
        secrets::store_secret(INVITE_ENTRY, &invitation.link())?;
    }
    Ok(InvitationAdded { info: InvitationInfo::from(&invitation), saved, report })
}

#[tauri::command(async)]
pub fn flickserver_clear_invitation() -> Result<()> {
    clear_invitation()
}
