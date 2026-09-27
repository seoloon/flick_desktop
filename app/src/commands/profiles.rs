//! Multi-user profiles: picker/settings state, switching, discovery of
//! server users (mode B) and profile edits. Rules live in
//! `oneshot_storage::{profiles, pin}`; this only wires them.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use oneshot_core::profile::{AvatarStyle, DiscoveredUser, Origin, Profile, ProfileId, ProfileMode, ProfilesState};
use oneshot_core::server::{ProviderKind, UserProfile};
use oneshot_core::settings::PersonalSettings;
use oneshot_core::{Error, Result, ServerId};
use oneshot_jellyfin::Connector;
use oneshot_plex::PlexAuth;
use oneshot_storage::{pin, profiles};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use super::servers::{PlexUser, jellyfin_descriptor, plex_account_token, register_plex};
use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

/// Per server: a slow one never holds the picker.
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) fn snapshot(state: &AppState) -> ProfilesState {
    let cfg = state.profiles.read().clone();
    let offline = state.offline.read().clone();
    let resolved = if cfg.enabled { state.resolved_profiles() } else { Vec::new() };
    ProfilesState {
        enabled: cfg.enabled,
        mode: cfg.mode,
        ask_on_startup: cfg.ask_on_startup,
        active: *state.active_profile.read(),
        profiles: resolved.iter().map(|r| profiles::card(r, &offline)).collect(),
        any_locked: cfg.profiles.iter().any(|p| p.pin.is_some()),
    }
}

#[tauri::command]
pub fn profiles_state(state: St<'_>) -> ProfilesState {
    snapshot(&state)
}

/// Lists server users (mode B): Jellyfin sign-in screens and the Plex Home.
/// A server that does not answer keeps its last list and shows offline.
/// Jellyfin and Plex are probed concurrently, not one after the other.
#[tauri::command]
pub async fn profiles_discover(state: St<'_>) -> Result<ProfilesState> {
    let servers = state.servers.read().clone();
    let previous = state.profiles.read().discovered.clone();
    let mut found: Vec<DiscoveredUser> = Vec::new();
    let mut offline: HashSet<ServerId> = HashSet::new();
    let mut kept: HashSet<ServerId> = HashSet::new();

    let mut seen = HashSet::new();
    let jellyfins: Vec<_> = servers.iter().filter(|s| s.kind == ProviderKind::Jellyfin && seen.insert(s.remote_id.clone())).cloned().collect();
    let connector = Connector::new(state.http(), state.jellyfin_identity());
    let jellyfin_probe = async {
        let probes = jellyfins.iter().map(|home| {
            let connector = connector.clone();
            async move { (home, tokio::time::timeout(DISCOVERY_TIMEOUT, connector.public_users(&home.base_url)).await) }
        });
        futures::future::join_all(probes).await
    };

    let mut seen = HashSet::new();
    let plexes: Vec<_> = servers.iter().filter(|s| s.kind == ProviderKind::Plex && seen.insert(s.remote_id.clone())).cloned().collect();
    // No token is treated like a failed probe: the last list is kept and the
    // group shows offline, rather than silently dropping (and losing the
    // `protected` flag of) every Plex Home member.
    let plex_token = if plexes.is_empty() { None } else { plex_account_token(&state) };
    let plex_probe = async {
        let token = plex_token.as_ref()?;
        let auth = PlexAuth::new(state.http(), state.plex_identity());
        Some(tokio::time::timeout(DISCOVERY_TIMEOUT, auth.home_users(token)).await)
    };

    let (jellyfin_results, plex_result) = tokio::join!(jellyfin_probe, plex_probe);

    for (home, res) in jellyfin_results {
        match res {
            Ok(Ok(users)) => found.extend(users.into_iter().map(|u| DiscoveredUser {
                server: home.id,
                kind: ProviderKind::Jellyfin,
                remote_user_id: u.id,
                switch_id: None,
                name: u.name,
                avatar: u.avatar,
                has_password: u.has_password,
                protected: false,
            })),
            _ => {
                tracing::info!(target: "provider", server = %home.name, "user discovery failed; keeping the last list");
                kept.insert(home.id);
                offline.extend(servers.iter().filter(|s| s.kind == home.kind && s.remote_id == home.remote_id).map(|s| s.id));
            }
        }
    }

    if !plexes.is_empty() {
        match plex_result {
            Some(Ok(Ok(members))) => {
                for home in &plexes {
                    found.extend(members.iter().map(|m| DiscoveredUser {
                        server: home.id,
                        kind: ProviderKind::Plex,
                        remote_user_id: m.id.clone(),
                        switch_id: Some(m.uuid.clone()),
                        name: m.name.clone(),
                        avatar: m.avatar.clone(),
                        has_password: false,
                        protected: m.protected,
                    }));
                }
            }
            _ => {
                tracing::info!(target: "provider", "Plex Home discovery failed; keeping the last list");
                kept.extend(plexes.iter().map(|h| h.id));
                offline.extend(servers.iter().filter(|s| s.kind == ProviderKind::Plex && plexes.iter().any(|h| h.remote_id == s.remote_id)).map(|s| s.id));
            }
        }
    }

    found.extend(previous.into_iter().filter(|u| kept.contains(&u.server)));
    {
        let mut cfg = state.profiles.write();
        cfg.discovered = found;
        state.store.save_profiles(&cfg)?;
    }
    *state.offline.write() = offline;
    Ok(snapshot(&state))
}

