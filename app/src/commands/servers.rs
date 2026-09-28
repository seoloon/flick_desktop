//! Server onboarding (Jellyfin password / Quick Connect, Plex PIN) and management.

use std::sync::Arc;

use oneshot_core::server::{ProviderKind, ServerDescriptor, ServerStatus, UserProfile};
use oneshot_core::{Error, Result, ServerId};
use oneshot_jellyfin::{Connector, Session};
use oneshot_plex::{PlexAuth, auth::require_reachable};
use oneshot_storage::secrets;
use serde::Serialize;
use tauri::State;
use tauri_plugin_opener::OpenerExt;
use url::Url;

use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerEntry {
    pub server: ServerDescriptor,
    /// False when the token is missing from the keychain.
    pub connected: bool,
}

#[tauri::command(async)]
pub fn servers_list(state: St<'_>, all: Option<bool>) -> Vec<ServerEntry> {
    let connected: Vec<ServerId> = state.catalog.providers().iter().map(|p| p.descriptor().id).collect();
    let members = if all.unwrap_or(false) { None } else { state.active_members() };
    state
        .servers
        .read()
        .iter()
        .filter(|s| members.as_ref().is_none_or(|m| m.contains(&s.id)))
        .map(|s| ServerEntry { server: s.clone(), connected: connected.contains(&s.id) })
        .collect()
}

#[tauri::command]
pub async fn server_status(state: St<'_>, id: ServerId) -> Result<ServerStatus> {
    Ok(state.catalog.provider(id)?.status().await)
}

#[tauri::command(async)]
pub fn server_remove(state: St<'_>, id: ServerId) -> Result<()> {
    state.remove_server(id)
}

#[tauri::command(async)]
pub fn server_set_enabled(state: St<'_>, id: ServerId, enabled: bool) -> Result<()> {
    state.set_server_enabled(id, enabled)
}

// ------------------------------------------------------------------ Jellyfin

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub url: Url,
    pub name: String,
    pub version: Option<String>,
}

#[tauri::command]
pub async fn jellyfin_probe(state: St<'_>, address: String) -> Result<ProbeResult> {
    let connector = Connector::new(state.http(), state.jellyfin_identity());
    let (url, info) = connector.probe(&address).await?;
    Ok(ProbeResult { url, name: info.server_name.unwrap_or_else(|| "Jellyfin".into()), version: info.version })
}

pub(crate) fn jellyfin_descriptor(s: &Session) -> ServerDescriptor {
    ServerDescriptor {
        id: ServerId::new(),
        kind: ProviderKind::Jellyfin,
        name: s.server_name.clone(),
        remote_id: s.server_id.clone(),
        base_url: s.base_url.clone(),
        alternate_urls: vec![],
        version: s.version.clone(),
        user: UserProfile { id: s.user_id.clone(), name: s.user_name.clone(), avatar: None, is_admin: s.is_admin },
        disabled: false,
        home_member: false,
    }
}

