//! Tokens in the OS credential store: Windows Credential Manager, macOS
//! Keychain, Secret Service on Linux. There is deliberately **no plaintext
//! fallback**: if the store is unavailable the user is told, and must sign
//! in again next launch.

use std::collections::HashMap;
use std::sync::LazyLock;

use oneshot_core::{Error, Result, ServerId};
use parking_lot::Mutex;

const SERVICE: &str = "flick.seoloon.work";

/// What the store holds, as far as this process has seen (`None`: nothing).
/// Every write goes through this module, so it stays true for the run and
/// each key costs one keychain lookup instead of one per provider rebuild
/// (profile switch, network change): the Keychain and Credential Manager
/// are slow, and macOS may prompt on each access. Errors are not cached.
static KNOWN: LazyLock<Mutex<HashMap<String, Option<String>>>> = LazyLock::new(Default::default);

fn entry(key: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, key).map_err(|e| Error::Storage(format!("credential store: {e}")))
}

fn server_key(server: ServerId) -> String {
    format!("server:{server}")
}

pub fn store_secret(key: &str, secret: &str) -> Result<()> {
    match entry(key)?.set_password(secret) {
        Ok(()) => {
            KNOWN.lock().insert(key.to_owned(), Some(secret.to_owned()));
            Ok(())
        }
        Err(e) => {
            // Unknown outcome: ask the store again next time.
            KNOWN.lock().remove(key);
            Err(Error::Storage(format!("cannot save secret: {e}")))
        }
    }
}

/// `Ok(None)` when nothing is stored under `key`.
pub fn load_secret(key: &str) -> Result<Option<String>> {
    if let Some(known) = KNOWN.lock().get(key) {
        return Ok(known.clone());
    }
    let found = match entry(key)?.get_password() {
        Ok(t) => Some(t),
        Err(keyring::Error::NoEntry) => None,
        Err(e) => return Err(Error::Storage(format!("cannot read secret: {e}"))),
    };
    KNOWN.lock().insert(key.to_owned(), found.clone());
    Ok(found)
}

pub fn delete_secret(key: &str) -> Result<()> {
    match entry(key)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {
            KNOWN.lock().insert(key.to_owned(), None);
            Ok(())
        }
        Err(e) => {
            // Unknown outcome: ask the store again next time.
            KNOWN.lock().remove(key);
            Err(Error::Storage(format!("cannot delete secret: {e}")))
        }
    }
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
}
