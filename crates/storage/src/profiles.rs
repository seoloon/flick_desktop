//! Profiles: who can be picked, resolved from `profiles.json` and the stored
//! connections. Pure (no I/O), so every rule is unit-tested here.

use std::collections::HashSet;

use oneshot_core::ServerId;
use oneshot_core::profile::{
    AccountState, AvatarStyle, DiscoveredUser, Origin, Profile, ProfileAccount, ProfileCard, ProfileId, ProfileMode, ProfilesConfig,
};
use oneshot_core::server::{ProviderKind, ServerDescriptor};
use oneshot_core::settings::PersonalSettings;
use url::Url;

use crate::images::avatar_cache_key;

pub use oneshot_core::text::normalize_name;

/// Profile colours: saturated enough to glow, light enough for white initials.
pub const PROFILE_COLORS: [&str; 8] = ["#5e8bff", "#ff6b6b", "#3ecf8e", "#ffb547", "#b07cff", "#ff7ac6", "#35c6d6", "#a3a3a3"];

#[derive(Debug, Clone)]
pub struct ResolvedAccount {
    pub kind: ProviderKind,
    pub server_name: String,
    pub base_url: Url,
    pub user_name: String,
    pub avatar: Option<Url>,
    /// Signed in here.
    pub connection: Option<ServerDescriptor>,
    /// Seen by discovery (a pending account, or a Plex Home member).
    pub discovered: Option<DiscoveredUser>,
}

#[derive(Debug, Clone)]
pub struct Resolved {
    pub profile: Profile,
    pub accounts: Vec<ResolvedAccount>,
}

fn color_for(key: &str) -> String {
    let h = key.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    PROFILE_COLORS[(h % PROFILE_COLORS.len() as u32) as usize].to_owned()
}

/// `d` is the connection of discovered user `u` (same physical server, same user).
fn same_user(d: &ServerDescriptor, servers: &[ServerDescriptor], u: &DiscoveredUser) -> bool {
    let Some(home) = servers.iter().find(|s| s.id == u.server) else { return false };
    d.kind == u.kind && d.remote_id == home.remote_id && d.user.id == u.remote_user_id
}

fn connected(d: &ServerDescriptor, servers: &[ServerDescriptor], discovered: &[DiscoveredUser]) -> ResolvedAccount {
    let seen = discovered.iter().find(|u| same_user(d, servers, u));
    ResolvedAccount {
        kind: d.kind,
        server_name: d.name.clone(),
        base_url: d.base_url.clone(),
        user_name: d.user.name.clone(),
        avatar: d.user.avatar.clone().or_else(|| seen.and_then(|u| u.avatar.clone())),
        connection: Some(d.clone()),
        discovered: seen.cloned(),
    }
}

/// The profiles of the current mode. In mode B, groups that have no profile
/// yet get one (appended to `config.profiles`; the caller saves).
pub fn resolve(config: &mut ProfilesConfig, servers: &[ServerDescriptor], defaults: &dyn Fn() -> PersonalSettings) -> Vec<Resolved> {
    match config.mode {
        ProfileMode::ServerUsers => derive(config, servers, defaults),
        ProfileMode::Local | ProfileMode::Linked => config
            .profiles
            .iter()
            .filter(|p| p.origin == Origin::Manual)
            .map(|p| Resolved {
                profile: p.clone(),
                accounts: p
                    .connections
                    .iter()
                    .filter_map(|id| servers.iter().find(|s| s.id == *id))
                    .map(|d| connected(d, servers, &config.discovered))
                    .collect(),
            })
            .collect(),
    }
}