fn os_user_name() -> String {
    std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "Me".into())
}

#[tauri::command]
pub fn profiles_configure(state: St<'_>, enabled: bool, mode: ProfileMode, ask_on_startup: bool, pin: Option<String>) -> Result<ProfilesState> {
    // Cloned so the read lock is not held while `authorize_config_change_guarded`
    // hashes the PIN (argon2 is slow; no lock should be held across it).
    let cfg = state.profiles.read().clone();
    let (was_enabled, old_mode) = (cfg.enabled, cfg.mode);
    pin::authorize_config_change_guarded(&cfg, enabled, mode, pin.as_deref(), &mut state.config_guard.lock(), std::time::Instant::now())?;
    {
        let mut cfg = state.profiles.write();
        cfg.enabled = enabled;
        cfg.mode = mode;
        cfg.ask_on_startup = ask_on_startup;
        // Modes A/C start with one profile holding what is configured today.
        if enabled && mode != ProfileMode::ServerUsers && !cfg.profiles.iter().any(|p| p.origin == Origin::Manual) {
            let mut first = Profile::new(os_user_name(), profiles::PROFILE_COLORS[0], Origin::Manual, PersonalSettings::from_settings(&state.store.settings()));
            first.connections = state.servers.read().iter().map(|s| s.id).collect();
            cfg.profiles.push(first);
        }
        state.store.save_profiles(&cfg)?;
    }
    if enabled != was_enabled || mode != old_mode {
        // Plex PINs are checked again for whoever is picked next.
        state.verified_plex.write().clear();
    }
    if !enabled {
        *state.active_profile.write() = None;
        state.apply_effective_settings(state.store.settings());
        state.restore_servers();
    } else if !was_enabled || mode != old_mode {
        // Keep the person where they are: enter the profile holding what is loaded.
        let loaded: HashSet<ServerId> = state.catalog.providers().iter().map(|p| p.descriptor().id).collect();
        match profiles::best_match(&state.resolved_profiles(), &loaded) {
            Some(id) => {
                state.activate_profile(id)?;
            }
            None => {
                // Nobody picked yet: shared settings, nothing loaded until the picker.
                *state.active_profile.write() = None;
                state.apply_effective_settings(state.store.settings());
                state.restore_servers();
            }
        }
    }
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn profile_check_pin(state: St<'_>, id: ProfileId, pin: String) -> Result<()> {
    state.check_pin(id, Some(&pin))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchOutcome {
    /// Servers of the profile that could not be connected.
    pub failed: Vec<String>,
}

#[tauri::command]
pub async fn profile_switch(app: tauri::AppHandle, state: St<'_>, id: ProfileId, pin: Option<String>, plex_pin: Option<String>) -> Result<SwitchOutcome> {
    let _busy = state.switching.try_lock().map_err(|_| Error::Invalid("a profile switch is already in progress".into()))?;
    state.check_pin(id, pin.as_deref())?;
    let resolved = state
        .resolved_profiles()
        .into_iter()
        .find(|r| r.profile.id == id)
        .ok_or_else(|| Error::NotFound(format!("profile {id}")))?;
    let configured_plex: HashSet<String> =
        state.servers.read().iter().filter(|s| s.kind == ProviderKind::Plex).map(|s| s.remote_id.clone()).collect();
    let mut failed = Vec::new();
    // The Plex PIN is asked at every change: what the previous profile had
    // verified is replaced (not merged) once this switch goes through.
    let mut verified = HashSet::new();

    for account in &resolved.accounts {
        let Some(user) = &account.discovered else { continue };
        match (user.kind, &account.connection) {
            // Plex Home: members not signed in yet, and protected ones every
            // time (plex.tv checks their PIN; unverified ones never load, see
            // `profiles::loadable`).
            (ProviderKind::Plex, conn) if conn.is_none() || user.protected => {
                let Some(uuid) = &user.switch_id else {
                    if user.protected {
                        failed.push(account.server_name.clone());
                    }
                    continue;
                };
                let member_pin = match (user.protected, plex_pin.as_deref()) {
                    (false, _) => None,
                    (true, Some(p)) => Some(p),
                    // Skipped: unavailable, and plex.tv is not asked.
                    (true, None) => {
                        failed.push(account.server_name.clone());
                        continue;
                    }
                };
                let Some(token) = plex_account_token(&state) else {
                    failed.push(account.server_name.clone());
                    continue;
                };
                let auth = PlexAuth::new(state.http(), state.plex_identity());
                match auth.switch_user(&token, uuid, member_pin).await {
                    Ok(member_token) => {
                        verified.extend(conn.as_ref().map(|c| c.id));
                        let who = UserProfile { id: user.remote_user_id.clone(), name: user.name.clone(), avatar: user.avatar.clone(), is_admin: false };
                        match register_plex(&state, &member_token, &who, &|s| configured_plex.contains(&s.machine_id), PlexUser::HomeMember).await {
                            Ok(added) => verified.extend(added.iter().map(|d| d.id)),
                            Err(e) => {
                                tracing::warn!(target: "provider", server = %account.server_name, "Plex member not connected: {e}");
                                failed.push(account.server_name.clone());
                            }
                        }
                    }
                    // plex.tv refuses the switch: a wrong PIN, unless the
                    // account's own sign-in has expired.
                    Err(Error::Unauthorized | Error::Forbidden(_)) if user.protected => {
                        if let Err(e) = auth.account(&token).await {
                            tracing::warn!(target: "provider", server = %account.server_name, "plex.tv sign-in expired: {e}");
                            failed.push(account.server_name.clone());
                            continue;
                        }
                        return Err(Error::WrongPin);
                    }
                    Err(e) => {
                        tracing::warn!(target: "provider", server = %account.server_name, "Plex switch failed: {e}");
                        failed.push(account.server_name.clone());
                    }
                }
            }
            // Jellyfin users without a password sign in on first use.
            (ProviderKind::Jellyfin, None) if !user.has_password => {
                let connector = Connector::new(state.http(), state.jellyfin_identity());
                match connector.login(&account.base_url, &user.name, "").await {
                    Ok(session) => {
                        state.register_server_with(jellyfin_descriptor(&session), &session.token, false)?;
                    }
                    Err(e) => {
                        tracing::warn!(target: "provider", server = %account.server_name, "Jellyfin sign-in failed: {e}");
                        failed.push(account.server_name.clone());
                    }
                }
            }
            _ => {}
        }
    }

    *state.verified_plex.write() = verified;
    failed.extend(state.activate_profile(id)?);
    failed.sort();
    failed.dedup();
    let _ = app.emit("profile-changed", id);
    Ok(SwitchOutcome { failed })
}

#[tauri::command]
pub fn profile_create(state: St<'_>, name: String, color: String) -> Result<ProfileId> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(Error::Invalid("a profile needs a name".into()));
    }
    let p = Profile::new(name, color, Origin::Manual, PersonalSettings::from_settings(&state.store.settings()));
    let id = p.id;
    let mut cfg = state.profiles.write();
    cfg.profiles.push(p);
    state.store.save_profiles(&cfg)?;
    Ok(id)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileEdit {
    pub name: Option<String>,
    pub color: Option<String>,
    pub avatar: Option<AvatarStyle>,
    pub connections: Option<Vec<ServerId>>,
    pub hidden: Option<bool>,
}

/// `owner_pin`: linking a connection that another PIN-protected profile uses
/// (modes A/C) needs that profile's PIN.
#[tauri::command]
pub fn profile_update(state: St<'_>, id: ProfileId, edit: ProfileEdit, pin: Option<String>, owner_pin: Option<String>) -> Result<()> {
    state.check_pin(id, pin.as_deref())?;
    if let Some(next) = &edit.connections {
        // Bound first: no `profiles` guard may be held while `check_pin` runs.
        let owners = profiles::locked_owners(&state.profiles.read().profiles, id, next);
        for owner in owners {
            state.check_pin(owner, owner_pin.as_deref())?;
        }
    }
    let mut reload = false;
    state.update_profile(id, |p| {
        if let Some(n) = edit.name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()) {
            p.name = n;
        }
        if let Some(c) = edit.color {
            p.color = c;
        }
        if let Some(a) = edit.avatar {
            p.avatar = a;
        }
        if let Some(h) = edit.hidden {
            p.hidden = h;
        }
        if let Some(c) = edit.connections
            && p.origin == Origin::Manual
        {
            p.connections = c;
            reload = true;
        }
    })?;
    if reload && *state.active_profile.read() == Some(id) {
        state.restore_servers();
    }
    Ok(())
}

#[tauri::command]
pub fn profile_set_pin(state: St<'_>, id: ProfileId, current: Option<String>, next: Option<String>) -> Result<()> {
    state.check_pin(id, current.as_deref())?;
    let hash = next.as_deref().map(pin::hash_pin).transpose()?;
    state.update_profile(id, |p| p.pin = hash)
}

#[tauri::command]
pub fn profile_detach(state: St<'_>, id: ProfileId, connection: ServerId, pin: Option<String>) -> Result<()> {
    state.check_pin(id, pin.as_deref())?;
    let owns = state
        .resolved_profiles()
        .into_iter()
        .find(|r| r.profile.id == id)
        .is_some_and(|r| r.accounts.iter().any(|a| a.connection.as_ref().is_some_and(|d| d.id == connection)));
    if !owns {
        return Err(Error::Invalid("that account is not part of this profile".into()));
    }
    state.update_profile(id, |p| {
        if !p.detached.contains(&connection) {
            p.detached.push(connection);
        }
    })?;
    if *state.active_profile.read() == Some(id) {
        state.restore_servers();
    }
    Ok(())
}

#[tauri::command]
pub fn profile_delete(state: St<'_>, id: ProfileId, pin: Option<String>) -> Result<Vec<ServerId>> {
    state.check_pin(id, pin.as_deref())?;
    let (removed, used) = {
        let mut cfg = state.profiles.write();
        let idx = cfg.profiles.iter().position(|p| p.id == id).ok_or_else(|| Error::NotFound(format!("profile {id}")))?;
        if cfg.profiles[idx].origin != Origin::Manual {
            return Err(Error::Invalid("profiles made from server users are hidden, not deleted".into()));
        }
        let removed = cfg.profiles.remove(idx);
        if cfg.last_profile == Some(id) {
            cfg.last_profile = None;
        }
        state.store.save_profiles(&cfg)?;
        let used: HashSet<ServerId> = cfg.profiles.iter().filter(|p| p.origin == Origin::Manual).flat_map(|p| p.connections.iter().copied()).collect();
        (removed, used)
    };
    if *state.active_profile.read() == Some(id) {
        *state.active_profile.write() = None;
        state.restore_servers();
    }
    Ok(removed.connections.into_iter().filter(|c| !used.contains(c)).collect())
}
