//! Application state shared by commands.

use std::sync::Arc;

use oneshot_capabilities::CapabilityManager;
use oneshot_catalog::Catalog;
use oneshot_core::provider::MediaProvider;
use oneshot_core::server::{ProviderKind, ServerDescriptor};
use oneshot_core::settings::Settings;
use oneshot_core::{Error, Result, ServerId};
use oneshot_net::reqwest::Client;
use oneshot_player::Player;
use oneshot_storage::images::ImageCache;
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
                Arc::new(oneshot_plex::PlexProvider::new(d.clone(), self.http(), self.plex_identity(), token, owned))
            }
        }
    }

    /// Persists a new/updated server, its token, and connects it.
    pub fn register_server(&self, d: ServerDescriptor, token: &str) -> Result<()> {
        secrets::store_token(d.id, token)?;
        {
            let mut servers = self.servers.write();
            servers.retain(|s| s.id != d.id && !(s.kind == d.kind && s.remote_id == d.remote_id && s.user.id == d.user.id));
            servers.push(d.clone());
            self.store.save_servers(&servers)?;
        }
        self.catalog.add(self.build_provider(&d, token.to_owned()));
        tracing::info!(target: "provider", server = %d.name, kind = ?d.kind, "server connected");
        Ok(())
    }

    pub fn remove_server(&self, id: ServerId) -> Result<()> {
        self.catalog.remove(id);
        secrets::delete_token(id)?;
        let mut servers = self.servers.write();
        servers.retain(|s| s.id != id);
        self.store.save_servers(&servers)
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
        if enabled {
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

    /// Reconnects every stored server at startup. Missing tokens are not an
    /// error: the server is listed as needing sign-in.
    pub fn restore_servers(&self) {
        let servers = self.servers.read().clone();
        for d in servers {
            if d.disabled {
                tracing::info!(target: "provider", server = %d.name, "server disabled; not connecting");
                continue;
            }
            match secrets::load_token(d.id) {
                Ok(Some(token)) => self.catalog.add(self.build_provider(&d, token)),
                Ok(None) => tracing::warn!(target: "provider", server = %d.name, "no stored token; sign-in required"),
                Err(e) => tracing::error!(target: "provider", server = %d.name, "credential store error: {e}"),
            }
        }
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
