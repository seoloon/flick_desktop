//! Application state shared by commands.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use oneshot_capabilities::CapabilityManager;
use oneshot_catalog::Catalog;
use oneshot_core::profile::{Origin, Profile, ProfileId, ProfileMode, ProfilesConfig};
use oneshot_core::provider::MediaProvider;
use oneshot_core::server::{ProviderKind, ServerDescriptor};
use oneshot_core::settings::{PersonalSettings, Settings};
use oneshot_core::{Error, Result, ServerId};
use oneshot_net::reqwest::Client;
use oneshot_player::Player;
use oneshot_storage::images::ImageCache;
use oneshot_storage::pin::{self, PinGuard};
use oneshot_storage::profiles::{self, Resolved};
use oneshot_storage::{Identity, Store, secrets};
use parking_lot::{Mutex, RwLock};

use crate::diagnostics::Diagnostics;

pub const APP_NAME: &str = "Flick";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Changes the global log level at runtime (Settings > Debug).
pub type LogReload = Arc<dyn Fn(&str) + Send + Sync>;

pub struct AppState {
    pub store: Store,
    pub identity: Identity,
    pub settings: RwLock<Settings>,
    pub http: RwLock<Client>,
    pub catalog: Catalog,
    pub caps: CapabilityManager,
    pub images: ImageCache,
    pub player: Player,
    pub diagnostics: Diagnostics,
    pub log_reload: LogReload,
    /// Descriptors of configured servers, including ones whose token is
    /// missing (shown as "sign in again").
    pub servers: RwLock<Vec<ServerDescriptor>>,
    /// plex.tv account token between PIN approval and server selection.
    pub plex_account: Mutex<Option<String>>,
    /// Window geometry to restore when leaving picture-in-picture.
    pub pip_restore: Mutex<Option<WindowRestore>>,
    /// `profiles.json`: multi-user mode, profiles, last discovery.
    pub profiles: RwLock<ProfilesConfig>,
    /// The profile in use (multi-user on); `None` until someone is picked.
    pub active_profile: RwLock<Option<ProfileId>>,
    /// Failed PIN attempts, per profile, for this run.
    pub pin_guards: Mutex<HashMap<ProfileId, PinGuard>>,
    /// Failed PIN attempts on changes that could bypass PINs (turning
    /// multi-user off, changing mode), for this run.
    pub config_guard: Mutex<PinGuard>,
    /// Connections whose server did not answer the last discovery.
    pub offline: RwLock<HashSet<ServerId>>,
    /// Connection ids of protected Plex members verified by plex.tv in this
    /// run.
    pub verified_plex: RwLock<HashSet<ServerId>>,
    /// One profile switch at a time.
    pub switching: tokio::sync::Mutex<()>,
    /// One settings save at a time (`settings_set` runs off the main thread).
    pub settings_io: Mutex<()>,
    /// TMDB client for person pages; `None` without a key (Settings ›
    /// Metadata). The key lives in the OS keychain only.
    pub tmdb: RwLock<Option<oneshot_tmdb::Tmdb>>,
    /// The catalogue's metadata cache, shared with TMDB answers (a week).
    pub metadata: Arc<oneshot_storage::cache::MetadataCache>,
    /// Watch together (optional: the app works the same without it).
    pub flicksync: crate::flicksync::Hub,
    /// Offline downloads through Flick Server (FlickDD).
    pub downloads: crate::downloads::Downloads,
    /// Chromecast / AirPlay receivers and the one being cast to.
    pub cast: oneshot_cast::Caster,
}

/// How the main window looked before it shrank into picture-in-picture.
#[derive(Debug, Clone, Copy)]
pub struct WindowRestore {
    pub position: tauri::PhysicalPosition<i32>,
    /// Inner size: what `set_size` takes back.
    pub size: tauri::PhysicalSize<u32>,
    pub maximized: bool,
    pub fullscreen: bool,
}

impl AppState {
    pub fn settings(&self) -> Settings {
        self.settings.read().clone()
    }

    pub fn http(&self) -> Client {
        self.http.read().clone()
    }

    /// Name of the profile in use (multi-user on), for display to others.
    pub fn active_profile_name(&self) -> Option<String> {
        let id = (*self.active_profile.read())?;
        self.profiles.read().profiles.iter().find(|p| p.id == id).map(|p| p.name.clone())
    }

