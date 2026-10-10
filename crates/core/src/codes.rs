//! Every error code a person can be shown, with what it means and how to get
//! past it. `ERROR_IDENTIFIER.md` is generated from this table (see the test
//! at the bottom): add the code here, never in the file.
//!
//! A code reads `FLK-<AREA>-<NNN>`. It is shown after the message, so that
//! "FLK-NET-001" is enough to know what happened.

/// One documented error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Code {
    pub id: &'static str,
    pub area: &'static str,
    /// What happened, for the person who reads the code in a bug report.
    pub meaning: &'static str,
    /// What the person can do about it; empty when nothing but retrying helps.
    pub fix: &'static str,
}

impl Code {
    /// `text` followed by the code, for messages shown as they are (a download's status line).
    pub fn suffix(&self, text: impl AsRef<str>) -> String {
        format!("{} ({})", text.as_ref().trim_end(), self.id)
    }

    /// `text` with this code attached, as carried inside an `Error`'s message.
    pub fn tag(&self, text: impl AsRef<str>) -> String {
        format!("[{}] {}", self.id, text.as_ref())
    }
}

/// The code at the start of a message built with [`Code::tag`], and the message without it.
pub fn split_tag(text: &str) -> (Option<&str>, &str) {
    if let Some(rest) = text.strip_prefix("[FLK-")
        && let Some((code, message)) = rest.split_once("] ")
    {
        return (Some(&text[1..code.len() + 5]), message);
    }
    (None, text)
}

macro_rules! codes {
    ($( $name:ident = $id:literal, $area:literal, $meaning:literal, $fix:literal; )*) => {
        $( pub const $name: Code = Code { id: $id, area: $area, meaning: $meaning, fix: $fix }; )*
        /// Every code, in the order of the table.
        pub const ALL: &[Code] = &[ $( $name ),* ];
        /// Each code's constant name, for the tests.
        #[cfg(test)]
        const ALL_NAMES: &[(&str, &str)] = &[ $( ($id, stringify!($name)) ),* ];
    };
}

/// The areas, in the order of the document, with their titles.
pub const AREAS: &[(&str, &str)] = &[
    ("GEN", "General"),
    ("NET", "Network and servers' answers"),
    ("AUTH", "Sign-in and permissions"),
    ("SRV", "Servers and libraries"),
    ("PLAY", "Playback decisions"),
    ("PLR", "Player engine"),
    ("DL", "Downloads and offline"),
    ("SYNC", "Watch together"),
    ("LINK", "Flick Server invitation links"),
    ("CAST", "Casting"),
    ("PROF", "Profiles and PINs"),
    ("STO", "Storage and credentials"),
    ("IMG", "Pictures"),
    ("UPD", "Updates"),
    ("SYS", "Window and system"),
    ("UI", "Screens"),
];

