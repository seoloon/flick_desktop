//! Tokens in the OS credential store: Windows Credential Manager, macOS
//! Keychain, Secret Service on Linux. There is deliberately **no plaintext
//! fallback**: if the store is unavailable the user is told, and must sign
//! in again next launch.
//!
//! All secrets live in **one** store entry (a JSON object, `key -> secret`).
//! The macOS Keychain ties each entry to the app's code signature and asks for
//! the password again, entry by entry, whenever that signature changes (every
//! ad hoc signed build): one entry means one prompt, not one per secret.
//! Entries written by earlier versions (one per key) are moved into the vault
//! the first time their key is read.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use oneshot_core::{Error, Result, ServerId};
use parking_lot::Mutex;

const SERVICE: &str = "flick.seoloon.work";
/// The single entry holding every secret.
const VAULT: &str = "vault";

#[derive(Default)]
struct Vault {
    /// `None` until the entry has been read once this run.
    secrets: Option<HashMap<String, String>>,
    /// Keys whose pre-vault entry was already looked for (found and moved, or absent).
    legacy_checked: HashSet<String>,
}

/// What the store holds, as far as this process has seen. Every write goes
/// through this module, so it stays true for the run and the store is read
/// once (the Keychain and Credential Manager are slow, and macOS may prompt
/// on each access). Errors are never cached. The lock is held across store
/// calls so two writes cannot overwrite each other's key.
static STATE: LazyLock<Mutex<Vault>> = LazyLock::new(Default::default);

fn entry(key: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, key).map_err(|e| Error::Storage(format!("credential store: {e}")))
}

fn read_vault() -> Result<HashMap<String, String>> {
    match entry(VAULT)?.get_password() {
        Ok(json) => serde_json::from_str(&json).map_err(|e| Error::Storage(format!("credential vault is unreadable: {e}"))),
        Err(keyring::Error::NoEntry) => Ok(HashMap::new()),
        Err(e) => Err(Error::Storage(format!("cannot read secret: {e}"))),
    }
}

fn write_vault(secrets: &HashMap<String, String>) -> Result<()> {
    let json = serde_json::to_string(secrets).map_err(|e| Error::Storage(format!("cannot save secret: {e}")))?;
    entry(VAULT)?.set_password(&json).map_err(|e| Error::Storage(format!("cannot save secret: {e}")))
}

impl Vault {
    fn loaded(&mut self) -> Result<&mut HashMap<String, String>> {
        if self.secrets.is_none() {
            self.secrets = Some(read_vault()?);
        }
        Ok(self.secrets.as_mut().expect("loaded above"))
    }

    /// Saves `next` and, only if that worked, makes it the current content.
    fn commit(&mut self, next: HashMap<String, String>) -> Result<()> {
        write_vault(&next)?;
        self.secrets = Some(next);
        Ok(())
    }
}

fn server_key(server: ServerId) -> String {
    format!("server:{server}")
}

pub fn store_secret(key: &str, secret: &str) -> Result<()> {
    let mut v = STATE.lock();
    let mut next = v.loaded()?.clone();
    next.insert(key.to_owned(), secret.to_owned());
    v.commit(next)?;
    v.legacy_checked.insert(key.to_owned());
    Ok(())
}

/// `Ok(None)` when nothing is stored under `key`.
pub fn load_secret(key: &str) -> Result<Option<String>> {
    let mut v = STATE.lock();
    if let Some(found) = v.loaded()?.get(key) {
        return Ok(Some(found.clone()));
    }
    if v.legacy_checked.contains(key) {
        return Ok(None);
    }
    // Written by an earlier version as an entry of its own: move it in.
    let legacy = entry(key)?;
    let found = match legacy.get_password() {
        Ok(t) => t,
        Err(keyring::Error::NoEntry) => {
            v.legacy_checked.insert(key.to_owned());
            return Ok(None);
        }
        Err(e) => return Err(Error::Storage(format!("cannot read secret: {e}"))),
    };
    let mut next = v.loaded()?.clone();
    next.insert(key.to_owned(), found.clone());
    match v.commit(next) {
        Ok(()) => {
            let _ = legacy.delete_credential();
        }
        // Still usable this run; it moves next time.
        Err(e) => tracing::warn!(target: "cache", "secret not moved into the vault: {e}"),
    }
    v.legacy_checked.insert(key.to_owned());
    Ok(Some(found))
}

pub fn delete_secret(key: &str) -> Result<()> {
    let mut v = STATE.lock();
    let secrets = v.loaded()?;
    if secrets.contains_key(key) {
        let mut next = secrets.clone();
        next.remove(key);
        v.commit(next)?;
    }
    // A pre-vault entry of that key must not come back from the dead.
    if let Ok(legacy) = entry(key) {
        let _ = legacy.delete_credential();
    }
    v.legacy_checked.insert(key.to_owned());
    Ok(())
}

pub fn store_token(server: ServerId, token: &str) -> Result<()> {
    store_secret(&server_key(server), token)
}

pub fn load_token(server: ServerId) -> Result<Option<String>> {
    load_secret(&server_key(server))
}

pub fn delete_token(server: ServerId) -> Result<()> {
    delete_secret(&server_key(server))
}

/// Whether a credential store is usable on this system.
pub fn available() -> bool {
    keyring::Entry::store_status().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Touches the real OS store: run explicitly with `--ignored`.
    #[test]
    #[ignore = "writes to the OS credential store"]
    fn roundtrip_in_os_store() {
        let id = ServerId::new();
        store_token(id, "secret-token").unwrap();
        assert_eq!(load_token(id).unwrap().as_deref(), Some("secret-token"));
        delete_token(id).unwrap();
        assert_eq!(load_token(id).unwrap(), None);
    }

    /// An entry of the old one-entry-per-key layout is found, then lives in the vault.
    #[test]
    #[ignore = "writes to the OS credential store"]
    fn legacy_entry_moves_into_vault() {
        let id = ServerId::new();
        let key = server_key(id);
        entry(&key).unwrap().set_password("old-token").unwrap();
        assert_eq!(load_token(id).unwrap().as_deref(), Some("old-token"));
        assert!(matches!(entry(&key).unwrap().get_password(), Err(keyring::Error::NoEntry)));
        assert_eq!(read_vault().unwrap().get(&key).map(String::as_str), Some("old-token"));
        delete_token(id).unwrap();
    }
}
