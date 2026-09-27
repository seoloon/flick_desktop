//! Multi-user profiles. `ProfilesConfig` is persisted (`profiles.json`) by
//! `oneshot-storage`; the `*Card` / `*State` types are what the UI sees
//! (no PIN hash, no token, ever).

use std::fmt;

use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::ids::ServerId;
use crate::server::ProviderKind;
use crate::settings::PersonalSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, type = "string"))]
#[serde(transparent)]
pub struct ProfileId(pub Uuid);

impl ProfileId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ProfileId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ProfileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for ProfileId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Uuid::parse_str(s).map(Self)
    }
}

/// Where profiles come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ProfileMode {
    /// B: the servers' own users, grouped by name.
    #[default]
    ServerUsers,
    /// A: local profiles, each signing in to its own servers.
    Local,
    /// C: local profiles linked to existing connections.
    Linked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum AvatarStyle {
    /// The picture of the first account that has one, else initials.
    #[default]
    Server,
    Initials,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum Origin {
    /// Created by hand (modes A and C).
    #[default]
    Manual,
    /// Grouped from server users (mode B); `key` is the normalised name.
    Derived { key: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Profile {
    pub id: ProfileId,
    pub name: String,
    pub avatar: AvatarStyle,
    /// `#rrggbb`, one of the profile palette.
    pub color: String,
    /// argon2id PHC string; never the PIN itself.
    pub pin: Option<String>,
    /// Modes A/C: the connections this profile uses. Ignored in B.
    pub connections: Vec<ServerId>,
    pub prefs: PersonalSettings,
    pub origin: Origin,
    /// Mode B: left off the picker.
    pub hidden: bool,
    /// Mode B: connections taken out of this group (they form their own).
    pub detached: Vec<ServerId>,
}

impl Default for Profile {
    fn default() -> Self {
        Self::new(String::new(), "#a3a3a3", Origin::Manual, PersonalSettings::default())
    }
}

impl Profile {
    pub fn new(name: impl Into<String>, color: impl Into<String>, origin: Origin, prefs: PersonalSettings) -> Self {
        Self {
            id: ProfileId::new(),
            name: name.into(),
            avatar: AvatarStyle::Server,
            color: color.into(),
            pin: None,
            connections: Vec::new(),
            prefs,
            origin,
            hidden: false,
            detached: Vec::new(),
        }
    }
}

/// A user seen on a configured server (mode B), signed in here or not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredUser {
    /// A connection to the physical server this user lives on.
    pub server: ServerId,
    pub kind: ProviderKind,
    /// Equals `UserProfile::id` once signed in (Jellyfin user id, Plex account id).
    pub remote_user_id: String,
    /// Plex Home member uuid (for `switch`).
    pub switch_id: Option<String>,
    pub name: String,
    pub avatar: Option<Url>,
    /// Jellyfin: a password is needed to sign in.
    pub has_password: bool,
    /// Plex Home: plex.tv asks this member's PIN.
    pub protected: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProfilesConfig {
    pub enabled: bool,
    pub mode: ProfileMode,
    pub ask_on_startup: bool,
    pub last_profile: Option<ProfileId>,
    pub profiles: Vec<Profile>,
    /// Last discovery (mode B), so the picker paints instantly.
    pub discovered: Vec<DiscoveredUser>,
}

impl Default for ProfilesConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: ProfileMode::ServerUsers,
            ask_on_startup: true,
            last_profile: None,
            profiles: Vec::new(),
            discovered: Vec::new(),
        }
    }
}

// ------------------------------------------------------------ UI views

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum AccountState {
    Connected,
    /// Seen on the server, not signed in here yet.
    Pending,
    /// Its server did not answer the last discovery.
    Offline,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ProfileAccount {
    pub kind: ProviderKind,
    pub server_name: String,
    pub user_name: String,
    pub state: AccountState,
    pub connection: Option<ServerId>,
    /// Where a pending Jellyfin account signs in.
    pub base_url: Url,
    pub needs_password: bool,
    /// A Plex Home member protected by a Plex PIN.
    pub plex_pin: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ProfileCard {
    pub id: ProfileId,
    pub name: String,
    pub color: String,
    /// Changes with the picture; `None` = initials.
    pub avatar_key: Option<String>,
    /// Protected by a Flick PIN.
    pub locked: bool,
    pub hidden: bool,
    pub accounts: Vec<ProfileAccount>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ProfilesState {
    pub enabled: bool,
    pub mode: ProfileMode,
    pub ask_on_startup: bool,
    pub active: Option<ProfileId>,
    pub profiles: Vec<ProfileCard>,
    /// At least one profile has a PIN (mode changes then need one).
    pub any_locked: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_fields_take_safe_defaults() {
        let c: ProfilesConfig = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert!(c.enabled);
        assert!(c.ask_on_startup);
        assert_eq!(c.mode, ProfileMode::ServerUsers);
        assert!(!ProfilesConfig::default().enabled);
    }

    #[test]
    fn origin_roundtrips_as_tagged_json() {
        let o = Origin::Derived { key: "antoine".into() };
        let json = serde_json::to_string(&o).unwrap();
        assert_eq!(json, r#"{"kind":"derived","key":"antoine"}"#);
        assert_eq!(serde_json::from_str::<Origin>(&json).unwrap(), o);
    }

    #[test]
    fn profile_id_parses_from_its_display() {
        let id = ProfileId::new();
        assert_eq!(id.to_string().parse::<ProfileId>().unwrap(), id);
    }
}