codes! {
    // ---- General -----------------------------------------------------------
    GEN_OTHER = "FLK-GEN-000", "GEN", "Something unexpected went wrong and no more specific code applies.", "Try again. If it keeps happening, report the code with the log (Settings › Advanced).";
    GEN_INVALID = "FLK-GEN-001", "GEN", "Flick was asked something that does not make sense (a missing or malformed value).", "Check what you entered and try again.";

    // ---- Network -----------------------------------------------------------
    NET_OTHER = "FLK-NET-000", "NET", "A network problem with no more specific cause.", "Check your connection and try again.";
    NET_UNREACHABLE = "FLK-NET-001", "NET", "The server could not be reached (nothing answered at that address).", "Check the server is on and the address is right; make sure this computer is on the same network or has internet.";
    NET_TIMEOUT = "FLK-NET-002", "NET", "The server took too long to answer.", "Try again; if the server is slow or far away, raise the request timeout in Settings › Network.";
    NET_TLS = "FLK-NET-003", "NET", "The secure connection (HTTPS certificate) could not be trusted.", "Check the address uses the right host name. For a server with a self-signed certificate, allow it in Settings › Network.";
    NET_SERVER_ERROR = "FLK-NET-004", "NET", "The server answered with an internal error (HTTP 5xx).", "Wait a moment and try again; check the server's own logs if it persists.";
    NET_BAD_ANSWER = "FLK-NET-005", "NET", "The server answered with something Flick does not understand.", "Make sure the server is up to date and is the kind (Jellyfin, Plex) you added.";
    NET_PROXY = "FLK-NET-006", "NET", "The proxy address in the settings is not valid.", "Fix or clear the proxy in Settings › Network.";
    NET_CLIENT = "FLK-NET-007", "NET", "Flick could not set up its network connection with the current settings.", "Review Settings › Network, or reset them.";
    NET_ADDRESS = "FLK-NET-008", "NET", "A server address could not be built from what was given.", "Check the address of the server.";

    // ---- Sign-in -----------------------------------------------------------
    AUTH_REJECTED = "FLK-AUTH-001", "AUTH", "The server refused the saved credentials (wrong password, revoked or expired session).", "Sign in to the server again from Settings › Servers.";
    AUTH_FORBIDDEN = "FLK-AUTH-002", "AUTH", "The server knows you but does not allow this action.", "Ask the server's owner for the permission, or use another account.";
    AUTH_QUICK_CONNECT = "FLK-AUTH-003", "AUTH", "Quick Connect is turned off on the Jellyfin server.", "Sign in with a user name and password, or enable Quick Connect in the Jellyfin dashboard.";
    AUTH_PLAYBACK_DENIED = "FLK-AUTH-004", "AUTH", "The server does not allow this account to play media.", "Ask the server's owner to enable playback for the account.";
    AUTH_PLEX_ACCOUNT = "FLK-AUTH-005", "AUTH", "A Plex account sign-in is needed for this and none is saved.", "Sign in to plex.tv from Settings › Servers.";
    AUTH_NOT_ADMIN = "FLK-AUTH-006", "AUTH", "The account is not an administrator of this server.", "Use an administrator account for these settings.";
    AUTH_AIRPLAY_PAIRING = "FLK-AUTH-007", "AUTH", "The AirPlay device asks for a PIN (or no longer recognises this computer).", "Enter the code shown on the TV; it is asked once, then remembered.";
    AUTH_AIRPLAY_WRONG_PIN = "FLK-AUTH-008", "AUTH", "The PIN typed is not the one the AirPlay device shows.", "Read the code on the TV again and retry.";

    // ---- Servers and libraries ---------------------------------------------
    SRV_NOT_FOUND = "FLK-SRV-000", "SRV", "What was asked for is not on the server (deleted, moved, or an id that is no longer valid).", "Refresh the library; if it is gone from the server, there is nothing to do.";
    SRV_UNSUPPORTED = "FLK-SRV-001", "SRV", "The server (or this kind of server) does not offer that feature.", "";
    SRV_NOT_JELLYFIN = "FLK-SRV-002", "SRV", "The address answered, but it is not a Jellyfin server.", "Check the address and port; for a reverse proxy, include its path.";
    SRV_NOT_CONNECTED = "FLK-SRV-003", "SRV", "The server is in the list but not connected right now.", "Open Settings › Servers and reconnect it.";
    SRV_FOREIGN_ITEM = "FLK-SRV-004", "SRV", "An item was asked from a server it does not belong to.", "Reopen the title from its own server's library.";
    SRV_PLEX_NO_ROUTE = "FLK-SRV-005", "SRV", "None of the Plex server's addresses can be reached from this network.", "Check the server is on and remote access is enabled, or join its network.";
    SRV_TMDB_KEY = "FLK-SRV-006", "SRV", "The TMDB key is not a valid v3 key or v4 token.", "Copy the key again from your TMDB account settings.";
    SRV_NO_PARENT = "FLK-SRV-007", "SRV", "A library to browse was not given.", "";
    SRV_BAD_SECTION = "FLK-SRV-008", "SRV", "The Plex library section id is not valid.", "";
    SRV_PLEX_ITEM = "FLK-SRV-009", "SRV", "The Plex item does not exist (any more).", "Refresh the library.";
    SRV_WATCHLIST_ACCOUNT = "FLK-SRV-010", "SRV", "The saved plex.tv sign-in belongs to another account than this server's.", "Sign in to plex.tv again with the right account.";
    SRV_PLEX_CATALOGUE = "FLK-SRV-012", "SRV", "The title is not in the Plex catalogue, so it cannot be favourited or watchlisted.", "";

    // ---- Playback decisions ------------------------------------------------
    PLAY_OTHER = "FLK-PLAY-000", "PLAY", "Playback could not start for a reason with no more specific code.", "Try again, or try another version of the title.";
    PLAY_NO_VERSION = "FLK-PLAY-001", "PLAY", "The server returned no playable version of the title.", "Check the file exists and plays on the server; ask the server to rescan.";
    PLAY_REFUSED = "FLK-PLAY-002", "PLAY", "The server refused to start the playback.", "Check the server's playback settings and the account's rights.";
    PLAY_NO_STREAM = "FLK-PLAY-003", "PLAY", "The server offered no stream address for the title.", "Allow transcoding or Direct Stream in Settings › Playback, then retry.";
    PLAY_NO_PART = "FLK-PLAY-004", "PLAY", "The Plex media has no playable file part.", "Rescan the library on the Plex server.";
    PLAY_BLOCKED = "FLK-PLAY-005", "PLAY", "Flick found no way to play this file on this computer with the current settings.", "The message lists why; allow transcoding in Settings › Playback or pick another version.";
    PLAY_NOTHING = "FLK-PLAY-006", "PLAY", "An action needed something to be playing and nothing was.", "";
    PLAY_VERSION_GONE = "FLK-PLAY-007", "PLAY", "The chosen version of the title is not on the server any more.", "Reopen the title and choose a version again.";

    // ---- Player engine -----------------------------------------------------
    PLR_NOT_FOUND = "FLK-PLR-001", "PLR", "The video engine (libmpv) was not found or could not be loaded.", "Reinstall Flick; if you build it yourself, put libmpv next to the app.";
    PLR_INIT = "FLK-PLR-002", "PLR", "The video engine failed to start.", "Restart Flick; check the log for the engine's reason.";
    PLR_NOT_RUNNING = "FLK-PLR-003", "PLR", "The video engine is not running.", "Restart Flick.";
    PLR_COMMAND = "FLK-PLR-004", "PLR", "The video engine rejected a command or could not open the file.", "Try again; the message gives the engine's reason (file unreadable, format unsupported…).";
    PLR_VERSION = "FLK-PLR-005", "PLR", "The libmpv on this computer is too old or too new for Flick.", "Use the libmpv shipped with Flick.";
    PLR_MEDIA = "FLK-PLR-006", "PLR", "The video engine could not play the file (unsupported or damaged).", "Try another version of the title; enable server transcoding in Settings › Playback.";

    // ---- Downloads ---------------------------------------------------------
    DL_NOT_CONFIGURED = "FLK-DL-001", "DL", "Downloads need a Flick Server invitation link and none is saved.", "Add the link in Settings › Flick Server.";
    DL_KIND = "FLK-DL-002", "DL", "Only movies and episodes can be downloaded.", "";
    DL_NOTHING = "FLK-DL-003", "DL", "There was nothing to download in the chosen item.", "";
    DL_DISK = "FLK-DL-004", "DL", "Flick cannot write the downloaded file (disk full, folder missing or not allowed).", "Free some space or fix the folder's permissions, then resume the download.";
    DL_KEY = "FLK-DL-005", "DL", "The key that protects downloaded files cannot be read from the credential store.", "Unlock the keychain/credential manager and retry. If it was erased, downloads made before cannot be played: download them again.";
    DL_GONE = "FLK-DL-006", "DL", "The download is no longer on this computer.", "Download it again.";
    DL_REFUSED = "FLK-DL-007", "DL", "The Flick Server refused the download.", "Check the invitation link and the server's download settings.";
    DL_UNSTABLE = "FLK-DL-008", "DL", "The connection to the Flick Server keeps failing; the download is paused.", "Check your connection and resume.";
    DL_DEVICE = "FLK-DL-009", "DL", "The Flick Server did not accept this device.", "Add the invitation link again in Settings › Flick Server.";
    DL_SLOW = "FLK-DL-010", "DL", "The Flick Server asked to slow down.", "Wait; the download retries by itself.";
    DL_MEDIA_SERVER = "FLK-DL-011", "DL", "The media server behind the Flick Server did not answer.", "Check the Jellyfin/Plex server is running.";
    DL_SERVER = "FLK-DL-012", "DL", "The Flick Server had a problem.", "Wait and resume.";
    DL_NO_DETAILS = "FLK-DL-013", "DL", "The saved details of the download are missing.", "Remove the download and download it again.";
    DL_PATH = "FLK-DL-014", "DL", "The downloaded file's path cannot be opened.", "Remove the download and download it again.";
    DL_RELAY = "FLK-DL-015", "DL", "Flick could not open its local reader for protected downloads.", "Restart Flick; check that no security software blocks local connections.";
    DL_CHANGED = "FLK-DL-016", "DL", "The file on the server changed or arrived with the wrong size; the download restarts.", "";
    DL_OPEN_FOLDER = "FLK-DL-017", "DL", "The downloads folder could not be opened.", "";
    DL_BAD_ID = "FLK-DL-018", "DL", "A download id is not valid.", "";
    DL_SERVER_GONE = "FLK-DL-019", "DL", "The server of this title is no longer in Flick.", "Add the server again in Settings › Servers.";

    // ---- Watch together ----------------------------------------------------
    SYNC_OTHER = "FLK-SYNC-000", "SYNC", "Something went wrong with the watch room.", "Leave and rejoin the room.";
    SYNC_ONLY_HOST_MEDIA = "FLK-SYNC-001", "SYNC", "Only the host can choose what the room watches.", "Ask the host.";
    SYNC_CONTROL_DENIED = "FLK-SYNC-002", "SYNC", "Only the host can control playback in this room.", "Ask the host to allow guests to control playback.";
    SYNC_ROOM_NOT_FOUND = "FLK-SYNC-003", "SYNC", "The room does not exist (any more).", "Ask for a new code.";
    SYNC_ROOM_FULL = "FLK-SYNC-004", "SYNC", "The room is full.", "Ask the host to make room.";
    SYNC_ROOM_CLOSED = "FLK-SYNC-005", "SYNC", "The host closed the room.", "";
    SYNC_INVALID_MEDIA = "FLK-SYNC-006", "SYNC", "The host picked something Flick cannot share.", "Pick a movie or an episode.";
    SYNC_SESSION = "FLK-SYNC-007", "SYNC", "The sign-in to the FlickSync server expired.", "Try again; check the Flick Server link in Settings.";
    SYNC_SLOW_DOWN = "FLK-SYNC-008", "SYNC", "Too many requests in a short time.", "Wait a few seconds.";
    SYNC_CHAT_OFF = "FLK-SYNC-009", "SYNC", "Chat is turned off in this room.", "";
    SYNC_VERSION = "FLK-SYNC-010", "SYNC", "This Flick version and the FlickSync server do not speak the same protocol version.", "Update Flick or the server.";
    SYNC_UNAVAILABLE = "FLK-SYNC-011", "SYNC", "Watch together is not reachable right now.", "Check the Flick Server is up and your connection works.";
    SYNC_MEDIA_UNAVAILABLE = "FLK-SYNC-012", "SYNC", "The title is not available on your connected server.", "Add the server that has it, or watch something else.";
    SYNC_NOT_CONFIGURED = "FLK-SYNC-013", "SYNC", "Watch together is not set up.", "Add the Flick Server link in Settings › Flick Server.";
    SYNC_NO_ROOM = "FLK-SYNC-014", "SYNC", "An action needed a room and you are not in one.", "Join or create a room.";
    SYNC_KIND = "FLK-SYNC-015", "SYNC", "Only movies and episodes can be watched together.", "";
    SYNC_DISCONNECTED = "FLK-SYNC-017", "SYNC", "The connection to the room was lost.", "Rejoin the room.";

    // ---- Invitation links --------------------------------------------------
    LINK_NOT_INVITATION = "FLK-LINK-001", "LINK", "The text is not a Flick Server invitation link.", "Copy the link from the Flick Server again.";
    LINK_NEWER = "FLK-LINK-002", "LINK", "The link comes from a newer Flick Server.", "Update Flick.";
    LINK_INCOMPLETE = "FLK-LINK-003", "LINK", "The link has no key or its key is unreadable (cut when copying).", "Copy the whole link again.";
    LINK_DAMAGED = "FLK-LINK-004", "LINK", "The link's address, path, version or TLS part is damaged.", "Copy it again in full, or ask for a new one.";
    LINK_NOT_SAVED = "FLK-LINK-006", "LINK", "The server did not answer, so the link was not saved.", "Check the link, that the server is on, and try again.";

    // ---- Casting -----------------------------------------------------------
    CAST_DEVICE_GONE = "FLK-CAST-001", "CAST", "The cast device is no longer on the network.", "Check it is on, then pick it again.";
    CAST_NOTHING = "FLK-CAST-002", "CAST", "Nothing is being cast.", "";
    CAST_CHROMECAST = "FLK-CAST-003", "CAST", "The Chromecast did not answer or closed the connection.", "Check it is on the same network, restart it, and retry.";
    CAST_LOAD = "FLK-CAST-004", "CAST", "The Chromecast could not load the stream.", "Try another version of the title, or stream from the server with transcoding allowed.";
    CAST_AIRPLAY = "FLK-CAST-005", "CAST", "The AirPlay device refused or could not play the stream.", "Check the device accepts streams from this network.";
    CAST_ADDRESS = "FLK-CAST-006", "CAST", "The device's address is not valid.", "Refresh the device list.";
    CAST_RELAY = "FLK-CAST-007", "CAST", "Flick could not open the local relay the device pulls the video from.", "Check no firewall blocks Flick on the local network.";
    CAST_PROTOCOL = "FLK-CAST-008", "CAST", "The cast device sent something malformed.", "Restart the device.";
    CAST_PAIR_WAIT = "FLK-CAST-010", "CAST", "The AirPlay device is busy or asks to wait before another PIN attempt.", "Wait a minute and retry.";
    CAST_PAIR_FAILED = "FLK-CAST-011", "CAST", "The pairing with the AirPlay device failed or could not be verified.", "Retry; if it persists, remove the device's pairing in its settings (Remotes and Devices) and pair again.";
    CAST_FORMAT = "FLK-CAST-012", "CAST", "AirPlay cannot play this file as it is (container or codec) and Flick does not convert it yet.", "Cast a title in MP4/H.264/HEVC with AAC or AC-3 audio, or watch it in Flick.";
    CAST_AIRPLAY_LOST = "FLK-CAST-013", "CAST", "The AirPlay device could not be reached, or stopped answering.", "Check it is on (wake it up) and on the same network as this computer, not a guest Wi-Fi, then retry.";
    CAST_AIRPLAY_CHANNEL = "FLK-CAST-014", "CAST", "The encrypted connection with the AirPlay device broke or could not be checked.", "Retry. If it keeps happening, remove Flick from the device's paired remotes and pair again.";
    CAST_OTHER = "FLK-CAST-009", "CAST", "A casting problem with no more specific cause.", "Restart the device and retry.";

    // ---- Profiles ----------------------------------------------------------
    PROF_WRONG_PIN = "FLK-PROF-001", "PROF", "The PIN is wrong.", "Try again.";
    PROF_LOCKED = "FLK-PROF-002", "PROF", "Too many wrong PINs: PIN entry is locked for a short time.", "Wait for the delay shown.";
    PROF_PIN_FORMAT = "FLK-PROF-003", "PROF", "A PIN must be exactly 4 digits.", "Use 4 digits.";
    PROF_NOT_FOUND = "FLK-PROF-004", "PROF", "The profile does not exist (any more).", "Reopen the profile list.";
    PROF_BUSY = "FLK-PROF-005", "PROF", "A profile switch is already in progress.", "Wait for it to finish.";
    PROF_NAME = "FLK-PROF-006", "PROF", "A profile needs a name.", "Type a name.";
    PROF_ACCOUNT = "FLK-PROF-007", "PROF", "That account is not part of this profile.", "";
    PROF_MERGE_SELF = "FLK-PROF-008", "PROF", "A profile cannot be merged with itself.", "";
    PROF_MERGE_KIND = "FLK-PROF-009", "PROF", "Only profiles made from server users can be merged.", "";
    PROF_HIDE_ONLY = "FLK-PROF-010", "PROF", "Profiles made from server users are hidden, not deleted.", "Hide it instead.";
    PROF_PIN_HASH = "FLK-PROF-012", "PROF", "Flick could not protect the PIN.", "Try again.";

    // ---- Storage -----------------------------------------------------------
    STO_OTHER = "FLK-STO-000", "STO", "A file could not be read or written.", "Check the disk has space and Flick may write to its folder.";
    STO_CREDENTIAL_STORE = "FLK-STO-001", "STO", "The system credential store (Keychain, Credential Manager) is not available.", "Unlock it or allow Flick access, then retry. Flick never keeps passwords in plain files.";
    STO_VAULT_UNREADABLE = "FLK-STO-002", "STO", "The saved credentials cannot be read back.", "Sign in to your servers again.";
    STO_SECRET_SAVE = "FLK-STO-003", "STO", "A credential could not be saved.", "Unlock the credential store and retry.";
    STO_FILE = "FLK-STO-004", "STO", "A settings or data file could not be read or written.", "Check the disk space and the folder's permissions.";
    STO_CACHE = "FLK-STO-005", "STO", "The metadata cache could not be written.", "Clear the cache in Settings › Network & Cache.";
    STO_IMAGE_CACHE = "FLK-STO-006", "STO", "The picture cache could not be written or cleared.", "Check the disk space.";
    STO_RANDOM = "FLK-STO-007", "STO", "The system could not provide a random key.", "Restart the computer.";

    // ---- Pictures ----------------------------------------------------------
    IMG_AVATAR_PATH = "FLK-IMG-001", "IMG", "An avatar address is malformed.", "";
    IMG_AVATAR_GONE = "FLK-IMG-002", "IMG", "The profile's picture is missing.", "Choose a picture again.";
    IMG_TMDB_PATH = "FLK-IMG-003", "IMG", "A TMDB picture address is malformed.", "";
    IMG_DECODE = "FLK-IMG-004", "IMG", "A picture could not be decoded.", "";
    IMG_OFFLINE = "FLK-IMG-005", "IMG", "A picture of a download is not stored.", "";

    // ---- Updates -----------------------------------------------------------
    UPD_NETWORK = "FLK-UPD-001", "UPD", "The update server could not be reached.", "Check your connection.";
    UPD_NO_RELEASE = "FLK-UPD-002", "UPD", "No published release was found to update from.", "Try later.";
    UPD_DEV_BUILD = "FLK-UPD-003", "UPD", "Updates are not installed in a development build.", "";
    UPD_NO_PENDING = "FLK-UPD-004", "UPD", "There is no update waiting to be installed.", "Check for updates again.";
    UPD_OTHER = "FLK-UPD-005", "UPD", "The update failed (download, signature or installation).", "Try again; download the new version manually if it persists.";

    // ---- Window and system -------------------------------------------------
    SYS_WINDOW = "FLK-SYS-001", "SYS", "The window could not do what was asked (fullscreen, picture-in-picture…).", "Try again.";
    SYS_TASK = "FLK-SYS-002", "SYS", "A background task stopped unexpectedly.", "Try again; restart Flick if it persists.";

    // ---- Screens -----------------------------------------------------------
    UI_CRASH = "FLK-UI-001", "UI", "A screen of Flick crashed and was replaced by an error page.", "Use Reload; report the code with the technical text on the page.";
    UI_CLIPBOARD = "FLK-UI-002", "UI", "The text could not be copied to the clipboard.", "Select and copy it by hand.";
}

