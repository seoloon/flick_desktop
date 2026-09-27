use serde::{Deserialize, Serialize};
use url::Url;

use crate::ids::{ItemRef, ServerId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    Jellyfin,
    Plex,
}

impl ProviderKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Jellyfin => "Jellyfin",
            Self::Plex => "Plex",
        }
    }
}

/// A configured server connection, as persisted (without secrets: tokens
/// live in the OS keychain, keyed by `id`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ServerDescriptor {
    pub id: ServerId,
    pub kind: ProviderKind,
    /// Display name reported by the server (user-renamable).
    pub name: String,
    /// Server's own unique id (Jellyfin `Id`, Plex `machineIdentifier`).
    pub remote_id: String,
    pub base_url: Url,
    /// Alternative addresses (Plex exposes local, remote and relay ones).
    pub alternate_urls: Vec<Url>,
    pub version: Option<String>,
    pub user: UserProfile,
    /// Turned off by the user: kept, with its token, but left out of the
    /// catalogue (home, libraries, search) until turned back on.
    #[serde(default)]
    pub disabled: bool,
    /// Connected by switching to a Plex Home member (not the signed-in
    /// account itself); when that member is PIN-protected it only loads
    /// after plex.tv checked the PIN in this run.
    #[serde(default)]
    pub home_member: bool,
}

impl ServerDescriptor {
    /// Re-signing in keeps the stored connection's identity and the user's choices.
    pub fn merged_with(mut self, existing: &ServerDescriptor) -> Self {
        self.id = existing.id;
        self.disabled = existing.disabled;
        // A Home admin re-signed in via a Home switch stays the owner.
        self.home_member = self.home_member && existing.home_member;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub id: String,
    pub name: String,
    pub avatar: Option<Url>,
    /// Server-side administrator. Only used to *show* admin entry points; the
    /// server still enforces every permission.
    pub is_admin: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum LibraryKind {
    Movies,
    Shows,
    Music,
    Photos,
    MusicVideos,
    HomeVideos,
    Mixed,
    LiveTv,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub id: ItemRef,
    pub name: String,
    pub kind: LibraryKind,
    pub item_count: Option<u32>,
    pub image: Option<crate::media::ImageRef>,
}

/// Connection health, surfaced in the Servers screen and diagnostics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "state")]
pub enum ServerStatus {
    Online { latency_ms: u32, url: Url },
    Unauthorized,
    Unreachable { error: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(home_member: bool, disabled: bool) -> ServerDescriptor {
        ServerDescriptor {
            id: ServerId::new(),
            kind: ProviderKind::Plex,
            name: "Home".into(),
            remote_id: "machine".into(),
            base_url: Url::parse("http://home.local:32400/").unwrap(),
            alternate_urls: vec![],
            version: None,
            user: UserProfile { id: "1".into(), name: "Admin".into(), avatar: None, is_admin: true },
            disabled,
            home_member,
        }
    }

    #[test]
    fn re_signing_in_keeps_the_stored_id_and_disabled_choice() {
        let existing = descriptor(false, true);
        let merged = descriptor(false, false).merged_with(&existing);
        assert_eq!(merged.id, existing.id);
        assert!(merged.disabled);
    }

    #[test]
    fn re_signing_in_never_turns_an_owner_connection_into_a_home_member() {
        assert!(!descriptor(true, false).merged_with(&descriptor(false, false)).home_member, "owner stays owner");
        assert!(!descriptor(false, false).merged_with(&descriptor(true, false)).home_member, "member may become owner");
        assert!(descriptor(true, false).merged_with(&descriptor(true, false)).home_member, "member stays member");
    }
}
