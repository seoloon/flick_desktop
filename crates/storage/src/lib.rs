//! Local persistence.
//!
//! * `settings.json`, `servers.json`, `identity.json`, `profiles.json` — small documents,
//!   written atomically (temp file + rename).
//! * Tokens — OS keychain only ([`secrets`]). Nothing secret touches disk.
//! * `cache.sqlite` — metadata cache with TTL ([`cache::MetadataCache`]).
//! * `images/` — size-bucketed artwork cache with an LRU cap ([`images`]).

pub mod cache;
pub mod images;
pub mod pin;
pub mod profiles;
pub mod secrets;

use std::path::{Path, PathBuf};

use oneshot_core::profile::ProfilesConfig;
use oneshot_core::server::ServerDescriptor;
use oneshot_core::settings::Settings;
use oneshot_core::{Error, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Where each kind of data lives (resolved by the shell from OS conventions).
#[derive(Debug, Clone)]
pub struct Paths {
    pub config: PathBuf,
    pub cache: PathBuf,
}

/// Stable installation identity sent to servers (Jellyfin DeviceId, Plex
/// client identifier). Regenerating it would orphan server-side sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub device_id: String,
    pub device_name: String,
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| Error::Storage(format!("{}: {e}", path.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::Storage(format!("{}: {e}", path.display()))),
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let dir = path.parent().ok_or_else(|| Error::Storage("invalid path".into()))?;
    std::fs::create_dir_all(dir).map_err(|e| Error::Storage(e.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| Error::Storage(e.to_string()))?;
    std::fs::write(&tmp, bytes).map_err(|e| Error::Storage(e.to_string()))?;
    std::fs::rename(&tmp, path).map_err(|e| Error::Storage(e.to_string()))
}

#[derive(Debug, Clone)]
pub struct Store {
    paths: Paths,
}

impl Store {
    pub fn open(paths: Paths) -> Result<Self> {
        for dir in [&paths.config, &paths.cache] {
            std::fs::create_dir_all(dir).map_err(|e| Error::Storage(format!("{}: {e}", dir.display())))?;
        }
        Ok(Self { paths })
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    pub fn settings(&self) -> Settings {
        match read_json(&self.paths.config.join("settings.json")) {
            Ok(Some(s)) => s,
            Ok(None) => Settings::default(),
            Err(e) => {
                // Never crash on a corrupt file: keep a copy, start from defaults.
                tracing::error!(target: "storage", "settings unreadable, using defaults: {e}");
                let _ = std::fs::copy(self.paths.config.join("settings.json"), self.paths.config.join("settings.corrupt.json"));
                Settings::default()
            }
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        write_json(&self.paths.config.join("settings.json"), settings)
    }

    pub fn servers(&self) -> Result<Vec<ServerDescriptor>> {
        Ok(read_json(&self.paths.config.join("servers.json"))?.unwrap_or_default())
    }

    pub fn save_servers(&self, servers: &[ServerDescriptor]) -> Result<()> {
        write_json(&self.paths.config.join("servers.json"), &servers)
    }

    pub fn profiles(&self) -> ProfilesConfig {
        let path = self.paths.config.join("profiles.json");
        match read_json(&path) {
            Ok(Some(p)) => p,
            Ok(None) => ProfilesConfig::default(),
            Err(e) => {
                // Never crash on a corrupt file: keep a copy, start with multi-user off.
                tracing::error!(target: "storage", "profiles unreadable, multi-user off: {e}");
                let _ = std::fs::copy(&path, self.paths.config.join("profiles.corrupt.json"));
                ProfilesConfig::default()
            }
        }
    }

    pub fn save_profiles(&self, profiles: &ProfilesConfig) -> Result<()> {
        write_json(&self.paths.config.join("profiles.json"), profiles)
    }

    pub fn identity(&self) -> Result<Identity> {
        let path = self.paths.config.join("identity.json");
        if let Some(id) = read_json::<Identity>(&path)? {
            return Ok(id);
        }
        let id = Identity {
            device_id: uuid::Uuid::new_v4().to_string(),
            device_name: hostname(),
        };
        write_json(&path, &id)?;
        Ok(id)
    }
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "Flick Desktop".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip_and_stable_identity() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(Paths { config: dir.path().join("cfg"), cache: dir.path().join("cache") }).unwrap();
        let mut s = store.settings();
        s.audio.passthrough = true;
        store.save_settings(&s).unwrap();
        assert!(store.settings().audio.passthrough);
        let a = store.identity().unwrap();
        let b = store.identity().unwrap();
        assert_eq!(a.device_id, b.device_id);
    }

    #[test]
    fn corrupt_settings_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(Paths { config: dir.path().into(), cache: dir.path().join("c") }).unwrap();
        std::fs::write(dir.path().join("settings.json"), b"{not json").unwrap();
        assert_eq!(store.settings(), Settings::default());
        assert!(dir.path().join("settings.corrupt.json").exists());
    }

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(Paths { config: dir.path().join("cfg"), cache: dir.path().join("cache") }).unwrap();
        (dir, s)
    }

    #[test]
    fn absent_profiles_file_means_disabled() {
        let (_d, s) = store();
        assert_eq!(s.profiles(), oneshot_core::profile::ProfilesConfig::default());
        assert!(!s.profiles().enabled);
    }

    #[test]
    fn profiles_roundtrip() {
        let (_d, s) = store();
        let mut p = s.profiles();
        p.enabled = true;
        p.profiles.push(oneshot_core::profile::Profile::new("Léa", "#ff6b6b", Default::default(), Default::default()));
        s.save_profiles(&p).unwrap();
        assert_eq!(s.profiles(), p);
    }

    #[test]
    fn corrupt_profiles_file_is_kept_aside() {
        let (_d, s) = store();
        std::fs::write(s.paths().config.join("profiles.json"), b"{ not json").unwrap();
        assert!(!s.profiles().enabled);
        assert!(s.paths().config.join("profiles.corrupt.json").exists());
    }
}
