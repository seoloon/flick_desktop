//! AirPlay pairings, kept in the credential vault with the other secrets:
//! one keychain entry, so one password prompt for all of them.

use oneshot_cast::PairingStore;
use oneshot_core::Result;
use oneshot_storage::secrets;

pub struct VaultPairings;

fn entry(device_id: &str) -> String {
    format!("airplay:{device_id}")
}

impl PairingStore for VaultPairings {
    fn load(&self, device_id: &str) -> Option<String> {
        match secrets::load_secret(&entry(device_id)) {
            Ok(found) => found,
            Err(e) => {
                tracing::warn!(target: "cast", "AirPlay pairing not read: {e}");
                None
            }
        }
    }

    fn save(&self, device_id: &str, credentials: &str) -> Result<()> {
        secrets::store_secret(&entry(device_id), credentials)
    }

    fn forget(&self, device_id: &str) {
        let _ = secrets::delete_secret(&entry(device_id));
    }
}