#[tauri::command]
pub async fn jellyfin_login(state: St<'_>, url: Url, username: String, password: String) -> Result<ServerDescriptor> {
    let connector = Connector::new(state.http(), state.jellyfin_identity());
    // The password is used once to obtain a token and never stored.
    let session = connector.login(&url, &username, &password).await?;
    let d = state.register_server(jellyfin_descriptor(&session), &session.token)?;
    Ok(d)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickConnect {
    pub code: String,
    pub secret: String,
}

#[tauri::command]
pub async fn jellyfin_quick_connect_start(state: St<'_>, url: Url) -> Result<QuickConnect> {
    let r = Connector::new(state.http(), state.jellyfin_identity()).quick_connect_start(&url).await?;
    Ok(QuickConnect { code: r.code, secret: r.secret })
}

#[tauri::command]
pub async fn jellyfin_quick_connect_poll(state: St<'_>, url: Url, secret: String) -> Result<Option<ServerDescriptor>> {
    let connector = Connector::new(state.http(), state.jellyfin_identity());
    let Some(session) = connector.quick_connect_poll(&url, &secret).await? else { return Ok(None) };
    Ok(Some(state.register_server(jellyfin_descriptor(&session), &session.token)?))
}

// ---------------------------------------------------------------------- Plex

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlexPin {
    pub id: i64,
    pub code: String,
    pub auth_url: Url,
}

/// Creates a PIN and opens the Plex sign-in page in the system browser.
#[tauri::command]
pub async fn plex_pin_start(app: tauri::AppHandle, state: St<'_>) -> Result<PlexPin> {
    let pin = PlexAuth::new(state.http(), state.plex_identity()).start_pin().await?;
    if let Err(e) = app.opener().open_url(pin.auth_url.as_str(), None::<&str>) {
        tracing::warn!(target: "provider", "could not open the browser: {e}");
    }
    Ok(PlexPin { id: pin.id, code: pin.code, auth_url: pin.auth_url })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlexServerChoice {
    pub machine_id: String,
    pub name: String,
    pub owned: bool,
    pub version: Option<String>,
    /// Best reachable address, if any.
    pub url: Option<Url>,
}

/// Returns `None` while the user has not approved the PIN yet.
#[tauri::command]
pub async fn plex_pin_poll(state: St<'_>, id: i64) -> Result<Option<Vec<PlexServerChoice>>> {
    let auth = PlexAuth::new(state.http(), state.plex_identity());
    let Some(account) = auth.poll_pin(id).await? else { return Ok(None) };
    let servers = auth.discover(&account.token).await?;
    secrets::store_secret(PLEX_ACCOUNT_KEY, &account.token)?;
    store_plex_user_token(&account.user_id, &account.token);
    *state.plex_account.lock() = Some(account.token);
    Ok(Some(
        servers
            .into_iter()
            .map(|s| PlexServerChoice {
                url: s.reachable.first().cloned(),
                machine_id: s.machine_id,
                name: s.name,
                owned: s.owned,
                version: s.version,
            })
            .collect(),
    ))
}

/// Keychain slot for the plex.tv account token (lets the user add more
/// servers, and switch Plex Home members, without a new PIN).
pub(crate) const PLEX_ACCOUNT_KEY: &str = "plex-account";

/// Keychain slot for a Plex user's own plex.tv token (their Watchlist:
/// Flick's favourites on Plex). Keyed by plex.tv account id, so every
/// connection of that user shares it.
pub(crate) fn plex_user_key(user_id: &str) -> String {
    format!("plex-user:{user_id}")
}

/// Best-effort: without it the user only loses Plex favourites.
pub(crate) fn store_plex_user_token(user_id: &str, token: &str) {
    if let Err(e) = secrets::store_secret(&plex_user_key(user_id), token) {
        tracing::warn!(target: "provider", "plex.tv token not stored, Plex favourites unavailable: {e}");
    }
}

pub(crate) fn plex_account_token(state: &AppState) -> Option<String> {
    state.plex_account.lock().clone().or_else(|| secrets::load_secret(PLEX_ACCOUNT_KEY).ok().flatten())
}

/// Whose connections `register_plex` adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlexUser {
    /// The signed-in plex.tv account: its connections join the active
    /// profile (modes A/C).
    Account,
    /// A Plex Home member reached by a profile switch: its connections do
    /// not join the *previous* profile (see `register_server_with`) and are
    /// marked `home_member`.
    HomeMember,
}

/// Registers `user`'s connections to the Plex servers `keep` selects, with
/// the access tokens plex.tv gives that user.
pub(crate) async fn register_plex(
    state: &AppState,
    token: &str,
    user: &UserProfile,
    keep: &(dyn Fn(&oneshot_plex::DiscoveredServer) -> bool + Send + Sync),
    who: PlexUser,
) -> Result<Vec<ServerDescriptor>> {
    let auth = PlexAuth::new(state.http(), state.plex_identity());
    let mut added = Vec::new();
    for server in auth.discover(token).await?.into_iter().filter(|s| keep(s)) {
        let base_url = require_reachable(&server)?;
        let d = ServerDescriptor {
            id: ServerId::new(),
            kind: ProviderKind::Plex,
            name: server.name.clone(),
            remote_id: server.machine_id.clone(),
            base_url,
            alternate_urls: server.reachable.iter().skip(1).cloned().collect(),
            version: server.version.clone(),
            user: UserProfile { is_admin: server.owned, ..user.clone() },
            disabled: false,
            home_member: who == PlexUser::HomeMember,
        };
        added.push(state.register_server_with(d, &server.access_token, who == PlexUser::Account)?);
    }
    Ok(added)
}

#[tauri::command]
pub async fn plex_add_servers(state: St<'_>, machine_ids: Vec<String>) -> Result<Vec<ServerDescriptor>> {
    let token = plex_account_token(&state).ok_or(Error::Unauthorized)?;
    let account = PlexAuth::new(state.http(), state.plex_identity()).account(&token).await?;
    // Before the connections, so their providers get the Watchlist.
    store_plex_user_token(&account.user_id, &token);
    let user = UserProfile { id: account.user_id, name: account.username, avatar: account.avatar, is_admin: false };
    register_plex(&state, &token, &user, &|s| machine_ids.contains(&s.machine_id), PlexUser::Account).await
}
