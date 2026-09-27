//! Application state shared by commands.

use std::sync::Arc;

use oneshot_capabilities::CapabilityManager;
use oneshot_catalog::Catalog;
use oneshot_core::provider::MediaProvider;
use oneshot_core::server::{ProviderKind, ServerDescriptor};
use oneshot_core::settings::Settings;
use oneshot_core::{Result, ServerId};
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

    /// Reconnects every stored server at startup. Missing tokens are not an
    /// error: the server is listed as needing sign-in.
    pub fn restore_servers(&self) {
        let servers = self.servers.read().clone();
        for d in servers {
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