    pub fn jellyfin_identity(&self) -> oneshot_jellyfin::ClientIdentity {
        oneshot_jellyfin::ClientIdentity {
            client: APP_NAME.into(),
            device_name: self.identity.device_name.clone(),
            device_id: self.identity.device_id.clone(),
            version: VERSION.into(),
        }
    }

    pub fn plex_identity(&self) -> oneshot_plex::PlexIdentity {
        oneshot_plex::PlexIdentity {
            product: APP_NAME.into(),
            version: VERSION.into(),
            client_identifier: self.identity.device_id.clone(),
            device_name: self.identity.device_name.clone(),
            platform: platform_name().into(),
        }
    }

    /// Instantiates the provider for a stored descriptor with its token.
    pub fn build_provider(&self, d: &ServerDescriptor, token: String) -> Arc<dyn MediaProvider> {
        match d.kind {
            ProviderKind::Jellyfin => {
                Arc::new(oneshot_jellyfin::JellyfinProvider::new(d.clone(), self.http(), self.jellyfin_identity(), token))
            }
            ProviderKind::Plex => {
                let owned = d.user.is_admin;
                let provider = oneshot_plex::PlexProvider::new(d.clone(), self.http(), self.plex_identity(), token, owned);
                Arc::new(match self.plex_watchlist(d) {
                    Some(w) => provider.with_watchlist(w),
                    None => provider,
                })
            }
        }
    }

    /// The plex.tv Watchlist of a Plex connection's user: their own token,
    /// else (connections made before per-user tokens) the signed-in
    /// account's, used only once plex.tv confirms it is this user's.
    fn plex_watchlist(&self, d: &ServerDescriptor) -> Option<oneshot_plex::Watchlist> {
        use crate::commands::servers::{PLEX_ACCOUNT_KEY, plex_user_key};
        let watchlist = |token| oneshot_plex::Watchlist::new(self.http(), self.plex_identity(), token);
        match secrets::load_secret(&plex_user_key(&d.user.id)) {
            Ok(Some(token)) => Some(watchlist(token)),
            _ if !d.home_member => secrets::load_secret(PLEX_ACCOUNT_KEY).ok().flatten().map(|t| watchlist(t).for_user(d.user.id.clone())),
            _ => None,
        }
    }

    /// The profiles of the current mode. Derived profiles appearing for the
    /// first time are saved.
    pub fn resolved_profiles(&self) -> Vec<Resolved> {
        let servers = self.servers.read().clone();
        let defaults = || PersonalSettings::from_settings(&self.store.settings());
        let mut cfg = self.profiles.write();
        let before = cfg.profiles.len();
        let out = profiles::resolve(&mut cfg, &servers, &defaults);
        if cfg.profiles.len() != before
            && let Err(e) = self.store.save_profiles(&cfg)
        {
            tracing::warn!(target: "storage", "profiles not saved: {e}");
        }
        out
    }

    /// Connections the catalogue should hold: `None` = all (multi-user off),
    /// empty = nobody picked yet.
    pub fn active_connections(&self) -> Option<HashSet<ServerId>> {
        if !self.profiles.read().enabled {
            return None;
        }
        let active = *self.active_profile.read();
        let Some(id) = active else { return Some(HashSet::new()) };
        Some(
            self.resolved_profiles()
                .iter()
                .find(|r| r.profile.id == id)
                .map(|r| profiles::connections_of(r).into_iter().collect())
                .unwrap_or_default(),
        )
    }

    /// Connections the active profile owns, disabled ones included: `None` =
    /// multi-user off (all of them), empty = nobody picked yet.
    pub fn active_members(&self) -> Option<HashSet<ServerId>> {
        if !self.profiles.read().enabled {
            return None;
        }
        let active = *self.active_profile.read();
        let Some(id) = active else { return Some(HashSet::new()) };
        Some(
            self.resolved_profiles()
                .iter()
                .find(|r| r.profile.id == id)
                .map(|r| r.accounts.iter().filter_map(|a| a.connection.as_ref().map(|d| d.id)).collect())
                .unwrap_or_default(),
        )
    }