fn derive(config: &mut ProfilesConfig, servers: &[ServerDescriptor], defaults: &dyn Fn() -> PersonalSettings) -> Vec<Resolved> {
    let detached: HashSet<ServerId> = config.profiles.iter().flat_map(|p| p.detached.iter().copied()).collect();
    // (key, display name, accounts), in order of first appearance.
    let mut groups: Vec<(String, String, Vec<ResolvedAccount>)> = Vec::new();
    let mut add = |key: String, name: &str, account: ResolvedAccount| match groups.iter_mut().find(|g| g.0 == key) {
        Some(g) => g.2.push(account),
        None => groups.push((key, name.to_owned(), vec![account])),
    };
    for d in servers {
        let key = if detached.contains(&d.id) { format!("detached:{}", d.id) } else { normalize_name(&d.user.name) };
        add(key, &d.user.name, connected(d, servers, &config.discovered));
    }
    for u in &config.discovered {
        if servers.iter().any(|d| same_user(d, servers, u)) {
            continue;
        }
        let Some(home) = servers.iter().find(|s| s.id == u.server) else { continue };
        let account = ResolvedAccount {
            kind: u.kind,
            server_name: home.name.clone(),
            base_url: home.base_url.clone(),
            user_name: u.name.clone(),
            avatar: u.avatar.clone(),
            connection: None,
            discovered: Some(u.clone()),
        };
        add(normalize_name(&u.name), &u.name, account);
    }
    groups
        .into_iter()
        .map(|(key, name, accounts)| {
            let origin = Origin::Derived { key: key.clone() };
            let profile = match config.profiles.iter().find(|p| p.origin == origin) {
                Some(p) => p.clone(),
                None => {
                    let p = Profile::new(name, color_for(&key), origin, defaults());
                    config.profiles.push(p.clone());
                    p
                }
            };
            Resolved { profile, accounts }
        })
        .collect()
}

/// What the catalogue loads for this profile.
pub fn connections_of(r: &Resolved) -> Vec<ServerId> {
    r.accounts.iter().filter_map(|a| a.connection.as_ref()).filter(|d| !d.disabled).map(|d| d.id).collect()
}

pub fn avatar_of(r: &Resolved) -> Option<Url> {
    match r.profile.avatar {
        AvatarStyle::Initials => None,
        AvatarStyle::Server => r.accounts.iter().find_map(|a| a.avatar.clone()),
    }
}

pub fn card(r: &Resolved, offline: &HashSet<ServerId>) -> ProfileCard {
    ProfileCard {
        id: r.profile.id,
        name: r.profile.name.clone(),
        color: r.profile.color.clone(),
        avatar_key: avatar_of(r).map(|u| avatar_cache_key(&u)[..12].to_owned()),
        locked: r.profile.pin.is_some(),
        hidden: r.profile.hidden,
        accounts: r
            .accounts
            .iter()
            .map(|a| {
                let connection = a.connection.as_ref().map(|d| d.id);
                let home = a.discovered.as_ref().map(|u| u.server);
                let state = if a.connection.as_ref().is_some_and(|d| d.disabled) {
                    AccountState::Disabled
                } else if connection.or(home).is_some_and(|s| offline.contains(&s)) {
                    AccountState::Offline
                } else if connection.is_some() {
                    AccountState::Connected
                } else {
                    AccountState::Pending
                };
                ProfileAccount {
                    kind: a.kind,
                    server_name: a.server_name.clone(),
                    user_name: a.user_name.clone(),
                    state,
                    connection,
                    base_url: a.base_url.clone(),
                    needs_password: connection.is_none() && a.discovered.as_ref().is_some_and(|u| u.has_password),
                    plex_pin: a.discovered.as_ref().is_some_and(|u| u.protected),
                }
            })
            .collect(),
    }
}

/// When multi-user is switched on, the profile that already holds what is
/// loaded (so nobody is thrown out of what they were watching). Never a
/// locked or hidden profile.
pub fn best_match(resolved: &[Resolved], loaded: &std::collections::HashSet<ServerId>) -> Option<ProfileId> {
    let mut best: Option<(usize, ProfileId)> = None;
    for r in resolved.iter().filter(|r| r.profile.pin.is_none() && !r.profile.hidden) {
        let n = connections_of(r).iter().filter(|id| loaded.contains(id)).count();
        if n > 0 && best.is_none_or(|(m, _)| n > m) {
            best = Some((n, r.profile.id));
        }
    }
    best.map(|(_, id)| id)
}