/// The code an error of a given kind gets when its site names none.
pub fn fallback(kind: &str) -> Code {
    match kind {
        "network" => NET_OTHER,
        "unauthorized" => AUTH_REJECTED,
        "forbidden" => AUTH_FORBIDDEN,
        "notFound" => SRV_NOT_FOUND,
        "unsupported" => SRV_UNSUPPORTED,
        "protocol" => NET_BAD_ANSWER,
        "invalid" => GEN_INVALID,
        "storage" => STO_OTHER,
        "playback" => PLAY_OTHER,
        "wrongPin" => PROF_WRONG_PIN,
        "pinLocked" => PROF_LOCKED,
        _ => GEN_OTHER,
    }
}

/// `ERROR_IDENTIFIER.md`, from the table.
pub fn document() -> String {
    let mut out = String::from(
        "# Error identifiers\n\n\
         <!-- Generated from `crates/core/src/codes.rs`: do not edit by hand.\n\
         Regenerate with `UPDATE_ERROR_DOC=1 cargo test -p oneshot-core error_document`. -->\n\n\
         Every error Flick shows ends with a code such as `FLK-NET-001`. Quote the code to find the line below:\n\
         what it means, and what can be done about it. The first part of the code is the area.\n\n",
    );
    for (area, title) in AREAS {
        out.push_str(&format!("## {title} (`FLK-{area}-…`)\n\n| Code | Meaning | What to do |\n|---|---|---|\n"));
        for c in ALL.iter().filter(|c| c.area == *area) {
            let fix = if c.fix.is_empty() { "—" } else { c.fix };
            out.push_str(&format!("| `{}` | {} | {} |\n", c.id, c.meaning, fix));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn ids_are_unique_well_formed_and_in_a_known_area() {
        let mut seen = HashSet::new();
        for c in ALL {
            assert!(seen.insert(c.id), "{} appears twice", c.id);
            let parts: Vec<&str> = c.id.split('-').collect();
            assert_eq!(parts.len(), 3, "{}", c.id);
            assert_eq!((parts[0], parts[1]), ("FLK", c.area), "{}", c.id);
            assert!(parts[2].len() == 3 && parts[2].chars().all(|d| d.is_ascii_digit()), "{}", c.id);
            assert!(AREAS.iter().any(|(a, _)| *a == c.area), "{} is in an area with no title", c.id);
            assert!(!c.meaning.is_empty());
        }
    }

    #[test]
    fn a_tag_round_trips() {
        let tagged = NET_TIMEOUT.tag("It took too long.");
        assert_eq!(split_tag(&tagged), (Some("FLK-NET-002"), "It took too long."));
        assert_eq!(split_tag("plain text"), (None, "plain text"));
        assert_eq!(split_tag("[FLK-broken"), (None, "[FLK-broken"));
    }

    #[test]
    fn every_kind_has_a_fallback_code() {
        for kind in ["network", "unauthorized", "forbidden", "notFound", "unsupported", "protocol", "invalid", "storage", "playback", "wrongPin", "pinLocked", "other"] {
            assert!(ALL.contains(&fallback(kind)), "{kind}");
        }
    }

    /// A code nobody raises is a promise nobody keeps: each one is used in Rust (by its name) or in the UI (by its id).
    #[test]
    fn every_documented_code_is_raised_somewhere() {
        fn collect(dir: &std::path::Path, out: &mut String, exts: &[&str]) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for e in entries.flatten() {
                let p = e.path();
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if p.is_dir() {
                    if !matches!(name, "target" | "node_modules" | "bindings" | "gen" | ".git") {
                        collect(&p, out, exts);
                    }
                } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| exts.contains(&x)) && name != "codes.rs" {
                    out.push_str(&std::fs::read_to_string(&p).unwrap_or_default());
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut rust = String::new();
        collect(&root.join("crates"), &mut rust, &["rs"]);
        collect(&root.join("app/src"), &mut rust, &["rs"]);
        let mut ui = String::new();
        collect(&root.join("ui/src"), &mut ui, &["ts", "tsx"]);
        let fallbacks: Vec<Code> = ["network", "unauthorized", "forbidden", "notFound", "unsupported", "protocol", "invalid", "storage", "playback", "wrongPin", "pinLocked", "other"].map(fallback).to_vec();
        let unused: Vec<&str> = ALL
            .iter()
            .filter(|c| {
                if fallbacks.contains(c) {
                    return false;
                }
                let name = ALL_NAMES.iter().find(|(id, _)| *id == c.id).map_or("", |(_, n)| *n);
                !(rust.contains(&format!("{name}.")) || rust.contains(&format!("::{name}")) || rust.contains(&format!("{name},")) || ui.contains(c.id))
            })
            .map(|c| c.id)
            .collect();
        assert!(unused.is_empty(), "documented but never raised: {unused:?}");
    }

    /// The document in the repository is the table, printed.
    #[test]
    fn error_document_is_up_to_date() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ERROR_IDENTIFIER.md");
        let expected = document();
        if std::env::var_os("UPDATE_ERROR_DOC").is_some() {
            std::fs::write(&path, &expected).unwrap();
        }
        let actual = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(actual == expected, "ERROR_IDENTIFIER.md is out of date: run `UPDATE_ERROR_DOC=1 cargo test -p oneshot-core error_document`");
    }
}