    /// Connections the catalogue may hold now: the active profile's (all of
    /// them with multi-user off), minus protected Plex users plex.tv has not
    /// checked in this run (see `profiles::loadable`).
    fn loadable(&self) -> HashSet<ServerId> {
        let wanted = self.active_connections();
        let servers = self.servers.read().clone();
        let (discovered, enabled) = {
            let cfg = self.profiles.read();
            (cfg.discovered.clone(), cfg.enabled)
        };
        let verified = self.verified_plex.read().clone();
        profiles::loadable(&servers, &discovered, wanted.as_ref(), &verified, enabled).into_iter().collect()
    }

    fn wanted(&self, id: ServerId) -> bool {
        self.loadable().contains(&id)
    }

    pub fn update_profile(&self, id: ProfileId, f: impl FnOnce(&mut Profile)) -> Result<()> {
        let mut cfg = self.profiles.write();
        let p = cfg.profiles.iter_mut().find(|p| p.id == id).ok_or_else(|| Error::NotFound(format!("profile {id}")))?;
        f(p);
        self.store.save_profiles(&cfg)
    }

    /// Checks `given` against the profile's PIN (open profiles pass).
    pub fn check_pin(&self, id: ProfileId, given: Option<&str>) -> Result<()> {
        let hash = self
            .profiles
            .read()
            .profiles
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error::NotFound(format!("profile {id}")))?
            .pin
            .clone();
        let mut guards = self.pin_guards.lock();
        pin::check_pin(hash.as_deref(), given, guards.entry(id).or_default(), Instant::now())
    }

    pub fn apply_effective_settings(&self, effective: Settings) {
        self.player.apply_settings(&effective);
        *self.settings.write() = effective;
    }

    /// Makes `id` the active profile: its preferences, its connections.
    /// Returns the names of connections that could not be loaded.
    pub fn activate_profile(&self, id: ProfileId) -> Result<Vec<String>> {
        let resolved = self.resolved_profiles();
        let r = resolved.iter().find(|r| r.profile.id == id).ok_or_else(|| Error::NotFound(format!("profile {id}")))?;
        *self.active_profile.write() = Some(id);
        {
            let mut cfg = self.profiles.write();
            cfg.last_profile = Some(id);
            // Best effort: only the next start's resume depends on it.
            if let Err(e) = self.store.save_profiles(&cfg) {
                tracing::warn!(target: "storage", "last profile not saved: {e}");
            }
        }
        let mut effective = self.store.settings();
        r.profile.prefs.apply(&mut effective);
        self.apply_effective_settings(effective);
        self.restore_servers();
        let live: HashSet<ServerId> = self.catalog.servers().iter().map(|p| p.descriptor().id).collect();
        tracing::info!(target: "provider", profile = %r.profile.name, "profile active");
        Ok(r.accounts
            .iter()
            .filter_map(|a| a.connection.as_ref())
            .filter(|d| !d.disabled && !live.contains(&d.id))
            .map(|d| d.name.clone())
            .collect())
    }

    /// Multi-user on, picker not wanted at startup: resume the last profile
    /// when nothing has to be typed. Returns whether a profile was activated.
    pub fn resume_last_profile(&self) -> bool {
        let cfg = self.profiles.read().clone();
        if !cfg.enabled || cfg.ask_on_startup {
            return false;
        }
        let Some(id) = cfg.last_profile else { return false };
        let Some(r) = self.resolved_profiles().into_iter().find(|r| r.profile.id == id) else { return false };
        let needs_typing = r.profile.pin.is_some() || r.accounts.iter().any(|a| a.discovered.as_ref().is_some_and(|u| u.protected));
        if needs_typing {
            return false;
        }
        match self.activate_profile(id) {
            Ok(_) => true,
            Err(e) => {
                tracing::warn!(target: "provider", "last profile not resumed: {e}");
                false
            }
        }
    }

    /// Modes A/C: a connection added while a profile is active belongs to it,
    /// unless another PIN-protected profile already uses it (linking that one
    /// asks the owner's PIN, from the profile sheet).
    fn attach_to_active(&self, id: ServerId) -> Result<()> {
        let (enabled, mode) = {
            let cfg = self.profiles.read();
            (cfg.enabled, cfg.mode)
        };
        let active = *self.active_profile.read();
        match active {
            Some(pid) if enabled && mode != ProfileMode::ServerUsers => {
                // Bound first: `update_profile` takes the `profiles` write guard.
                let owned = !profiles::locked_owners(&self.profiles.read().profiles, pid, &[id]).is_empty();
                if owned {
                    tracing::info!(target: "provider", server = %id, "connection belongs to a protected profile; link it from the profile sheet");
                    return Ok(());
                }
                self.update_profile(pid, |p| {
                    if p.origin == Origin::Manual && !p.connections.contains(&id) {
                        p.connections.push(id);
                    }
                })
            }
            _ => Ok(()),
        }
    }

    /// Persists a new/updated connection and its token, and connects it when
    /// the active profile uses it. Signing in again as the same user on the
    /// same server keeps the connection's id (profiles and caches refer to it).
    pub fn register_server(&self, d: ServerDescriptor, token: &str) -> Result<ServerDescriptor> {
        self.register_server_with(d, token, true)
    }

    /// Like `register_server`, but `attach` controls whether the connection
    /// joins the currently active profile (modes A/C). A profile switch that
    /// signs a member in while the *previous* profile is still active passes
    /// `false`, since `activate_profile` makes the new one active afterwards.
    pub fn register_server_with(&self, mut d: ServerDescriptor, token: &str, attach: bool) -> Result<ServerDescriptor> {
        {
            let mut servers = self.servers.write();
            if let Some(existing) = servers.iter().find(|s| s.kind == d.kind && s.remote_id == d.remote_id && s.user.id == d.user.id) {
                d = d.merged_with(existing);
            }
            secrets::store_token(d.id, token)?;
            servers.retain(|s| s.id != d.id);
            servers.push(d.clone());
            self.store.save_servers(&servers)?;
        }
        if attach {
            self.attach_to_active(d.id)?;
        }
        if !d.disabled && self.wanted(d.id) {
            self.catalog.add(self.build_provider(&d, token.to_owned()));
        }
        tracing::info!(target: "provider", server = %d.name, kind = ?d.kind, "server connected");
        Ok(d)
    }

    pub fn remove_server(&self, id: ServerId) -> Result<()> {
        self.catalog.remove(id);
        secrets::delete_token(id)?;
        {
            let mut servers = self.servers.write();
            servers.retain(|s| s.id != id);
            self.store.save_servers(&servers)?;
        }
        let mut cfg = self.profiles.write();
        for p in &mut cfg.profiles {
            p.connections.retain(|c| *c != id);
            p.detached.retain(|c| *c != id);
        }
        self.store.save_profiles(&cfg)
    }

    /// Turns a server on or off. Off keeps it and its token but takes it out
    /// of the catalogue; on reconnects it with the stored token.
    pub fn set_server_enabled(&self, id: ServerId, enabled: bool) -> Result<()> {
        let d = {
            let mut servers = self.servers.write();
            let s = servers.iter_mut().find(|s| s.id == id).ok_or_else(|| Error::NotFound(format!("server {id}")))?;
            s.disabled = !enabled;
            let d = s.clone();
            self.store.save_servers(&servers)?;
            d
        };
        if enabled && self.wanted(id) {
            match secrets::load_token(id)? {
                Some(token) => self.catalog.add(self.build_provider(&d, token)),
                None => tracing::warn!(target: "provider", server = %d.name, "enabled without a stored token; sign-in required"),
            }
        } else {
            self.catalog.remove(id);
        }
        tracing::info!(target: "provider", server = %d.name, enabled, "server toggled");
        Ok(())
    }

    /// Connects the stored connections the catalogue should hold (all of
    /// them with multi-user off, the active profile's otherwise; see
    /// `loadable`). Missing tokens are not an error: the server is listed as
    /// needing sign-in.
    pub fn restore_servers(&self) {
        let loadable = self.loadable();
        let servers = self.servers.read().clone();
        let mut providers = Vec::new();
        for d in servers {
            if d.disabled {
                tracing::info!(target: "provider", server = %d.name, "server disabled; not connecting");
                continue;
            }
            if !loadable.contains(&d.id) {
                continue;
            }
            match secrets::load_token(d.id) {
                Ok(Some(token)) => providers.push(self.build_provider(&d, token)),
                Ok(None) => tracing::warn!(target: "provider", server = %d.name, "no stored token; sign-in required"),
                Err(e) => tracing::error!(target: "provider", server = %d.name, "credential store error: {e}"),
            }
        }
        self.catalog.replace(providers);
    }
}

fn platform_name() -> &'static str {
    if cfg!(windows) {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Linux"
    }
}
