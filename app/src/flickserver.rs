//! The Flick Server: the one invitation link (address + key) that FlickSync and
//! FlickDD both use. It is a secret, so it lives in the OS keychain.

use oneshot_core::Result;
use oneshot_flickserver::Invitation;
use oneshot_storage::secrets;

/// Keychain entry of the Flick Server invitation link.
pub const INVITE_ENTRY: &str = "flickserver-invite";
/// Where the link was kept while the server was only known as FlickSync.
const LEGACY_INVITE_ENTRY: &str = "flicksync-invite";

/// The code of a rejected invitation link.
pub fn invite_code(e: oneshot_flickserver::invite::InviteError) -> oneshot_core::codes::Code {
    use oneshot_core::codes::{LINK_DAMAGED, LINK_INCOMPLETE, LINK_NEWER, LINK_NOT_INVITATION};
    use oneshot_flickserver::invite::InviteError as E;
    match e {
        E::NotAnInvitation => LINK_NOT_INVITATION,
        E::UnsupportedVersion => LINK_NEWER,
        E::MissingKey | E::BadKey => LINK_INCOMPLETE,
        E::BadAddress | E::BadPath | E::BadVersion | E::BadTls => LINK_DAMAGED,
    }
}

/// The saved invitation, if any. A link that no longer parses (the format
/// moved on, the entry was damaged) counts as none: it is logged without its content.
/// A link saved under the old entry name is moved to the new one.
pub fn stored_invitation() -> Result<Option<Invitation>> {
    let link = match secrets::load_secret(INVITE_ENTRY)? {
        Some(link) => link,
        None => {
            let Some(old) = secrets::load_secret(LEGACY_INVITE_ENTRY)? else { return Ok(None) };
            if let Ok(inv) = Invitation::parse(&old) {
                secrets::store_secret(INVITE_ENTRY, &inv.link())?;
                let _ = secrets::delete_secret(LEGACY_INVITE_ENTRY);
            }
            old
        }
    };
    match Invitation::parse(&link) {
        Ok(inv) => Ok(Some(inv)),
        Err(e) => {
            tracing::warn!(target: "flickserver", "the saved invitation is unusable: {e}");
            Ok(None)
        }
    }
}

/// Forgets the server.
pub fn clear_invitation() -> Result<()> {
    secrets::delete_secret(INVITE_ENTRY)?;
    secrets::delete_secret(LEGACY_INVITE_ENTRY)
}