/// Connections the catalogue may load, in `servers` order: not disabled, in
/// `wanted` (the active profile's; `None` = multi-user off, everything), and
/// never a PIN-protected Plex user plex.tv has not checked in this run. With
/// multi-user off, only Home-member connections need that check (the
/// signed-in account's own connection loads as it always did).
pub fn loadable(
    servers: &[ServerDescriptor],
    discovered: &[DiscoveredUser],
    wanted: Option<&HashSet<ServerId>>,
    verified: &HashSet<ServerId>,
    multi_user: bool,
) -> Vec<ServerId> {
    servers
        .iter()
        .filter(|d| !d.disabled)
        .filter(|d| wanted.is_none_or(|w| w.contains(&d.id)))
        .filter(|d| {
            let protected = discovered.iter().any(|u| u.protected && same_user(d, servers, u));
            let gated = protected && (multi_user || d.home_member);
            !gated || verified.contains(&d.id)
        })
        .map(|d| d.id)
        .collect()
}

/// Other PIN-protected manual profiles using a connection that `next` adds
/// to profile `id`: linking it needs their PIN (modes A/C).
pub fn locked_owners(all: &[Profile], id: ProfileId, next: &[ServerId]) -> Vec<ProfileId> {
    let current: &[ServerId] = all.iter().find(|p| p.id == id).map_or(&[], |p| &p.connections);
    let added: Vec<&ServerId> = next.iter().filter(|c| !current.contains(c)).collect();
    all.iter()
        .filter(|p| p.id != id && p.origin == Origin::Manual && p.pin.is_some())
        .filter(|p| added.iter().any(|c| p.connections.contains(c)))
        .map(|p| p.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_across_case_accents_and_spaces() {
        assert_eq!(normalize_name("  Antoine "), "antoine");
        assert_eq!(normalize_name("ANTOINE"), normalize_name("antoine"));
        assert_eq!(normalize_name("Antoïne"), "antoine");
        assert_eq!(normalize_name("Élodie   Martin"), "elodie martin");
        assert_ne!(normalize_name("Léa"), normalize_name("Leo"));
    }

    use std::collections::HashSet;

    use oneshot_core::profile::{AccountState, DiscoveredUser, Origin, Profile, ProfileMode, ProfilesConfig};
    use oneshot_core::server::{ProviderKind, ServerDescriptor, UserProfile};
    use oneshot_core::settings::PersonalSettings;
    use oneshot_core::ServerId;
    use url::Url;

    fn conn(kind: ProviderKind, remote: &str, user_id: &str, user: &str) -> ServerDescriptor {
        ServerDescriptor {
            id: ServerId::new(),
            kind,
            name: format!("{remote} server"),
            remote_id: remote.into(),
            base_url: Url::parse(&format!("http://{remote}.local/")).unwrap(),
            alternate_urls: vec![],
            version: None,
            user: UserProfile { id: user_id.into(), name: user.into(), avatar: None, is_admin: false },
            disabled: false,
            home_member: false,
        }
    }

    fn seen(on: &ServerDescriptor, user_id: &str, name: &str) -> DiscoveredUser {
        DiscoveredUser { server: on.id, kind: on.kind, remote_user_id: user_id.into(), switch_id: None, name: name.into(), avatar: None, has_password: true, protected: false }
    }

    fn defaults() -> PersonalSettings {
        PersonalSettings::default()
    }

    #[test]
    fn groups_connections_by_normalised_name() {
        let jf = conn(ProviderKind::Jellyfin, "jf", "j1", "Antoine");
        let px = conn(ProviderKind::Plex, "px", "11", "antoine ");
        let lea = conn(ProviderKind::Jellyfin, "jf", "j2", "Léa");
        let mut cfg = ProfilesConfig::default();
        let r = resolve(&mut cfg, &[jf.clone(), px.clone(), lea.clone()], &defaults);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].profile.name, "Antoine");
        assert_eq!(connections_of(&r[0]), vec![jf.id, px.id]);
        assert_eq!(connections_of(&r[1]), vec![lea.id]);
    }

    #[test]
    fn derived_profiles_are_created_once_with_stable_ids() {
        let jf = conn(ProviderKind::Jellyfin, "jf", "j1", "Antoine");
        let mut cfg = ProfilesConfig::default();
        let first = resolve(&mut cfg, std::slice::from_ref(&jf), &defaults);
        let again = resolve(&mut cfg, std::slice::from_ref(&jf), &defaults);
        assert_eq!(cfg.profiles.len(), 1);
        assert_eq!(first[0].profile.id, again[0].profile.id);
        assert_eq!(cfg.profiles[0].origin, Origin::Derived { key: "antoine".into() });
    }

    #[test]
    fn discovered_users_become_pending_accounts_without_duplicating_connected_ones() {
        let jf = conn(ProviderKind::Jellyfin, "jf", "j1", "Antoine");
        let mut cfg = ProfilesConfig { discovered: vec![seen(&jf, "j1", "Antoine"), seen(&jf, "j9", "Kid")], ..Default::default() };
        let r = resolve(&mut cfg, std::slice::from_ref(&jf), &defaults);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].accounts.len(), 1, "Antoine is not listed twice");
        let kid = card(&r[1], &HashSet::new());
        assert_eq!(kid.name, "Kid");
        assert_eq!(kid.accounts[0].state, AccountState::Pending);
        assert!(kid.accounts[0].needs_password);
        assert!(connections_of(&r[1]).is_empty());
    }

    #[test]
    fn other_users_connection_is_not_in_active_profile() {
        let me = conn(ProviderKind::Jellyfin, "jf", "j1", "Antoine");
        let kid = conn(ProviderKind::Jellyfin, "jf", "j9", "Kid");
        let mut cfg = ProfilesConfig::default();
        let r = resolve(&mut cfg, &[me.clone(), kid.clone()], &defaults);
        let mine = r.iter().find(|p| p.profile.name == "Antoine").unwrap();
        assert_eq!(connections_of(mine), vec![me.id]);
    }

    #[test]
    fn detached_connection_forms_its_own_profile() {
        let a = conn(ProviderKind::Jellyfin, "jf", "j1", "Alex");
        let b = conn(ProviderKind::Plex, "px", "11", "alex");
        let mut cfg = ProfilesConfig::default();
        let before = resolve(&mut cfg, &[a.clone(), b.clone()], &defaults);
        assert_eq!(before.len(), 1);
        cfg.profiles[0].detached.push(b.id);
        let after = resolve(&mut cfg, &[a.clone(), b.clone()], &defaults);
        assert_eq!(after.len(), 2);
        assert_eq!(connections_of(&after[0]), vec![a.id]);
        assert_eq!(connections_of(&after[1]), vec![b.id]);
    }

    #[test]
    fn disabled_connections_are_listed_but_not_loaded() {
        let mut jf = conn(ProviderKind::Jellyfin, "jf", "j1", "Antoine");
        jf.disabled = true;
        let mut cfg = ProfilesConfig::default();
        let r = resolve(&mut cfg, &[jf], &defaults);
        assert_eq!(r[0].accounts.len(), 1);
        assert!(connections_of(&r[0]).is_empty());
        assert_eq!(card(&r[0], &HashSet::new()).accounts[0].state, AccountState::Disabled);
    }

    #[test]
    fn manual_modes_list_their_own_connections_only() {
        let a = conn(ProviderKind::Jellyfin, "jf", "j1", "Antoine");
        let b = conn(ProviderKind::Plex, "px", "11", "Antoine");
        let mut p = Profile::new("Salon", "#5e8bff", Origin::Manual, defaults());
        p.connections = vec![b.id, ServerId::new() /* removed since */];
        let mut cfg = ProfilesConfig { mode: ProfileMode::Linked, profiles: vec![p], ..Default::default() };
        let r = resolve(&mut cfg, &[a, b.clone()], &defaults);
        assert_eq!(r.len(), 1);
        assert_eq!(connections_of(&r[0]), vec![b.id]);
    }

    #[test]
    fn offline_and_protected_accounts_show_on_cards() {
        let px = conn(ProviderKind::Plex, "px", "11", "Antoine");
        let mut member = seen(&px, "12", "Léa");
        member.protected = true;
        member.has_password = false;
        let mut cfg = ProfilesConfig { discovered: vec![member], ..Default::default() };
        let r = resolve(&mut cfg, std::slice::from_ref(&px), &defaults);
        let offline: HashSet<ServerId> = [px.id].into();
        assert_eq!(card(&r[0], &offline).accounts[0].state, AccountState::Offline);
        let lea = card(&r[1], &HashSet::new());
        assert!(lea.accounts[0].plex_pin);
        assert!(!lea.accounts[0].needs_password);
    }

    #[test]
    fn best_match_picks_the_profile_holding_the_loaded_connections() {
        let a = conn(ProviderKind::Jellyfin, "jf", "j1", "Antoine");
        let b = conn(ProviderKind::Plex, "px", "11", "Antoine");
        let k = conn(ProviderKind::Jellyfin, "jf", "j9", "Kid");
        let mut cfg = ProfilesConfig::default();
        let r = resolve(&mut cfg, &[a.clone(), b.clone(), k.clone()], &defaults);
        let loaded: HashSet<ServerId> = [a.id, b.id, k.id].into();
        assert_eq!(best_match(&r, &loaded), Some(r[0].profile.id));
        let mut locked = r.clone();
        locked[0].profile.pin = Some("hash".into());
        assert_eq!(best_match(&locked, &loaded), Some(r[1].profile.id), "a locked profile is never entered silently");
        assert_eq!(best_match(&r, &HashSet::new()), None);
    }

    #[test]
    fn avatar_keys_follow_the_picture() {
        let mut jf = conn(ProviderKind::Jellyfin, "jf", "j1", "Antoine");
        jf.user.avatar = Some(Url::parse("https://plex.tv/users/a/avatar").unwrap());
        let mut cfg = ProfilesConfig::default();
        let r = resolve(&mut cfg, std::slice::from_ref(&jf), &defaults);
        let key = card(&r[0], &HashSet::new()).avatar_key.unwrap();
        assert_eq!(key.len(), 12);
        cfg.profiles[0].avatar = oneshot_core::profile::AvatarStyle::Initials;
        let r = resolve(&mut cfg, &[jf], &defaults);
        assert_eq!(card(&r[0], &HashSet::new()).avatar_key, None);
    }

    fn protected_member(on: &ServerDescriptor, d: &ServerDescriptor) -> DiscoveredUser {
        DiscoveredUser { protected: true, has_password: false, ..seen(on, &d.user.id, &d.user.name) }
    }

    #[test]
    fn multi_user_off_loads_everything_but_unverified_protected_home_members() {
        let owner = conn(ProviderKind::Plex, "px", "11", "Antoine");
        let mut kid = conn(ProviderKind::Plex, "px", "12", "Léa");
        kid.home_member = true;
        let jf = conn(ProviderKind::Jellyfin, "jf", "j1", "Antoine");
        let servers = [owner.clone(), kid.clone(), jf.clone()];
        // The account owner can be PIN-protected too: that never gated it before.
        let discovered = [protected_member(&owner, &owner), protected_member(&owner, &kid)];
        let none = HashSet::new();
        assert_eq!(loadable(&servers, &discovered, None, &none, false), vec![owner.id, jf.id]);
        let verified: HashSet<ServerId> = [kid.id].into();
        assert_eq!(loadable(&servers, &discovered, None, &verified, false), vec![owner.id, kid.id, jf.id]);
        let open_member = [seen(&owner, "12", "Léa")];
        assert_eq!(loadable(&servers, &open_member, None, &none, false), vec![owner.id, kid.id, jf.id], "an open member needs no PIN");
    }

    #[test]
    fn multi_user_on_loads_the_profile_and_gates_every_protected_plex_user() {
        let owner = conn(ProviderKind::Plex, "px", "11", "Antoine");
        let mut kid = conn(ProviderKind::Plex, "px", "12", "Léa");
        kid.home_member = true;
        let jf = conn(ProviderKind::Jellyfin, "jf", "j1", "Antoine");
        let servers = [owner.clone(), kid.clone(), jf.clone()];
        let discovered = [protected_member(&owner, &owner), protected_member(&owner, &kid)];
        let none = HashSet::new();
        let mine: HashSet<ServerId> = [owner.id, jf.id].into();
        assert_eq!(loadable(&servers, &discovered, Some(&mine), &none, true), vec![jf.id], "protected, not home_member, still unverified");
        let verified: HashSet<ServerId> = [owner.id].into();
        assert_eq!(loadable(&servers, &discovered, Some(&mine), &verified, true), vec![owner.id, jf.id]);
        let hers: HashSet<ServerId> = [kid.id].into();
        assert!(loadable(&servers, &discovered, Some(&hers), &verified, true).is_empty(), "verifying one user does not open another");
        assert!(loadable(&servers, &discovered, Some(&HashSet::new()), &verified, true).is_empty(), "nobody picked, nothing loaded");
    }

    #[test]
    fn disabled_connections_never_load() {
        let mut jf = conn(ProviderKind::Jellyfin, "jf", "j1", "Antoine");
        jf.disabled = true;
        let mut kid = conn(ProviderKind::Plex, "px", "12", "Léa");
        kid.home_member = true;
        kid.disabled = true;
        let servers = [jf.clone(), kid.clone()];
        let discovered = [protected_member(&kid, &kid)];
        let all: HashSet<ServerId> = [jf.id, kid.id].into();
        assert!(loadable(&servers, &discovered, None, &all, false).is_empty());
        assert!(loadable(&servers, &discovered, Some(&all), &all, true).is_empty());
    }

    fn manual(name: &str, connections: &[ServerId], pin: bool) -> Profile {
        let mut p = Profile::new(name, "#5e8bff", Origin::Manual, defaults());
        p.connections = connections.to_vec();
        p.pin = pin.then(|| "hash".to_owned());
        p
    }

    #[test]
    fn linking_a_locked_profiles_connection_names_its_owner() {
        let (shared, parents, open) = (ServerId::new(), ServerId::new(), ServerId::new());
        let kid = manual("Kid", &[shared], false);
        let parent = manual("Parent", &[shared, parents], true);
        let guest = manual("Guest", &[open], false);
        let all = [kid.clone(), parent.clone(), guest];
        assert!(locked_owners(&all, kid.id, &[shared]).is_empty(), "already linked: no new PIN");
        assert!(locked_owners(&all, kid.id, &[shared, open]).is_empty(), "an open profile's connection is free");
        assert_eq!(locked_owners(&all, kid.id, &[shared, parents]), vec![parent.id]);
        assert!(locked_owners(&all, parent.id, &[shared, parents, open]).is_empty(), "a profile never guards itself");
    }

    #[test]
    fn attaching_a_connection_to_the_active_profile_respects_locked_owners() {
        let parents = ServerId::new();
        let kid = manual("Kid", &[], false);
        let parent = manual("Parent", &[parents], true);
        let all = [kid.clone(), parent.clone()];
        assert_eq!(locked_owners(&all, kid.id, &[parents]), vec![parent.id], "a locked profile's connection is owned");
        assert!(locked_owners(&all, kid.id, &[ServerId::new()]).is_empty(), "a brand-new connection is free");
    }
}
