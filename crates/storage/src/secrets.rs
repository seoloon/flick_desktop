//! Tokens in the OS credential store: Windows Credential Manager, macOS
//! Keychain, Secret Service on Linux. There is deliberately **no plaintext
//! fallback**: if the store is unavailable the user is told, and must sign
//! in again next launch.

use oneshot_core::{Error, Result, ServerId};

const SERVICE: &str = "flick.seoloon.work";

fn entry(key: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, key).map_err(|e| Error::Storage(format!("credential store: {e}")))
}

fn server_key(server: ServerId) -> String {
    format!("server:{server}")
}

pub fn store_secret(key: &str, secret: &str) -> Result<()> {
    entry(key)?.set_password(secret).map_err(|e| Error::Storage(format!("cannot save secret: {e}")))
}

/// `Ok(None)` when nothing is stored under `key`.
pub fn load_secret(key: &str) -> Result<Option<String>> {
    match entry(key)?.get_password() {
        Ok(t) => Ok(Some(t)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(Error::Storage(format!("cannot read secret: {e}"))),
    }
}

pub fn delete_secret(key: &str) -> Result<()> {
    match entry(key)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(Error::Storage(format!("cannot delete secret: {e}"))),
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
