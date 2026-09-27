# Profils multi-utilisateurs + bouton retour — Plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ajouter à Flick des profils multi-utilisateurs (3 modes, B par défaut) avec écran de sélection animé, changement rapide depuis la sidebar, PIN optionnel, réglages personnels par profil — et un bouton retour sur la fiche et la grille de bibliothèque.

**Architecture:** `servers.json` reste le réservoir de toutes les connexions (serveur × utilisateur). Un nouveau `profiles.json` décrit les profils ; un profil = un ensemble de connexions + des préférences. Le profil actif décide quelles connexions sont chargées dans le `Catalog` ; changer de profil = remplacer les providers du catalogue, appliquer ses préférences, vider le cache React Query. Les règles (regroupement par nom, PIN, verrous, fusion des réglages) sont pures et testées dans `oneshot-core` / `oneshot-storage` ; `app` ne fait que les câbler.

**Tech Stack:** Rust 2024 (Tauri 2, parking_lot, tokio, argon2, unicode-normalization, wiremock pour les tests), React 19 + TypeScript strict, TanStack Query, Zustand, Motion, Norigin (via `useTv` / `FocusGroup`), Vitest.

**Spec:** `docs/superpowers/specs/2026-09-27-multi-user-profiles-design.md`

## Global Constraints

- Multi-utilisateurs **désactivé** (défaut, ou pas de `profiles.json`) = comportement actuel à l'identique : toutes les connexions non désactivées sont chargées.
- Défauts : `enabled: false`, `mode: ServerUsers` (B), `ask_on_startup: true`.
- Textes de l'interface **en anglais**, comme le reste de l'app (« Who's watching? », « Other User », « Profiles »…) ; les libellés français du spec sont des traductions.
- Interface **monochrome** : la couleur d'un profil ne teinte que son avatar et la lumière ambiante (cf. `docs/DESIGN_SYSTEM.md`).
- Les tokens ne quittent jamais Rust ; la WebView ne charge aucune origine distante (CSP) → les avatars passent par `oneshot-img` (`avatar/<profile-id>/<key>`).
- PIN : exactement 4 chiffres, hash **argon2id** + sel, jamais stocké ni journalisé en clair. Après 5 échecs : 30 s, puis 60 s, puis 300 s. Le PIN Plex n'est jamais stocké.
- Verrous vérifiés **côté Rust** : modifier/supprimer un profil protégé → son PIN ; désactiver le multi-utilisateurs ou changer de mode → le PIN d'un profil protégé s'il en existe un.
- Découverte : délai **3 s** par serveur, en parallèle ; le dernier résultat reste en cache dans `profiles.json`.
- 8 couleurs de profil fixes : `#5e8bff #ff6b6b #3ecf8e #ffb547 #b07cff #ff7ac6 #35c6d6 #a3a3a3`.
- Mouvement : ressorts de `ui/src/lib/motion.ts` ; tuiles en cascade 60 ms (y +24 px, ×0,92, flou 8 px → 0) ; focus tuile ×1,1, autres tuiles à 55 % ; logo qui respire 0,96 ↔ 1 ; flou/fondu du contenu ≈ 200 ms au changement ; objectif < 500 ms cache chaud.
- Tout élément interactif passe par `useTv` / `Button` / `FocusGroup` (navigation manette/télécommande).
- Jamais de `filter` autre que `none` laissé sur un ancêtre de panneaux `glass` au repos (il couperait leur flou d'arrière-plan) : utiliser `transitionEnd: { filter: "none" }`.
- Chaque tâche se termine par un commit dont le message finit par `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Vérifs Rust : `cargo test -p <crate>` ; `cargo clippy --workspace` sans **nouvel** avertissement. Vérifs UI : `pnpm --dir ui run typecheck` et `pnpm --dir ui run test`.

## Review Focus

1. **Fuite de données entre profils au changement** : après un changement, aucune donnée du profil précédent ne doit s'afficher (cache React Query, fond ambiant, catalogue). → test Vitest « switchProfile vide le cache » (Tâche 11) + test Rust « la connexion d'un autre utilisateur n'est pas dans le profil actif » (Tâche 5).
2. **Contournement du PIN par un enfant** : désactiver le mode, changer de mode, ou retirer le PIN d'un profil protégé sans PIN. → tests `authorize_config_change` et `check_pin` (Tâche 6).
3. **Regroupement par nom faux** : « Antoine » / « antoine » / « Antoïne » doivent fusionner ; deux personnes homonymes se séparent par « Détacher ». → tests `normalize_name` (Tâche 4) et `detached_connection_forms_its_own_profile` (Tâche 5).
4. **Connexion ajoutée pendant qu'un profil est actif** (mode B, « Other User ») qui apparaîtrait dans le catalogue du profil courant. → test `other_users_connection_is_not_in_active_profile` (Tâche 5) ; câblage `wanted()` (Tâche 9).
5. **Mise à jour depuis une version sans `profiles.json` / fichier corrompu** : l'app doit démarrer comme avant. → tests `absent_profiles_file_means_disabled` et `corrupt_profiles_file_is_kept_aside` (Tâche 4).

---

## Carte des fichiers

| Fichier | Rôle |
|---|---|
| `ui/src/components/tv/BackButton.tsx` (nouveau) | Bouton retour en verre |
| `ui/src/lib/history.ts` (+ test) (nouveau) | `hasAppHistory`, `goBack` |
| `crates/core/src/settings.rs` | `PersonalSettings`, `split_settings` |
| `crates/core/src/error.rs` | `WrongPin`, `PinLocked` |
| `crates/core/src/profile.rs` (nouveau) | Types de profils (persistés + vues UI exportées en TS) |
| `crates/storage/src/lib.rs` | `Store::profiles` / `save_profiles` |
| `crates/storage/src/profiles.rs` (nouveau) | Normalisation, résolution/regroupement, cartes, `best_match` |
| `crates/storage/src/pin.rs` (nouveau) | Hash PIN, `PinGuard`, `check_pin`, `authorize_config_change` |
| `crates/storage/src/images.rs` | `avatar_cache_key` |
| `crates/providers/jellyfin/src/{auth,dto,lib}.rs` | `public_users` |
| `crates/providers/plex/src/{auth,dto,lib}.rs` | `home_users`, `switch_user` |
| `crates/catalog/src/lib.rs` | `Catalog::replace` |
| `app/src/state.rs` | Profil actif, connexions voulues, activation, PIN |
| `app/src/commands/profiles.rs` (nouveau) | Commandes IPC des profils |
| `app/src/commands/servers.rs` | `register_plex`, `plex_account_token`, `register_server` renvoie le descripteur |
| `app/src/commands/system.rs` | Réglages effectifs |
| `app/src/images.rs` | Route `avatar/…` |
| `app/src/main.rs` | Initialisation, reprise du dernier profil, enregistrement des commandes |
| `ui/src/lib/queryClient.ts` (nouveau) | `QueryClient` partagé |
| `ui/src/lib/profiles.ts` (+ test) (nouveau) | Requête, orchestration du changement, helpers purs |
| `ui/src/lib/ambient.ts` | `ambientColor`, `ambientReset` |
| `ui/src/components/tv/ProfileAvatar.tsx`, `AccountPills.tsx`, `PinPad.tsx` (nouveaux) | Briques visuelles |
| `ui/src/features/profiles/*` (nouveau) | `ProfilePicker`, `ProfileTile`, `AccountSignIn`, `OtherUserDialog`, `ProfileGate`, `ProfileSwitcher`, `ProfilesSettings`, `ProfileEditor` |
| `ui/src/shell/{Sidebar,TabBar,Shell}.tsx`, `ui/src/App.tsx`, `ui/src/features/settings/Settings.tsx` | Intégration |
| `ARCHITECTURE.md`, `docs/DESIGN_SYSTEM.md` | Documentation |

---

### Task 1: Bouton retour

**Files:**
- Create: `ui/src/lib/history.ts`, `ui/src/lib/history.test.ts`, `ui/src/components/tv/BackButton.tsx`
- Modify: `ui/src/features/detail/Detail.tsx`, `ui/src/features/library/LibraryGrid.tsx`, `ui/src/App.tsx` (`GlobalActions`)
- Modify: `docs/superpowers/specs/2026-09-27-multi-user-profiles-design.md` §7 (taille `icon`, pas `icon-lg`)

**Interfaces:**
- Produces: `hasAppHistory(state: unknown): boolean`, `goBack(navigate: NavigateFunction): void`, `<BackButton className? />`

- [ ] **Step 1: Write the failing test** — `ui/src/lib/history.test.ts`

```ts
import { describe, expect, it } from "vitest";
import { hasAppHistory } from "./history";

describe("hasAppHistory", () => {
  it("is true only past the first in-app entry", () => {
    expect(hasAppHistory({ idx: 2, key: "k" })).toBe(true);
    expect(hasAppHistory({ idx: 0, key: "k" })).toBe(false);
    expect(hasAppHistory(null)).toBe(false);
    expect(hasAppHistory({})).toBe(false);
    expect(hasAppHistory({ idx: "3" })).toBe(false);
  });
});
```

- [ ] **Step 2: Run it to see it fail**

Run: `pnpm --dir ui run test -- history`
Expected: FAIL — `Cannot find module './history'`

- [ ] **Step 3: Implement** — `ui/src/lib/history.ts`

```ts
// Back inside the app: react-router numbers its history entries (`idx`); the
// first one has nothing of ours behind it, so Back goes Home instead of
// leaving the app (a detail page opened directly, a reload).
import type { NavigateFunction } from "react-router";

export function hasAppHistory(state: unknown): boolean {
  if (typeof state !== "object" || state === null || !("idx" in state)) return false;
  const idx = (state as { idx: unknown }).idx;
  return typeof idx === "number" && idx > 0;
}

export function goBack(navigate: NavigateFunction) {
  if (hasAppHistory(window.history.state)) void navigate(-1);
  else void navigate("/");
}
```

- [ ] **Step 4: Run the test**

Run: `pnpm --dir ui run test -- history`
Expected: PASS

- [ ] **Step 5: Create the component** — `ui/src/components/tv/BackButton.tsx`

```tsx
// Glass back arrow for pages reached by drilling in (a title, a library).
// Remote users also have Back (Escape, B); this is for the pointer.
import { ArrowLeft } from "lucide-react";
import { useNavigate } from "react-router";
import { goBack } from "@/lib/history";
import { Button } from "./Button";

export function BackButton({ className }: { className?: string }) {
  const navigate = useNavigate();
  return <Button size="icon" icon={ArrowLeft} label="Back" focusKey="page-back" onClick={() => goBack(navigate)} className={className} />;
}
```

- [ ] **Step 6: Place it on the detail page** — in `ui/src/features/detail/Detail.tsx`, add the import and, as the first child of the hero `<section className="relative flex min-h-[max(36rem,80vh)] flex-col justify-end">`, right after `<HeroBackdrop … />`:

```tsx
import { BackButton } from "@/components/tv/BackButton";
// …
        <BackButton className="absolute top-[var(--page-top)] left-[var(--gutter)] z-10" />
```

- [ ] **Step 7: Place it on the library grid** — in `ui/src/features/library/LibraryGrid.tsx`, import `BackButton` and put it as the first child inside `<Screen ready={total !== null}>`, wrapped so it sits in the gutter above the header:

```tsx
      <div className="px-[var(--gutter)] pt-[var(--page-top)] -mb-[var(--page-top)]">
        <BackButton />
      </div>
```

The negative bottom margin cancels the header's own top padding so the title does not move down; the button sits in the space above the title.

- [ ] **Step 8: Use the same rule for the Back key** — in `ui/src/App.tsx`, `GlobalActions`:

```tsx
import { goBack } from "@/lib/history";
// …
    setBackFallback(() => goBack(navigate));
```

- [ ] **Step 9: Align the spec** — in the spec §7 replace `` `size="icon-lg"` `` with `` `size="icon"` (44 px) ``.

- [ ] **Step 10: Verify**

Run: `pnpm --dir ui run typecheck && pnpm --dir ui run test`
Expected: no type error, all tests PASS.
Then `pnpm desktop` (or the project's usual dev command), open a title from Home: the arrow shows top-left under the title bar; click → back to Home at the same scroll/focus; with arrows, Up from the Play button reaches it. Open a library: the arrow sits above the title without overlapping it.

- [ ] **Step 11: Commit**

```bash
git add ui/src/lib/history.ts ui/src/lib/history.test.ts ui/src/components/tv/BackButton.tsx ui/src/features/detail/Detail.tsx ui/src/features/library/LibraryGrid.tsx ui/src/App.tsx docs/superpowers/specs/2026-09-27-multi-user-profiles-design.md
git commit -m "UI: back button on title and library pages

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Réglages personnels + erreurs de PIN (core)

**Files:**
- Modify: `crates/core/src/settings.rs`, `crates/core/src/error.rs`

**Interfaces:**
- Produces:
  - `pub struct PersonalSettings { subtitles, preferred_audio_languages, autoplay_next, autoplay_countdown_secs, skip_intro, skip_credits, max_bitrate, resume, accent, animation_intensity, background_intensity }` (serde camelCase, `default`)
  - `PersonalSettings::from_settings(&Settings) -> PersonalSettings`, `PersonalSettings::apply(&self, &mut Settings)`
  - `pub fn split_settings(effective: &Settings, base: &Settings) -> (Settings, PersonalSettings)`
  - `Error::WrongPin` (kind `"wrongPin"`), `Error::PinLocked(u64)` (kind `"pinLocked"`, message `"too many attempts, try again in {n} s"`)

- [ ] **Step 1: Write the failing tests** — append inside `mod tests` of `crates/core/src/settings.rs`:

```rust
    #[test]
    fn split_keeps_personal_fields_out_of_the_shared_base() {
        let mut base = Settings::default();
        base.audio.passthrough = true;
        let mut effective = base.clone();
        effective.subtitles.scale = 1.4;
        effective.playback.preferred_audio_languages = vec!["fra".into()];
        effective.playback.autoplay_next = false;
        effective.audio.volume = 70; // a machine setting changed while a profile is active

        let (shared, prefs) = split_settings(&effective, &base);
        assert_eq!(prefs.subtitles.scale, 1.4);
        assert_eq!(prefs.preferred_audio_languages, vec!["fra".to_string()]);
        assert!(!prefs.autoplay_next);
        assert_eq!(shared.audio.volume, 70);
        assert!(shared.audio.passthrough);
        assert_eq!(shared.subtitles.scale, base.subtitles.scale, "the base keeps its own defaults");

        let mut merged = shared.clone();
        prefs.apply(&mut merged);
        assert_eq!(merged, effective);
    }

    #[test]
    fn personal_settings_fill_defaults_from_partial_json() {
        let p: PersonalSettings = serde_json::from_str(r#"{"autoplayNext":false}"#).unwrap();
        assert!(!p.autoplay_next);
        assert_eq!(p.subtitles, SubtitleSettings::default());
        assert_eq!(p.resume, PlaybackSettings::default().resume);
    }
```

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p oneshot-core settings::`
Expected: FAIL — `cannot find function split_settings` / `PersonalSettings`.

- [ ] **Step 3: Implement** — in `crates/core/src/settings.rs`, after `impl Default for SubtitleSettings` (anywhere at module level):

```rust
/// What belongs to a person rather than to the machine (multi-user
/// profiles): languages, subtitle look, autoplay and skipping, quality cap,
/// motion. Everything else (audio output, decoding, network, cache,
/// controller…) stays shared.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PersonalSettings {
    pub subtitles: SubtitleSettings,
    pub preferred_audio_languages: Vec<String>,
    pub autoplay_next: bool,
    pub autoplay_countdown_secs: u32,
    pub skip_intro: SkipMode,
    pub skip_credits: SkipMode,
    pub max_bitrate: Option<u64>,
    pub resume: ResumeBehavior,
    pub accent: String,
    pub animation_intensity: f32,
    pub background_intensity: f32,
}

impl Default for PersonalSettings {
    fn default() -> Self {
        Self::from_settings(&Settings::default())
    }
}

impl PersonalSettings {
    pub fn from_settings(s: &Settings) -> Self {
        Self {
            subtitles: s.subtitles.clone(),
            preferred_audio_languages: s.playback.preferred_audio_languages.clone(),
            autoplay_next: s.playback.autoplay_next,
            autoplay_countdown_secs: s.playback.autoplay_countdown_secs,
            skip_intro: s.playback.skip_intro,
            skip_credits: s.playback.skip_credits,
            max_bitrate: s.playback.max_bitrate,
            resume: s.playback.resume,
            accent: s.appearance.accent.clone(),
            animation_intensity: s.appearance.animation_intensity,
            background_intensity: s.appearance.background_intensity,
        }
    }

    pub fn apply(&self, s: &mut Settings) {
        s.subtitles = self.subtitles.clone();
        s.playback.preferred_audio_languages = self.preferred_audio_languages.clone();
        s.playback.autoplay_next = self.autoplay_next;
        s.playback.autoplay_countdown_secs = self.autoplay_countdown_secs;
        s.playback.skip_intro = self.skip_intro;
        s.playback.skip_credits = self.skip_credits;
        s.playback.max_bitrate = self.max_bitrate;
        s.playback.resume = self.resume;
        s.appearance.accent = self.accent.clone();
        s.appearance.animation_intensity = self.animation_intensity;
        s.appearance.background_intensity = self.background_intensity;
    }
}

/// Splits settings edited while a profile is active: the personal part goes
/// to the profile; the rest becomes the new shared base, which keeps its own
/// personal values (the defaults new profiles start from).
pub fn split_settings(effective: &Settings, base: &Settings) -> (Settings, PersonalSettings) {
    let mut shared = effective.clone();
    PersonalSettings::from_settings(base).apply(&mut shared);
    (shared, PersonalSettings::from_settings(effective))
}
```

- [ ] **Step 4: Add the PIN errors** — in `crates/core/src/error.rs`, add two variants before `Other` and their kinds:

```rust
    #[error("wrong PIN")]
    WrongPin,
    #[error("too many attempts, try again in {0} s")]
    PinLocked(u64),
```

```rust
            Self::WrongPin => "wrongPin",
            Self::PinLocked(_) => "pinLocked",
```

- [ ] **Step 5: Run tests and build**

Run: `cargo test -p oneshot-core && cargo build --workspace`
Expected: PASS; the build reports any other exhaustive `match` on `Error` — add the two arms there if so (none are expected).

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/settings.rs crates/core/src/error.rs
git commit -m "Core: personal settings split and PIN errors

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Types de profils (core)

**Files:**
- Create: `crates/core/src/profile.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod profile;`)

**Interfaces:**
- Consumes: `PersonalSettings` (Task 2), `ServerId`, `ProviderKind`
- Produces (all `serde(rename_all = "camelCase")`):
  - `ProfileId(Uuid)` — `new()`, `Display`, `FromStr`, TS `string`
  - `ProfileMode { ServerUsers (default), Local, Linked }` — TS
  - `AvatarStyle { Server (default), Initials }` — TS
  - `Origin { Manual (default), Derived { key: String } }` (tag `kind`)
  - `Profile { id, name, avatar, color, pin: Option<String>, connections: Vec<ServerId>, prefs: PersonalSettings, origin, hidden, detached: Vec<ServerId> }` + `Profile::new(name, color, origin, prefs)`
  - `DiscoveredUser { server: ServerId, kind, remote_user_id, switch_id: Option<String>, name, avatar: Option<Url>, has_password, protected }`
  - `ProfilesConfig { enabled, mode, ask_on_startup, last_profile, profiles, discovered }` (default: `false`, `ServerUsers`, `true`)
  - UI views, exported to TS: `AccountState { Connected, Pending, Offline }`, `ProfileAccount { kind, server_name, user_name, state, connection: Option<ServerId>, base_url: Url, needs_password, plex_pin }`, `ProfileCard { id, name, color, avatar_key: Option<String>, locked, hidden, accounts }`, `ProfilesState { enabled, mode, ask_on_startup, active: Option<ProfileId>, profiles: Vec<ProfileCard>, any_locked }`

- [ ] **Step 1: Write the file with its tests** — `crates/core/src/profile.rs`

```rust
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
```

- [ ] **Step 2: Register the module** — in `crates/core/src/lib.rs` add `pub mod profile;` after `pub mod playback;`.

- [ ] **Step 3: Run the tests**

Run: `cargo test -p oneshot-core profile::`
Expected: PASS (3 tests).

- [ ] **Step 4: Export the TS bindings**

Run: `cargo test -p oneshot-core --features ts export_bindings`
Expected: new files in `ui/src/ipc/bindings/`: `ProfileId.ts`, `ProfileMode.ts`, `AvatarStyle.ts`, `AccountState.ts`, `ProfileAccount.ts`, `ProfileCard.ts`, `ProfilesState.ts`.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/profile.rs crates/core/src/lib.rs ui/src/ipc/bindings
git commit -m "Core: profile types

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Persistance de `profiles.json` + normalisation des noms (storage)

**Files:**
- Modify: `crates/storage/Cargo.toml`, `crates/storage/src/lib.rs`
- Create: `crates/storage/src/profiles.rs` (first part)

**Interfaces:**
- Produces: `Store::profiles(&self) -> ProfilesConfig`, `Store::save_profiles(&self, &ProfilesConfig) -> Result<()>`, `profiles::normalize_name(&str) -> String`

- [ ] **Step 1: Add dependencies** — `crates/storage/Cargo.toml`, under `[dependencies]`:

```toml
argon2 = "0.5"
unicode-normalization = "0.1"
url.workspace = true
```

- [ ] **Step 2: Write the failing tests** — append to `mod tests` in `crates/storage/src/lib.rs`:

```rust
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
```

And create `crates/storage/src/profiles.rs` with only its test module for now:

```rust
//! Profiles: who can be picked, resolved from `profiles.json` and the stored
//! connections. Pure (no I/O), so every rule is unit-tested here.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

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
}
```

Add `pub mod profiles;` next to `pub mod images;` in `crates/storage/src/lib.rs`.

- [ ] **Step 3: Run to see it fail**

Run: `cargo test -p oneshot-storage`
Expected: FAIL — `no method named profiles`, `cannot find function normalize_name`.

- [ ] **Step 4: Implement the store methods** — in `impl Store` (`crates/storage/src/lib.rs`), after `save_servers`, and extend the module doc list with `profiles.json`:

```rust
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
```

with `use oneshot_core::profile::ProfilesConfig;` at the top.

- [ ] **Step 5: Implement `normalize_name`** — in `crates/storage/src/profiles.rs`, above the tests:

```rust
/// Grouping key for a person's name: case, accents and spacing ignored.
pub fn normalize_name(name: &str) -> String {
    let folded: String = name.nfkd().filter(|c| !is_combining_mark(*c)).collect::<String>().to_lowercase();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}
```

- [ ] **Step 6: Run the tests**

Run: `cargo test -p oneshot-storage`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/storage Cargo.lock
git commit -m "Storage: profiles.json and name normalisation

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Résolution et regroupement des profils (storage)

**Files:**
- Modify: `crates/storage/src/profiles.rs`, `crates/storage/src/images.rs`

**Interfaces:**
- Consumes: Task 3 types, `normalize_name`
- Produces:
  - `pub const PROFILE_COLORS: [&str; 8]`
  - `pub struct ResolvedAccount { kind, server_name, base_url: Url, user_name, avatar: Option<Url>, connection: Option<ServerDescriptor>, discovered: Option<DiscoveredUser> }`
  - `pub struct Resolved { profile: Profile, accounts: Vec<ResolvedAccount> }`
  - `pub fn resolve(config: &mut ProfilesConfig, servers: &[ServerDescriptor], defaults: &dyn Fn() -> PersonalSettings) -> Vec<Resolved>` (creates and appends missing derived profiles to `config.profiles`)
  - `pub fn connections_of(r: &Resolved) -> Vec<ServerId>` (connected, not disabled)
  - `pub fn avatar_of(r: &Resolved) -> Option<Url>`
  - `pub fn card(r: &Resolved, offline: &HashSet<ServerId>) -> ProfileCard`
  - `pub fn best_match(resolved: &[Resolved], loaded: &HashSet<ServerId>) -> Option<ProfileId>`
  - `images::avatar_cache_key(url: &Url) -> String` (64 hex chars)

- [ ] **Step 1: Write the failing tests** — in `crates/storage/src/profiles.rs` `mod tests`, add:

```rust
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
```

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p oneshot-storage profiles::`
Expected: FAIL — `cannot find function resolve` etc.

- [ ] **Step 3: Add `avatar_cache_key`** — in `crates/storage/src/images.rs`, after `cache_key`:

```rust
/// Cache key for a profile picture fetched from a public URL.
pub fn avatar_cache_key(url: &url::Url) -> String {
    let mut h = Sha256::new();
    h.update(b"avatar\0");
    h.update(url.as_str().as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}
```

- [ ] **Step 4: Implement** — in `crates/storage/src/profiles.rs`, replace the `use` lines at the top and add the code above the tests:

```rust
use std::collections::HashSet;

use oneshot_core::ServerId;
use oneshot_core::profile::{
    AccountState, AvatarStyle, DiscoveredUser, Origin, Profile, ProfileAccount, ProfileCard, ProfileId, ProfileMode, ProfilesConfig,
};
use oneshot_core::server::{ProviderKind, ServerDescriptor};
use oneshot_core::settings::PersonalSettings;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;
use url::Url;

use crate::images::avatar_cache_key;

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
                let state = if connection.or(home).is_some_and(|s| offline.contains(&s)) {
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
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p oneshot-storage`
Expected: PASS (all).

- [ ] **Step 6: Commit**

```bash
git add crates/storage
git commit -m "Storage: resolve profiles from connections and server users

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: PIN, temporisation et verrous (storage)

**Files:**
- Create: `crates/storage/src/pin.rs`
- Modify: `crates/storage/src/lib.rs` (`pub mod pin;`)

**Interfaces:**
- Consumes: `Error::WrongPin`, `Error::PinLocked` (Task 2), `ProfilesConfig`, `ProfileMode`
- Produces:
  - `hash_pin(pin: &str) -> Result<String>` (refuses anything but 4 digits)
  - `verify_pin(hash: &str, pin: &str) -> bool`
  - `PinGuard` (`Default`, `check(now) -> Result<(), Duration>`, `fail(now)`, `succeed()`)
  - `check_pin(hash: Option<&str>, given: Option<&str>, guard: &mut PinGuard, now: Instant) -> Result<()>`
  - `authorize_config_change(config: &ProfilesConfig, enabled: bool, mode: ProfileMode, pin: Option<&str>) -> Result<()>`

- [ ] **Step 1: Write the file with failing tests** — `crates/storage/src/pin.rs` (tests first, functions as `todo!()` so it compiles):

```rust
//! Profile PINs: a local lock (like a streaming service's profile PIN), not
//! protection against someone with access to the session's files. Hashed
//! with argon2id; failures slow down after five attempts.

use std::time::{Duration, Instant};

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use oneshot_core::profile::{ProfileMode, ProfilesConfig};
use oneshot_core::{Error, Result};

pub fn hash_pin(pin: &str) -> Result<String> {
    todo!()
}

pub fn verify_pin(hash: &str, pin: &str) -> bool {
    todo!()
}

#[derive(Debug, Default, Clone)]
pub struct PinGuard {
    failures: u32,
    locked_until: Option<Instant>,
}

impl PinGuard {
    pub fn check(&self, now: Instant) -> std::result::Result<(), Duration> {
        todo!()
    }
    pub fn fail(&mut self, now: Instant) {
        todo!()
    }
    pub fn succeed(&mut self) {
        todo!()
    }
}

pub fn check_pin(hash: Option<&str>, given: Option<&str>, guard: &mut PinGuard, now: Instant) -> Result<()> {
    todo!()
}

pub fn authorize_config_change(config: &ProfilesConfig, enabled: bool, mode: ProfileMode, pin: Option<&str>) -> Result<()> {
    todo!()
}

#[cfg(test)]
mod tests {
    use oneshot_core::profile::{Origin, Profile};

    use super::*;

    #[test]
    fn pins_hash_and_verify() {
        let h = hash_pin("0427").unwrap();
        assert!(!h.contains("0427"));
        assert!(verify_pin(&h, "0427"));
        assert!(!verify_pin(&h, "0428"));
        assert!(!verify_pin("garbage", "0427"));
    }

    #[test]
    fn only_four_digits_are_pins() {
        for bad in ["", "123", "12345", "12a4", "１２３４"] {
            assert!(hash_pin(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn guard_locks_after_five_failures_then_longer() {
        let t0 = Instant::now();
        let mut g = PinGuard::default();
        for _ in 0..4 {
            g.fail(t0);
        }
        assert!(g.check(t0).is_ok());
        g.fail(t0);
        assert_eq!(g.check(t0), Err(Duration::from_secs(30)));
        assert!(g.check(t0 + Duration::from_secs(31)).is_ok());
        g.fail(t0 + Duration::from_secs(31));
        assert_eq!(g.check(t0 + Duration::from_secs(31)), Err(Duration::from_secs(60)));
        g.fail(t0);
        assert_eq!(g.check(t0), Err(Duration::from_secs(300)));
        g.succeed();
        assert!(g.check(t0).is_ok());
    }

    #[test]
    fn check_pin_reports_wrong_then_locked() {
        let h = hash_pin("1111").unwrap();
        let now = Instant::now();
        let mut g = PinGuard::default();
        assert!(check_pin(None, None, &mut g, now).is_ok(), "no PIN, nothing to check");
        assert!(matches!(check_pin(Some(&h), None, &mut g, now), Err(Error::WrongPin)));
        for _ in 0..3 {
            let _ = check_pin(Some(&h), Some("0000"), &mut g, now);
        }
        assert!(matches!(check_pin(Some(&h), Some("0000"), &mut g, now), Err(Error::PinLocked(30))));
        assert!(matches!(check_pin(Some(&h), Some("1111"), &mut g, now), Err(Error::PinLocked(_))), "even the right PIN waits");
        assert!(check_pin(Some(&h), Some("1111"), &mut g, now + Duration::from_secs(30)).is_ok());
    }

    fn config_with_pin() -> ProfilesConfig {
        let kid = Profile::new("Kid", "#ff6b6b", Origin::Manual, Default::default());
        let mut parent = Profile::new("Parent", "#5e8bff", Origin::Manual, Default::default());
        parent.pin = Some(hash_pin("9876").unwrap());
        ProfilesConfig { enabled: true, profiles: vec![kid, parent], ..Default::default() }
    }

    #[test]
    fn turning_multi_user_off_or_changing_mode_needs_a_pin() {
        let c = config_with_pin();
        assert!(matches!(authorize_config_change(&c, false, c.mode, None), Err(Error::WrongPin)));
        assert!(matches!(authorize_config_change(&c, true, ProfileMode::Local, Some("0000")), Err(Error::WrongPin)));
        assert!(authorize_config_change(&c, false, c.mode, Some("9876")).is_ok());
        assert!(authorize_config_change(&c, true, c.mode, None).is_ok(), "ask-on-startup alone is not guarded");
    }

    #[test]
    fn no_pin_anywhere_means_no_lock() {
        let mut c = config_with_pin();
        c.profiles[1].pin = None;
        assert!(authorize_config_change(&c, false, ProfileMode::Linked, None).is_ok());
        let off = ProfilesConfig::default();
        assert!(authorize_config_change(&off, true, ProfileMode::Local, None).is_ok(), "turning it on is free");
    }
}
```

Add `pub mod pin;` to `crates/storage/src/lib.rs`.

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p oneshot-storage pin::`
Expected: FAIL — panics `not yet implemented`.

- [ ] **Step 3: Implement** — replace the `todo!()` bodies:

```rust
pub fn hash_pin(pin: &str) -> Result<String> {
    if pin.len() != 4 || !pin.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::Invalid("a PIN is 4 digits".into()));
    }
    // uuid v4 is 16 random bytes: a fine salt, and no extra RNG dependency.
    let salt = SaltString::encode_b64(uuid::Uuid::new_v4().as_bytes()).map_err(|e| Error::Other(e.to_string()))?;
    Argon2::default().hash_password(pin.as_bytes(), &salt).map(|h| h.to_string()).map_err(|e| Error::Other(e.to_string()))
}

pub fn verify_pin(hash: &str, pin: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| Argon2::default().verify_password(pin.as_bytes(), &h).is_ok())
}

impl PinGuard {
    /// `Err(wait)` while locked.
    pub fn check(&self, now: Instant) -> std::result::Result<(), Duration> {
        match self.locked_until {
            Some(t) if t > now => Err(t - now),
            _ => Ok(()),
        }
    }

    pub fn fail(&mut self, now: Instant) {
        self.failures += 1;
        let secs = match self.failures {
            0..=4 => return,
            5 => 30,
            6 => 60,
            _ => 300,
        };
        self.locked_until = Some(now + Duration::from_secs(secs));
    }

    pub fn succeed(&mut self) {
        *self = Self::default();
    }
}

/// `hash` is the profile's PIN (none = open). Counts failures in `guard`.
pub fn check_pin(hash: Option<&str>, given: Option<&str>, guard: &mut PinGuard, now: Instant) -> Result<()> {
    let Some(hash) = hash else { return Ok(()) };
    if let Err(wait) = guard.check(now) {
        return Err(Error::PinLocked(wait.as_secs().max(1)));
    }
    if given.is_some_and(|p| verify_pin(hash, p)) {
        guard.succeed();
        return Ok(());
    }
    guard.fail(now);
    Err(match guard.check(now) {
        Err(wait) => Error::PinLocked(wait.as_secs().max(1)),
        Ok(()) => Error::WrongPin,
    })
}

/// Turning multi-user off, or changing where profiles come from, would let
/// anyone past the PINs (the implicit profile sees everything): it needs the
/// PIN of one protected profile, when there is one.
pub fn authorize_config_change(config: &ProfilesConfig, enabled: bool, mode: ProfileMode, pin: Option<&str>) -> Result<()> {
    let sensitive = config.enabled && (!enabled || mode != config.mode);
    let mut hashes = config.profiles.iter().filter_map(|p| p.pin.as_deref()).peekable();
    if !sensitive || hashes.peek().is_none() {
        return Ok(());
    }
    match pin {
        Some(pin) if hashes.any(|h| verify_pin(h, pin)) => Ok(()),
        _ => Err(Error::WrongPin),
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p oneshot-storage pin::`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/storage
git commit -m "Storage: profile PINs, lockout and config locks

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Utilisateurs publics Jellyfin (provider)

**Files:**
- Modify: `crates/providers/jellyfin/src/dto.rs`, `crates/providers/jellyfin/src/auth.rs`, `crates/providers/jellyfin/src/lib.rs`
- Create: `crates/providers/jellyfin/tests/public_users.rs`

**Interfaces:**
- Produces: `pub struct PublicUser { id: String, name: String, avatar: Option<Url>, has_password: bool }`, `Connector::public_users(&self, base: &Url) -> Result<Vec<PublicUser>>` (re-exported from the crate root)

- [ ] **Step 1: Write the failing test** — `crates/providers/jellyfin/tests/public_users.rs`

```rust
use oneshot_jellyfin::{ClientIdentity, Connector};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn connector() -> Connector {
    let identity = ClientIdentity { client: "Flick".into(), device_name: "test".into(), device_id: "dev".into(), version: "0".into() };
    Connector::new(oneshot_net::reqwest::Client::new(), identity)
}

#[tokio::test]
async fn lists_the_sign_in_screen_users() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Users/Public"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            { "Id": "a1", "Name": "Antoine", "PrimaryImageTag": "t1", "HasPassword": true },
            { "Id": "k2", "Name": "Kid", "HasPassword": false }
        ])))
        .mount(&server)
        .await;
    let base = Url::parse(&server.uri()).unwrap();
    let users = connector().public_users(&base).await.unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].name, "Antoine");
    assert!(users[0].has_password);
    let avatar = users[0].avatar.as_ref().unwrap();
    assert_eq!(avatar.path(), "/Users/a1/Images/Primary");
    assert!(avatar.query().unwrap().contains("tag=t1"));
    assert!(!users[1].has_password);
    assert!(users[1].avatar.is_none());
}
```

If `oneshot-net` is not already a normal dependency of `oneshot-jellyfin`, it is (auth.rs uses it); `serde_json` and `url` are normal dependencies too.

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p oneshot-jellyfin --test public_users`
Expected: FAIL — `no method named public_users`.

- [ ] **Step 3: Add the DTO** — `crates/providers/jellyfin/src/dto.rs`, after `UserPolicy`:

```rust
/// `GET /Users/Public`: users shown on the server's sign-in screen.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PublicUserDto {
    pub id: String,
    pub name: String,
    pub primary_image_tag: Option<String>,
    #[serde(default)]
    pub has_password: bool,
}
```

- [ ] **Step 4: Implement** — `crates/providers/jellyfin/src/auth.rs`: extend the dto import with `PublicUserDto`, add the type after `Session`, and the method in `impl Connector` after `login`:

```rust
/// A user listed on the server's sign-in screen.
#[derive(Debug, Clone)]
pub struct PublicUser {
    pub id: String,
    pub name: String,
    /// Public picture URL (no token needed).
    pub avatar: Option<Url>,
    pub has_password: bool,
}
```

```rust
    /// Users the server shows on its sign-in screen (no authentication).
    /// Users hidden from that screen are not listed.
    pub async fn public_users(&self, base: &Url) -> Result<Vec<PublicUser>> {
        let resp = self
            .http
            .get(oneshot_net::join(base, "Users/Public")?)
            .header("Authorization", self.identity.header(None))
            .send()
            .await
            .map_err(oneshot_net::map_err)?;
        let users: Vec<PublicUserDto> = oneshot_net::json(resp).await?;
        Ok(users
            .into_iter()
            .map(|u| {
                let avatar = u.primary_image_tag.as_deref().and_then(|tag| {
                    let mut url = oneshot_net::join(base, &format!("Users/{}/Images/Primary", u.id)).ok()?;
                    url.query_pairs_mut().append_pair("tag", tag).append_pair("maxHeight", "256");
                    Some(url)
                });
                PublicUser { avatar, id: u.id, name: u.name, has_password: u.has_password }
            })
            .collect())
    }
```

In `crates/providers/jellyfin/src/lib.rs`: `pub use auth::{ClientIdentity, Connector, PublicUser, Session};`

- [ ] **Step 5: Run the test**

Run: `cargo test -p oneshot-jellyfin --test public_users`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/providers/jellyfin
git commit -m "Jellyfin: list public users

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Membres Plex Home + changement d'utilisateur (provider)

**Files:**
- Modify: `crates/providers/plex/src/dto.rs`, `crates/providers/plex/src/auth.rs`, `crates/providers/plex/src/lib.rs`, `crates/providers/plex/tests/live.rs`
- Create: `crates/providers/plex/tests/home.rs`

**Interfaces:**
- Produces: `pub struct HomeMember { id: String, uuid: String, name: String, avatar: Option<Url>, protected: bool, admin: bool }`, `PlexAuth::home_users(&self, account_token: &str) -> Result<Vec<HomeMember>>`, `PlexAuth::switch_user(&self, account_token: &str, uuid: &str, pin: Option<&str>) -> Result<String>` (the member's token). `HomeMember` re-exported from the crate root.

- [ ] **Step 1: Write the failing tests** — `crates/providers/plex/tests/home.rs`

```rust
use oneshot_core::Error;
use oneshot_plex::{PlexAuth, PlexIdentity};
use url::Url;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn auth(server: &MockServer) -> PlexAuth {
    let identity = PlexIdentity {
        product: "Flick".into(),
        version: "0".into(),
        client_identifier: "test".into(),
        device_name: "test".into(),
        platform: "Windows".into(),
    };
    PlexAuth::new(oneshot_net::reqwest::Client::new(), identity).with_base(Url::parse(&format!("{}/", server.uri())).unwrap())
}

#[tokio::test]
async fn lists_home_members() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/home/users"))
        .and(header("X-Plex-Token", "acct"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 1, "name": "Home",
            "users": [
                { "id": 11, "uuid": "u-admin", "title": "Antoine", "username": "antoine", "thumb": "https://plex.tv/users/u-admin/avatar", "admin": true, "restricted": false, "protected": false },
                { "id": 12, "uuid": "u-kid", "title": "Léa", "username": null, "thumb": null, "admin": false, "restricted": true, "protected": true }
            ]
        })))
        .mount(&server)
        .await;
    let members = auth(&server).home_users("acct").await.unwrap();
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].id, "11");
    assert_eq!(members[0].avatar.as_ref().unwrap().as_str(), "https://plex.tv/users/u-admin/avatar");
    assert_eq!(members[1].name, "Léa");
    assert!(members[1].protected);
}

#[tokio::test]
async fn switch_sends_the_pin_and_returns_the_member_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/home/users/u-kid/switch"))
        .and(query_param("pin", "1234"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 12, "uuid": "u-kid", "authToken": "kid-token" })))
        .mount(&server)
        .await;
    assert_eq!(auth(&server).switch_user("acct", "u-kid", Some("1234")).await.unwrap(), "kid-token");
}

#[tokio::test]
async fn a_wrong_switch_pin_is_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/api/v2/home/users/u-kid/switch")).respond_with(ResponseTemplate::new(401)).mount(&server).await;
    let err = auth(&server).switch_user("acct", "u-kid", Some("0000")).await.unwrap_err();
    assert!(matches!(err, Error::Unauthorized), "{err:?}");
}
```

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p oneshot-plex --test home`
Expected: FAIL — `no method named home_users`.

- [ ] **Step 3: Add the DTOs** — `crates/providers/plex/src/dto.rs`, after `PlexUser`:

```rust
/// `GET plex.tv/api/v2/home/users`.
#[derive(Debug, Deserialize)]
pub struct HomeUsers {
    #[serde(default)]
    pub users: Vec<HomeUser>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeUser {
    pub id: i64,
    pub uuid: String,
    pub title: Option<String>,
    pub username: Option<String>,
    pub thumb: Option<String>,
    #[serde(default)]
    pub admin: bool,
    #[serde(default)]
    pub protected: bool,
}

/// `POST plex.tv/api/v2/home/users/{uuid}/switch`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchedUser {
    pub auth_token: String,
}
```

- [ ] **Step 4: Implement** — `crates/providers/plex/src/auth.rs`: extend the dto import (`HomeUsers, SwitchedUser`), add the type after `PlexAccount`, and the methods after `account`:

```rust
/// A member of the account's Plex Home.
#[derive(Debug, Clone)]
pub struct HomeMember {
    /// Account id, as `PlexAccount::user_id` reports it once switched.
    pub id: String,
    pub uuid: String,
    pub name: String,
    pub avatar: Option<Url>,
    /// plex.tv asks this member's PIN on switch.
    pub protected: bool,
    pub admin: bool,
}
```

```rust
    /// Members of the account's Plex Home (just the account itself when it
    /// has no Home).
    pub async fn home_users(&self, account_token: &str) -> Result<Vec<HomeMember>> {
        let rb = self.http.get(self.url("api/v2/home/users")?);
        let home: HomeUsers = oneshot_net::json(self.identity.apply(rb, Some(account_token)).send().await.map_err(oneshot_net::map_err)?).await?;
        Ok(home
            .users
            .into_iter()
            .map(|u| HomeMember {
                id: u.id.to_string(),
                name: u.title.filter(|t| !t.is_empty()).or(u.username).unwrap_or_else(|| u.uuid.clone()),
                uuid: u.uuid,
                avatar: u.thumb.and_then(|t| Url::parse(&t).ok()),
                protected: u.protected,
                admin: u.admin,
            })
            .collect())
    }

    /// Switches to a Home member; plex.tv checks `pin` for protected ones.
    /// Returns the member's own account token.
    pub async fn switch_user(&self, account_token: &str, uuid: &str, pin: Option<&str>) -> Result<String> {
        let mut url = self.url(&format!("api/v2/home/users/{uuid}/switch"))?;
        if let Some(pin) = pin {
            url.query_pairs_mut().append_pair("pin", pin);
        }
        let rb = self.http.post(url);
        let user: SwitchedUser = oneshot_net::json(self.identity.apply(rb, Some(account_token)).send().await.map_err(oneshot_net::map_err)?).await?;
        Ok(user.auth_token)
    }
```

In `crates/providers/plex/src/lib.rs`: `pub use auth::{DiscoveredServer, HomeMember, PinChallenge, PlexAccount, PlexAuth, PlexIdentity};`

- [ ] **Step 5: Run the tests**

Run: `cargo test -p oneshot-plex --test home`
Expected: PASS (3 tests).

- [ ] **Step 6: Add a live check against real plex.tv** — at the end of `crates/providers/plex/tests/live.rs`, following that file's helpers for identity/client:

```rust
/// Confirms the Plex Home endpoints and their JSON shape on a real account:
/// `PLEX_ACCOUNT_TOKEN=… cargo test -p oneshot-plex --test live home_users_live -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn home_users_live() {
    let Ok(token) = std::env::var("PLEX_ACCOUNT_TOKEN") else { return };
    let identity = oneshot_plex::PlexIdentity {
        product: "Flick".into(),
        version: "0".into(),
        client_identifier: "flick-live-test".into(),
        device_name: "live test".into(),
        platform: "Windows".into(),
    };
    let auth = oneshot_plex::PlexAuth::new(oneshot_net::reqwest::Client::new(), identity);
    let members = auth.home_users(&token).await.expect("home users");
    for m in &members {
        println!("{} uuid={} protected={} admin={}", m.name, m.uuid, m.protected, m.admin);
    }
    assert!(!members.is_empty(), "an account is always a member of its own home");
}
```

- [ ] **Step 7: Run the live check** (needs a real token; the account token is in the OS keychain entry `plex-account` once a Plex server was added in Flick)

Run: `PLEX_ACCOUNT_TOKEN=<token> cargo test -p oneshot-plex --test live home_users_live -- --ignored --nocapture`
Expected: PASS and one line per Home member. **If it fails with a decode error**, print the raw body (`curl -H "Accept: application/json" -H "X-Plex-Token: <token>" -H "X-Plex-Client-Identifier: x" https://plex.tv/api/v2/home/users`), then fix the `HomeUsers` / `HomeUser` DTOs and the `lists_home_members` fixture to the real shape before going on. Without a token, report the step as not run.

- [ ] **Step 8: Commit**

```bash
git add crates/providers/plex
git commit -m "Plex: Home members and user switch

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: État applicatif — profil actif, catalogue, réglages effectifs, démarrage

**Files:**
- Modify: `crates/catalog/src/lib.rs`, `app/src/state.rs`, `app/src/main.rs`, `app/src/commands/system.rs`, `app/src/commands/servers.rs`, `app/src/dev.rs`

**Interfaces:**
- Consumes: Tasks 2–6
- Produces (on `AppState`):
  - fields `profiles: RwLock<ProfilesConfig>`, `active_profile: RwLock<Option<ProfileId>>`, `pin_guards: Mutex<HashMap<ProfileId, PinGuard>>`, `offline: RwLock<HashSet<ServerId>>`, `excluded: RwLock<HashSet<ServerId>>`, `switching: tokio::sync::Mutex<()>`
  - `resolved_profiles(&self) -> Vec<Resolved>`
  - `active_connections(&self) -> Option<HashSet<ServerId>>` (`None` = multi-user off, load everything)
  - `active_members(&self) -> Option<HashSet<ServerId>>` (same, disabled connections included) and command `servers_list(all: Option<bool>)` (active profile's accounts unless `all`)
  - `update_profile(&self, id, f: impl FnOnce(&mut Profile)) -> Result<()>`
  - `check_pin(&self, id, given: Option<&str>) -> Result<()>`
  - `activate_profile(&self, id) -> Result<Vec<String>>` (names of connections that could not be loaded)
  - `apply_effective_settings(&self, Settings)`
  - `resume_last_profile(&self) -> bool`
  - `register_server(&self, d, token) -> Result<ServerDescriptor>` (now returns the stored descriptor; reuses the id of the same server+user)
  - `Catalog::replace(&self, Vec<Arc<dyn MediaProvider>>)`

- [ ] **Step 1: `Catalog::replace`** — `crates/catalog/src/lib.rs`, after `remove`:

```rust
    /// Swaps the whole set of live providers (profile switch). The metadata
    /// cache is kept: it is keyed by connection, so the next profile's
    /// screens paint from it at once.
    pub fn replace(&self, providers: Vec<Arc<dyn MediaProvider>>) {
        *self.providers.write() = providers;
    }
```

- [ ] **Step 2: New state fields and imports** — `app/src/state.rs`:

```rust
use std::collections::{HashMap, HashSet};
use std::time::Instant;

use oneshot_core::profile::{Origin, Profile, ProfileId, ProfileMode, ProfilesConfig};
use oneshot_core::settings::PersonalSettings;
use oneshot_storage::pin::{self, PinGuard};
use oneshot_storage::profiles::{self, Resolved};
```

Add to `pub struct AppState` (after `pip_restore`):

```rust
    /// `profiles.json`: multi-user mode, profiles, last discovery.
    pub profiles: RwLock<ProfilesConfig>,
    /// The profile in use (multi-user on); `None` until someone is picked.
    pub active_profile: RwLock<Option<ProfileId>>,
    /// Failed PIN attempts, per profile, for this run.
    pub pin_guards: Mutex<HashMap<ProfileId, PinGuard>>,
    /// Connections whose server did not answer the last discovery.
    pub offline: RwLock<HashSet<ServerId>>,
    /// Connections left out of the active profile for this session (a
    /// protected Plex Home member whose PIN plex.tv could not check).
    pub excluded: RwLock<HashSet<ServerId>>,
    /// One profile switch at a time.
    pub switching: tokio::sync::Mutex<()>,
```

- [ ] **Step 3: Profile methods** — in `impl AppState` (`app/src/state.rs`):

```rust
    /// The profiles of the current mode. Derived profiles appearing for the
    /// first time are saved.
    pub fn resolved_profiles(&self) -> Vec<Resolved> {
        let servers = self.servers.read().clone();
        let defaults = || PersonalSettings::from_settings(&self.store.settings());
        let mut cfg = self.profiles.write();
        let before = cfg.profiles.len();
        let out = profiles::resolve(&mut cfg, &servers, &defaults);
        if cfg.profiles.len() != before
            && let Err(e) = self.store.save_profiles(&cfg)
        {
            tracing::warn!(target: "storage", "profiles not saved: {e}");
        }
        out
    }

    /// Connections the catalogue should hold: `None` = all (multi-user off),
    /// empty = nobody picked yet.
    pub fn active_connections(&self) -> Option<HashSet<ServerId>> {
        if !self.profiles.read().enabled {
            return None;
        }
        let active = *self.active_profile.read();
        let Some(id) = active else { return Some(HashSet::new()) };
        let excluded = self.excluded.read().clone();
        Some(
            self.resolved_profiles()
                .iter()
                .find(|r| r.profile.id == id)
                .map(|r| profiles::connections_of(r).into_iter().filter(|c| !excluded.contains(c)).collect())
                .unwrap_or_default(),
        )
    }

    fn wanted(&self, id: ServerId) -> bool {
        self.active_connections().is_none_or(|w| w.contains(&id))
    }

    pub fn update_profile(&self, id: ProfileId, f: impl FnOnce(&mut Profile)) -> Result<()> {
        let mut cfg = self.profiles.write();
        let p = cfg.profiles.iter_mut().find(|p| p.id == id).ok_or_else(|| Error::NotFound(format!("profile {id}")))?;
        f(p);
        self.store.save_profiles(&cfg)
    }

    /// Checks `given` against the profile's PIN (open profiles pass).
    pub fn check_pin(&self, id: ProfileId, given: Option<&str>) -> Result<()> {
        let hash = self
            .profiles
            .read()
            .profiles
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error::NotFound(format!("profile {id}")))?
            .pin
            .clone();
        let mut guards = self.pin_guards.lock();
        pin::check_pin(hash.as_deref(), given, guards.entry(id).or_default(), Instant::now())
    }

    pub fn apply_effective_settings(&self, effective: Settings) {
        self.player.apply_settings(&effective);
        *self.settings.write() = effective;
    }

    /// Makes `id` the active profile: its preferences, its connections.
    /// Returns the names of connections that could not be loaded.
    pub fn activate_profile(&self, id: ProfileId) -> Result<Vec<String>> {
        let resolved = self.resolved_profiles();
        let r = resolved.iter().find(|r| r.profile.id == id).ok_or_else(|| Error::NotFound(format!("profile {id}")))?;
        *self.active_profile.write() = Some(id);
        {
            let mut cfg = self.profiles.write();
            cfg.last_profile = Some(id);
            self.store.save_profiles(&cfg)?;
        }
        let mut effective = self.store.settings();
        r.profile.prefs.apply(&mut effective);
        self.apply_effective_settings(effective);
        self.restore_servers();
        let live: HashSet<ServerId> = self.catalog.providers().iter().map(|p| p.descriptor().id).collect();
        tracing::info!(target: "provider", profile = %r.profile.name, "profile active");
        Ok(r.accounts
            .iter()
            .filter_map(|a| a.connection.as_ref())
            .filter(|d| !d.disabled && !live.contains(&d.id))
            .map(|d| d.name.clone())
            .collect())
    }

    /// Multi-user on, picker not wanted at startup: resume the last profile
    /// when nothing has to be typed. Returns whether a profile was activated.
    pub fn resume_last_profile(&self) -> bool {
        let cfg = self.profiles.read().clone();
        if !cfg.enabled || cfg.ask_on_startup {
            return false;
        }
        let Some(id) = cfg.last_profile else { return false };
        let Some(r) = self.resolved_profiles().into_iter().find(|r| r.profile.id == id) else { return false };
        let needs_typing = r.profile.pin.is_some() || r.accounts.iter().any(|a| a.discovered.as_ref().is_some_and(|u| u.protected));
        if needs_typing {
            return false;
        }
        match self.activate_profile(id) {
            Ok(_) => true,
            Err(e) => {
                tracing::warn!(target: "provider", "last profile not resumed: {e}");
                false
            }
        }
    }

    /// Modes A/C: a connection added while a profile is active belongs to it.
    fn attach_to_active(&self, id: ServerId) -> Result<()> {
        let (enabled, mode) = {
            let cfg = self.profiles.read();
            (cfg.enabled, cfg.mode)
        };
        let active = *self.active_profile.read();
        match active {
            Some(pid) if enabled && mode != ProfileMode::ServerUsers => self.update_profile(pid, |p| {
                if p.origin == Origin::Manual && !p.connections.contains(&id) {
                    p.connections.push(id);
                }
            }),
            _ => Ok(()),
        }
    }
```

- [ ] **Step 4: Profile-aware server registry** — replace `register_server`, `remove_server`, `set_server_enabled` and `restore_servers` in `app/src/state.rs`:

```rust
    /// Persists a new/updated connection and its token, and connects it when
    /// the active profile uses it. Signing in again as the same user on the
    /// same server keeps the connection's id (profiles and caches refer to it).
    pub fn register_server(&self, mut d: ServerDescriptor, token: &str) -> Result<ServerDescriptor> {
        {
            let mut servers = self.servers.write();
            if let Some(existing) = servers.iter().find(|s| s.kind == d.kind && s.remote_id == d.remote_id && s.user.id == d.user.id) {
                d.id = existing.id;
                d.disabled = existing.disabled;
            }
            secrets::store_token(d.id, token)?;
            servers.retain(|s| s.id != d.id);
            servers.push(d.clone());
            self.store.save_servers(&servers)?;
        }
        self.attach_to_active(d.id)?;
        if !d.disabled && self.wanted(d.id) {
            self.catalog.add(self.build_provider(&d, token.to_owned()));
        }
        tracing::info!(target: "provider", server = %d.name, kind = ?d.kind, "server connected");
        Ok(d)
    }

    pub fn remove_server(&self, id: ServerId) -> Result<()> {
        self.catalog.remove(id);
        secrets::delete_token(id)?;
        {
            let mut servers = self.servers.write();
            servers.retain(|s| s.id != id);
            self.store.save_servers(&servers)?;
        }
        let mut cfg = self.profiles.write();
        for p in &mut cfg.profiles {
            p.connections.retain(|c| *c != id);
            p.detached.retain(|c| *c != id);
        }
        self.store.save_profiles(&cfg)
    }
```

In `set_server_enabled`, change `if enabled {` to `if enabled && self.wanted(id) {` (the rest is unchanged).

```rust
    /// Connects the stored connections the catalogue should hold (all of
    /// them with multi-user off, the active profile's otherwise). Missing
    /// tokens are not an error: the server is listed as needing sign-in.
    pub fn restore_servers(&self) {
        let wanted = self.active_connections();
        let servers = self.servers.read().clone();
        let mut providers = Vec::new();
        for d in servers {
            if d.disabled {
                tracing::info!(target: "provider", server = %d.name, "server disabled; not connecting");
                continue;
            }
            if wanted.as_ref().is_some_and(|w| !w.contains(&d.id)) {
                continue;
            }
            match secrets::load_token(d.id) {
                Ok(Some(token)) => providers.push(self.build_provider(&d, token)),
                Ok(None) => tracing::warn!(target: "provider", server = %d.name, "no stored token; sign-in required"),
                Err(e) => tracing::error!(target: "provider", server = %d.name, "credential store error: {e}"),
            }
        }
        self.catalog.replace(providers);
    }
```

- [ ] **Step 5: Callers of `register_server`** — `app/src/commands/servers.rs`:
  - make `jellyfin_descriptor` `pub(crate)`;
  - in `jellyfin_login`: `let d = state.register_server(jellyfin_descriptor(&session), &session.token)?; Ok(d)`;
  - in `jellyfin_quick_connect_poll`: `Ok(Some(state.register_server(jellyfin_descriptor(&session), &session.token)?))`;
  - replace the body of `plex_add_servers` and add the shared helpers:

```rust
/// Keychain slot for the plex.tv account token (lets the user add more
/// servers, and switch Plex Home members, without a new PIN).
pub(crate) const PLEX_ACCOUNT_KEY: &str = "plex-account";

pub(crate) fn plex_account_token(state: &AppState) -> Option<String> {
    state.plex_account.lock().clone().or_else(|| secrets::load_secret(PLEX_ACCOUNT_KEY).ok().flatten())
}

/// Registers `user`'s connections to the Plex servers `keep` selects, with
/// the access tokens plex.tv gives that user.
pub(crate) async fn register_plex(
    state: &AppState,
    token: &str,
    user: &UserProfile,
    keep: &(dyn Fn(&oneshot_plex::DiscoveredServer) -> bool + Send + Sync),
) -> Result<Vec<ServerDescriptor>> {
    let auth = PlexAuth::new(state.http(), state.plex_identity());
    let mut added = Vec::new();
    for server in auth.discover(token).await?.into_iter().filter(|s| keep(s)) {
        let base_url = require_reachable(&server)?;
        let d = ServerDescriptor {
            id: ServerId::new(),
            kind: ProviderKind::Plex,
            name: server.name.clone(),
            remote_id: server.machine_id.clone(),
            base_url,
            alternate_urls: server.reachable.iter().skip(1).cloned().collect(),
            version: server.version.clone(),
            user: UserProfile { is_admin: server.owned, ..user.clone() },
            disabled: false,
        };
        added.push(state.register_server(d, &server.access_token)?);
    }
    Ok(added)
}

#[tauri::command]
pub async fn plex_add_servers(state: St<'_>, machine_ids: Vec<String>) -> Result<Vec<ServerDescriptor>> {
    let token = plex_account_token(&state).ok_or(Error::Unauthorized)?;
    let account = PlexAuth::new(state.http(), state.plex_identity()).account(&token).await?;
    let user = UserProfile { id: account.user_id, name: account.username, avatar: account.avatar, is_admin: false };
    register_plex(&state, &token, &user, &|s| machine_ids.contains(&s.machine_id)).await
}
```

Delete the old `const PLEX_ACCOUNT_KEY` further down (now defined above). `app/src/dev.rs` keeps compiling (`if let Err(e) = state.register_server(…)` still matches).

Still in `servers.rs`, the Servers screen shows the active profile's accounts (spec §1.5), disabled ones included so they can be turned back on; profile sheets ask for all of them:

```rust
#[tauri::command]
pub fn servers_list(state: St<'_>, all: Option<bool>) -> Vec<ServerEntry> {
    let connected: Vec<ServerId> = state.catalog.providers().iter().map(|p| p.descriptor().id).collect();
    let members = if all.unwrap_or(false) { None } else { state.active_members() };
    state
        .servers
        .read()
        .iter()
        .filter(|s| members.as_ref().is_none_or(|m| m.contains(&s.id)))
        .map(|s| ServerEntry { server: s.clone(), connected: connected.contains(&s.id) })
        .collect()
}
```

and in `impl AppState` (`state.rs`, next to `active_connections`):

```rust
    /// Connections the active profile owns, disabled ones included: `None` =
    /// multi-user off (all of them), empty = nobody picked yet.
    pub fn active_members(&self) -> Option<HashSet<ServerId>> {
        if !self.profiles.read().enabled {
            return None;
        }
        let active = *self.active_profile.read();
        let Some(id) = active else { return Some(HashSet::new()) };
        Some(
            self.resolved_profiles()
                .iter()
                .find(|r| r.profile.id == id)
                .map(|r| r.accounts.iter().filter_map(|a| a.connection.as_ref().map(|d| d.id)).collect())
                .unwrap_or_default(),
        )
    }
```

- [ ] **Step 6: Effective settings** — `app/src/commands/system.rs`, `settings_set`: replace its first line `state.store.save_settings(&settings)?;` with:

```rust
    // A profile is active: its part of the settings goes to the profile,
    // the rest to the shared settings file.
    let active = state.profiles.read().enabled.then(|| *state.active_profile.read()).flatten();
    match active {
        Some(id) => {
            let (shared, prefs) = oneshot_core::settings::split_settings(&settings, &state.store.settings());
            state.store.save_settings(&shared)?;
            state.update_profile(id, |p| p.prefs = prefs)?;
        }
        None => state.store.save_settings(&settings)?,
    }
```

- [ ] **Step 7: Startup** — `app/src/main.rs`, in the `AppState { … }` literal add:

```rust
        profiles: RwLock::new(store.profiles()),
        active_profile: RwLock::new(None),
        pin_guards: Mutex::new(Default::default()),
        offline: RwLock::new(Default::default()),
        excluded: RwLock::new(Default::default()),
        switching: tokio::sync::Mutex::new(()),
```

(`store.profiles()` must be read before `store` is moved into the struct: add `let profiles = store.profiles();` next to `let settings = store.settings();` and use `profiles: RwLock::new(profiles),`.)

Replace `state.restore_servers();` with:

```rust
    // Multi-user: resume the last profile, or wait for the picker (nothing
    // is loaded until someone is chosen). Off: every connection, as before.
    if !state.resume_last_profile() {
        state.restore_servers();
    }
```

- [ ] **Step 8: Build and run every test**

Run: `cargo build -p oneshot-app && cargo test --workspace && cargo clippy --workspace`
Expected: build OK, all tests PASS, no new clippy warning.

- [ ] **Step 9: Regression check with multi-user off**

Run the app as usual. With no `profiles.json` in the config dir, Home, libraries and servers behave exactly as before (all servers loaded; adding/removing/toggling a server works).

- [ ] **Step 10: Commit**

```bash
git add crates/catalog app
git commit -m "App: active profile drives the catalogue and settings

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: Commandes IPC des profils, découverte, avatars

**Files:**
- Create: `app/src/commands/profiles.rs`
- Modify: `app/src/commands/mod.rs`, `app/src/main.rs` (handler list), `app/src/images.rs`, `app/Cargo.toml` (`futures.workspace = true`)

**Interfaces:**
- Consumes: Task 9 `AppState` methods; `Connector::public_users`, `PlexAuth::{home_users, switch_user}`; `register_plex`, `plex_account_token`, `jellyfin_descriptor`
- Produces (Tauri commands; args arrive camelCase from JS):
  - `profiles_state() -> ProfilesState`
  - `profiles_discover() -> Result<ProfilesState>`
  - `profiles_configure(enabled, mode, ask_on_startup, pin: Option<String>) -> Result<ProfilesState>`
  - `profile_check_pin(id, pin: String) -> Result<()>`
  - `profile_switch(id, pin: Option<String>, plex_pin: Option<String>) -> Result<SwitchOutcome { failed: Vec<String> }>` (emits `profile-changed`)
  - `profile_create(name, color) -> Result<ProfileId>`
  - `profile_update(id, edit: ProfileEdit, pin) -> Result<()>` with `ProfileEdit { name, color, avatar: Option<AvatarStyle>, connections: Option<Vec<ServerId>>, hidden: Option<bool> }` (all optional)
  - `profile_set_pin(id, current: Option<String>, next: Option<String>) -> Result<()>`
  - `profile_detach(id, connection: ServerId, pin) -> Result<()>`
  - `profile_delete(id, pin) -> Result<Vec<ServerId>>` (connections no manual profile uses anymore)
  - image route `oneshot-img://…/avatar/<profile-id>/<key>`

- [ ] **Step 1: Write the commands** — `app/src/commands/profiles.rs`

```rust
//! Multi-user profiles: picker/settings state, switching, discovery of
//! server users (mode B) and profile edits. Rules live in
//! `oneshot_storage::{profiles, pin}`; this only wires them.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use oneshot_core::profile::{AvatarStyle, DiscoveredUser, Origin, Profile, ProfileId, ProfileMode, ProfilesState};
use oneshot_core::server::{ProviderKind, UserProfile};
use oneshot_core::settings::PersonalSettings;
use oneshot_core::{Error, Result, ServerId};
use oneshot_jellyfin::Connector;
use oneshot_plex::PlexAuth;
use oneshot_storage::{pin, profiles};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use super::servers::{jellyfin_descriptor, plex_account_token, register_plex};
use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

/// Per server: a slow one never holds the picker.
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) fn snapshot(state: &AppState) -> ProfilesState {
    let cfg = state.profiles.read().clone();
    let offline = state.offline.read().clone();
    let resolved = if cfg.enabled { state.resolved_profiles() } else { Vec::new() };
    ProfilesState {
        enabled: cfg.enabled,
        mode: cfg.mode,
        ask_on_startup: cfg.ask_on_startup,
        active: *state.active_profile.read(),
        profiles: resolved.iter().map(|r| profiles::card(r, &offline)).collect(),
        any_locked: cfg.profiles.iter().any(|p| p.pin.is_some()),
    }
}

#[tauri::command]
pub fn profiles_state(state: St<'_>) -> ProfilesState {
    snapshot(&state)
}

/// Lists server users (mode B): Jellyfin sign-in screens and the Plex Home.
/// A server that does not answer keeps its last list and shows offline.
#[tauri::command]
pub async fn profiles_discover(state: St<'_>) -> Result<ProfilesState> {
    let servers = state.servers.read().clone();
    let previous = state.profiles.read().discovered.clone();
    let mut found: Vec<DiscoveredUser> = Vec::new();
    let mut offline: HashSet<ServerId> = HashSet::new();
    let mut kept: HashSet<ServerId> = HashSet::new();

    let mut seen = HashSet::new();
    let jellyfins: Vec<_> = servers.iter().filter(|s| s.kind == ProviderKind::Jellyfin && seen.insert(s.remote_id.clone())).cloned().collect();
    let connector = Connector::new(state.http(), state.jellyfin_identity());
    let probes = jellyfins.iter().map(|home| {
        let connector = connector.clone();
        async move { (home, tokio::time::timeout(DISCOVERY_TIMEOUT, connector.public_users(&home.base_url)).await) }
    });
    for (home, res) in futures::future::join_all(probes).await {
        match res {
            Ok(Ok(users)) => found.extend(users.into_iter().map(|u| DiscoveredUser {
                server: home.id,
                kind: ProviderKind::Jellyfin,
                remote_user_id: u.id,
                switch_id: None,
                name: u.name,
                avatar: u.avatar,
                has_password: u.has_password,
                protected: false,
            })),
            _ => {
                tracing::info!(target: "provider", server = %home.name, "user discovery failed; keeping the last list");
                kept.insert(home.id);
                offline.extend(servers.iter().filter(|s| s.kind == home.kind && s.remote_id == home.remote_id).map(|s| s.id));
            }
        }
    }

    let mut seen = HashSet::new();
    let plexes: Vec<_> = servers.iter().filter(|s| s.kind == ProviderKind::Plex && seen.insert(s.remote_id.clone())).cloned().collect();
    if !plexes.is_empty()
        && let Some(token) = plex_account_token(&state)
    {
        let auth = PlexAuth::new(state.http(), state.plex_identity());
        match tokio::time::timeout(DISCOVERY_TIMEOUT, auth.home_users(&token)).await {
            Ok(Ok(members)) => {
                for home in &plexes {
                    found.extend(members.iter().map(|m| DiscoveredUser {
                        server: home.id,
                        kind: ProviderKind::Plex,
                        remote_user_id: m.id.clone(),
                        switch_id: Some(m.uuid.clone()),
                        name: m.name.clone(),
                        avatar: m.avatar.clone(),
                        has_password: false,
                        protected: m.protected,
                    }));
                }
            }
            _ => {
                tracing::info!(target: "provider", "Plex Home discovery failed; keeping the last list");
                kept.extend(plexes.iter().map(|h| h.id));
            }
        }
    }

    found.extend(previous.into_iter().filter(|u| kept.contains(&u.server)));
    {
        let mut cfg = state.profiles.write();
        cfg.discovered = found;
        state.store.save_profiles(&cfg)?;
    }
    *state.offline.write() = offline;
    Ok(snapshot(&state))
}

fn os_user_name() -> String {
    std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "Me".into())
}

#[tauri::command]
pub fn profiles_configure(state: St<'_>, enabled: bool, mode: ProfileMode, ask_on_startup: bool, pin: Option<String>) -> Result<ProfilesState> {
    let (was_enabled, old_mode) = {
        let cfg = state.profiles.read();
        pin::authorize_config_change(&cfg, enabled, mode, pin.as_deref())?;
        (cfg.enabled, cfg.mode)
    };
    {
        let mut cfg = state.profiles.write();
        cfg.enabled = enabled;
        cfg.mode = mode;
        cfg.ask_on_startup = ask_on_startup;
        // Modes A/C start with one profile holding what is configured today.
        if enabled && mode != ProfileMode::ServerUsers && !cfg.profiles.iter().any(|p| p.origin == Origin::Manual) {
            let mut first = Profile::new(os_user_name(), profiles::PROFILE_COLORS[0], Origin::Manual, PersonalSettings::from_settings(&state.store.settings()));
            first.connections = state.servers.read().iter().map(|s| s.id).collect();
            cfg.profiles.push(first);
        }
        state.store.save_profiles(&cfg)?;
    }
    if !enabled {
        *state.active_profile.write() = None;
        state.excluded.write().clear();
        state.apply_effective_settings(state.store.settings());
        state.restore_servers();
    } else if !was_enabled || mode != old_mode {
        // Keep the person where they are: enter the profile holding what is loaded.
        let loaded: HashSet<ServerId> = state.catalog.providers().iter().map(|p| p.descriptor().id).collect();
        match profiles::best_match(&state.resolved_profiles(), &loaded) {
            Some(id) => {
                state.activate_profile(id)?;
            }
            None => *state.active_profile.write() = None,
        }
    }
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn profile_check_pin(state: St<'_>, id: ProfileId, pin: String) -> Result<()> {
    state.check_pin(id, Some(&pin))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchOutcome {
    /// Servers of the profile that could not be connected.
    pub failed: Vec<String>,
}

#[tauri::command]
pub async fn profile_switch(app: tauri::AppHandle, state: St<'_>, id: ProfileId, pin: Option<String>, plex_pin: Option<String>) -> Result<SwitchOutcome> {
    let _busy = state.switching.try_lock().map_err(|_| Error::Invalid("a profile switch is already in progress".into()))?;
    state.check_pin(id, pin.as_deref())?;
    let resolved = state
        .resolved_profiles()
        .into_iter()
        .find(|r| r.profile.id == id)
        .ok_or_else(|| Error::NotFound(format!("profile {id}")))?;
    let configured_plex: HashSet<String> =
        state.servers.read().iter().filter(|s| s.kind == ProviderKind::Plex).map(|s| s.remote_id.clone()).collect();
    let mut failed = Vec::new();
    let mut excluded = HashSet::new();

    for account in &resolved.accounts {
        let Some(user) = &account.discovered else { continue };
        match (user.kind, &account.connection) {
            // Plex Home: members not signed in yet, and protected ones every
            // time (plex.tv checks their PIN).
            (ProviderKind::Plex, conn) if conn.is_none() || user.protected => {
                let Some(uuid) = &user.switch_id else { continue };
                let Some(token) = plex_account_token(&state) else {
                    failed.push(account.server_name.clone());
                    continue;
                };
                let auth = PlexAuth::new(state.http(), state.plex_identity());
                let member_pin = if user.protected { plex_pin.as_deref() } else { None };
                match auth.switch_user(&token, uuid, member_pin).await {
                    Ok(member_token) => {
                        let who = UserProfile { id: user.remote_user_id.clone(), name: user.name.clone(), avatar: user.avatar.clone(), is_admin: false };
                        if let Err(e) = register_plex(&state, &member_token, &who, &|s| configured_plex.contains(&s.machine_id)).await {
                            tracing::warn!(target: "provider", server = %account.server_name, "Plex member not connected: {e}");
                            failed.push(account.server_name.clone());
                        }
                    }
                    Err(Error::Unauthorized | Error::Forbidden(_)) if user.protected => return Err(Error::WrongPin),
                    Err(e) => {
                        tracing::warn!(target: "provider", server = %account.server_name, "Plex switch failed: {e}");
                        failed.push(account.server_name.clone());
                        if let Some(c) = conn {
                            excluded.insert(c.id);
                        }
                    }
                }
            }
            // Jellyfin users without a password sign in on first use.
            (ProviderKind::Jellyfin, None) if !user.has_password => {
                let connector = Connector::new(state.http(), state.jellyfin_identity());
                match connector.login(&account.base_url, &user.name, "").await {
                    Ok(session) => {
                        state.register_server(jellyfin_descriptor(&session), &session.token)?;
                    }
                    Err(e) => {
                        tracing::warn!(target: "provider", server = %account.server_name, "Jellyfin sign-in failed: {e}");
                        failed.push(account.server_name.clone());
                    }
                }
            }
            _ => {}
        }
    }

    *state.excluded.write() = excluded;
    failed.extend(state.activate_profile(id)?);
    failed.sort();
    failed.dedup();
    let _ = app.emit("profile-changed", id);
    Ok(SwitchOutcome { failed })
}

#[tauri::command]
pub fn profile_create(state: St<'_>, name: String, color: String) -> Result<ProfileId> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(Error::Invalid("a profile needs a name".into()));
    }
    let p = Profile::new(name, color, Origin::Manual, PersonalSettings::from_settings(&state.store.settings()));
    let id = p.id;
    let mut cfg = state.profiles.write();
    cfg.profiles.push(p);
    state.store.save_profiles(&cfg)?;
    Ok(id)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileEdit {
    pub name: Option<String>,
    pub color: Option<String>,
    pub avatar: Option<AvatarStyle>,
    pub connections: Option<Vec<ServerId>>,
    pub hidden: Option<bool>,
}

#[tauri::command]
pub fn profile_update(state: St<'_>, id: ProfileId, edit: ProfileEdit, pin: Option<String>) -> Result<()> {
    state.check_pin(id, pin.as_deref())?;
    let mut reload = false;
    state.update_profile(id, |p| {
        if let Some(n) = edit.name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()) {
            p.name = n;
        }
        if let Some(c) = edit.color {
            p.color = c;
        }
        if let Some(a) = edit.avatar {
            p.avatar = a;
        }
        if let Some(h) = edit.hidden {
            p.hidden = h;
        }
        if let Some(c) = edit.connections
            && p.origin == Origin::Manual
        {
            p.connections = c;
            reload = true;
        }
    })?;
    if reload && *state.active_profile.read() == Some(id) {
        state.restore_servers();
    }
    Ok(())
}

#[tauri::command]
pub fn profile_set_pin(state: St<'_>, id: ProfileId, current: Option<String>, next: Option<String>) -> Result<()> {
    state.check_pin(id, current.as_deref())?;
    let hash = next.as_deref().map(pin::hash_pin).transpose()?;
    state.update_profile(id, |p| p.pin = hash)
}

#[tauri::command]
pub fn profile_detach(state: St<'_>, id: ProfileId, connection: ServerId, pin: Option<String>) -> Result<()> {
    state.check_pin(id, pin.as_deref())?;
    state.update_profile(id, |p| {
        if !p.detached.contains(&connection) {
            p.detached.push(connection);
        }
    })?;
    if *state.active_profile.read() == Some(id) {
        state.restore_servers();
    }
    Ok(())
}

#[tauri::command]
pub fn profile_delete(state: St<'_>, id: ProfileId, pin: Option<String>) -> Result<Vec<ServerId>> {
    state.check_pin(id, pin.as_deref())?;
    let (removed, used) = {
        let mut cfg = state.profiles.write();
        let idx = cfg.profiles.iter().position(|p| p.id == id).ok_or_else(|| Error::NotFound(format!("profile {id}")))?;
        if cfg.profiles[idx].origin != Origin::Manual {
            return Err(Error::Invalid("profiles made from server users are hidden, not deleted".into()));
        }
        let removed = cfg.profiles.remove(idx);
        if cfg.last_profile == Some(id) {
            cfg.last_profile = None;
        }
        state.store.save_profiles(&cfg)?;
        let used: HashSet<ServerId> = cfg.profiles.iter().filter(|p| p.origin == Origin::Manual).flat_map(|p| p.connections.iter().copied()).collect();
        (removed, used)
    };
    if *state.active_profile.read() == Some(id) {
        *state.active_profile.write() = None;
        state.restore_servers();
    }
    Ok(removed.connections.into_iter().filter(|c| !used.contains(c)).collect())
}
```

- [ ] **Step 2: Register** — `app/src/commands/mod.rs`: add `pub mod profiles;`. `app/Cargo.toml`: add `futures.workspace = true`. `app/src/main.rs` `generate_handler!` list, after `plex_add_servers`:

```rust
            commands::profiles::profiles_state,
            commands::profiles::profiles_discover,
            commands::profiles::profiles_configure,
            commands::profiles::profile_check_pin,
            commands::profiles::profile_switch,
            commands::profiles::profile_create,
            commands::profiles::profile_update,
            commands::profiles::profile_set_pin,
            commands::profiles::profile_detach,
            commands::profiles::profile_delete,
```

- [ ] **Step 3: Avatar route** — `app/src/images.rs`. Update the module doc (second URL form), then replace the body of the spawned task in `handle` and add `load_avatar`:

```rust
//! A second form serves profile pictures: `avatar/<profile-id>/<key>` (the
//! key only changes the URL when the picture changes).
```

```rust
    tauri::async_runtime::spawn(async move {
        let state = app.state::<Arc<AppState>>();
        let loaded = match path.trim_start_matches('/').strip_prefix("avatar/") {
            Some(rest) => Some(load_avatar(&state, rest).await),
            None => match parse_path(&path) {
                Some((image, size)) => Some(load(&state, &image, size).await),
                None => None,
            },
        };
        let response = match loaded {
            None => Response::builder().status(StatusCode::BAD_REQUEST).body(Vec::new()),
            Some(Ok(bytes)) => Response::builder()
                .header("Content-Type", mime(&bytes))
                .header("Cache-Control", "max-age=604800, immutable")
                .body(bytes),
            Some(Err(e)) => {
                tracing::debug!(target: "cache", "image {path}: {e}");
                Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new())
            }
        };
        responder.respond(response.unwrap_or_else(|_| Response::new(Vec::new())));
    });
```

```rust
/// A profile's picture: a public URL (plex.tv, a Jellyfin sign-in screen),
/// proxied because the WebView loads no remote origin.
async fn load_avatar(state: &AppState, rest: &str) -> Result<Vec<u8>> {
    let id: ProfileId = rest.split('/').next().unwrap_or_default().parse().map_err(|_| Error::Invalid("avatar path".into()))?;
    let url = state
        .resolved_profiles()
        .iter()
        .find(|r| r.profile.id == id)
        .and_then(oneshot_storage::profiles::avatar_of)
        .ok_or_else(|| Error::NotFound(format!("avatar of profile {id}")))?;
    let key = oneshot_storage::images::avatar_cache_key(&url);
    if let Some(bytes) = state.images.get(&key) {
        return Ok(bytes);
    }
    let resp = oneshot_net::ensure_ok(state.http().get(url).send().await.map_err(oneshot_net::map_err)?).await?;
    let bytes = resp.bytes().await.map_err(oneshot_net::map_err)?.to_vec();
    if let Err(e) = state.images.put(&key, &bytes) {
        tracing::warn!(target: "cache", "avatar cache write failed: {e}");
    }
    Ok(bytes)
}
```

with `use oneshot_core::profile::ProfileId;`.

- [ ] **Step 4: Build**

Run: `cargo build -p oneshot-app && cargo clippy -p oneshot-app`
Expected: OK, no new warning.

- [ ] **Step 5: Commit**

```bash
git add app Cargo.lock
git commit -m "App: profile commands, user discovery and avatar images

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 11: UI — IPC, requête des profils, orchestration du changement

**Files:**
- Create: `ui/src/lib/queryClient.ts`, `ui/src/lib/profiles.ts`, `ui/src/lib/profiles.test.ts`
- Modify: `ui/src/App.tsx`, `ui/src/ipc/api.ts`, `ui/src/ipc/app-types.ts`, `ui/src/ipc/images.ts`, `ui/src/lib/ambient.ts`

**Interfaces:**
- Consumes: Task 10 commands; bindings `ProfileId`, `ProfileMode`, `ProfileCard`, `ProfileAccount`, `ProfilesState`, `AvatarStyle`
- Produces:
  - `queryClient` (shared)
  - `api.profilesState()`, `profilesDiscover()`, `profilesConfigure(enabled, mode, askOnStartup, pin)`, `profileCheckPin(id, pin)`, `profileSwitch(id, pin, plexPin)`, `profileCreate(name, color)`, `profileUpdate(id, edit, pin)`, `profileSetPin(id, current, next)`, `profileDetach(id, connection, pin)`, `profileDelete(id, pin)`
  - types `ProfileEdit`, `SwitchOutcome` (app-types)
  - `avatarUrl(id: ProfileId, key: string): string`
  - `ambientColor(color: string)`, `ambientReset()`
  - `api.serversList(all = false)`
  - `lib/profiles.ts`: `profilesQuery`, `allServersQuery`, `PROFILE_COLORS`, `useProfileSwitch` (`phase: "idle" | "leaving" | "entering"`), `switchProfile(id, { pin?, plexPin? }): Promise<SwitchOutcome>`, `finishSwitch()`, `initials(name)`, `avatarGradient(color)`, `visibleProfiles(cards)`, `needsPlexPin(card)`, `pendingSignIns(card)`, `accountTitle(account)`, `pinError(e): "wrong" | { locked: number } | null`, `type PickStage = "pin" | "plex-pin" | "sign-in" | "loading"`, `nextStage(card, after: PickStage | null): PickStage`

- [ ] **Step 1: Write the failing tests** — `ui/src/lib/profiles.test.ts`

```ts
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProfileAccount } from "@/ipc/bindings/ProfileAccount";
import type { ProfileCard } from "@/ipc/bindings/ProfileCard";

vi.mock("@/ipc/api", () => ({
  api: { profileSwitch: vi.fn(async () => ({ failed: [] })) },
  asError: (e: { kind?: string; message?: string }) => ({ kind: e?.kind ?? "other", message: String(e?.message ?? e) }),
}));
vi.mock("@/lib/settings", () => ({ loadSettings: vi.fn(async () => null) }));
vi.mock("@/lib/ambient", () => ({ ambientReset: vi.fn() }));

import { queryClient } from "./queryClient";
import { accountTitle, initials, needsPlexPin, nextStage, pendingSignIns, pinError, switchProfile, useProfileSwitch, visibleProfiles } from "./profiles";

const account = (over: Partial<ProfileAccount> = {}): ProfileAccount => ({
  kind: "jellyfin",
  serverName: "Home",
  userName: "Antoine",
  state: "connected",
  connection: "c1",
  baseUrl: "http://home.local/",
  needsPassword: false,
  plexPin: false,
  ...over,
});
const card = (over: Partial<ProfileCard> = {}): ProfileCard => ({ id: "p1", name: "Antoine", color: "#5e8bff", avatarKey: null, locked: false, hidden: false, accounts: [account()], ...over });

describe("profile helpers", () => {
  it("makes initials from one or two words", () => {
    expect(initials("antoine")).toBe("A");
    expect(initials("  Élodie  Martin ")).toBe("ÉM");
    expect(initials("")).toBe("?");
  });

  it("hides hidden profiles", () => {
    expect(visibleProfiles([card(), card({ id: "p2", hidden: true })]).map((c) => c.id)).toEqual(["p1"]);
  });

  it("finds what must be typed before entering", () => {
    const c = card({ accounts: [account(), account({ state: "pending", connection: null, needsPassword: true }), account({ kind: "plex", plexPin: true })] });
    expect(pendingSignIns(c)).toHaveLength(1);
    expect(needsPlexPin(c)).toBe(true);
    expect(nextStage(c, null)).toBe("plex-pin");
    expect(nextStage({ ...c, locked: true }, null)).toBe("pin");
    expect(nextStage(c, "plex-pin")).toBe("sign-in");
    expect(nextStage(c, "sign-in")).toBe("loading");
    expect(nextStage(card(), null)).toBe("loading");
  });

  it("describes where an account comes from", () => {
    expect(accountTitle(account())).toBe("Antoine on Home (Jellyfin)");
    expect(accountTitle(account({ state: "pending" }))).toBe("Antoine on Home (Jellyfin) · sign-in needed");
    expect(accountTitle(account({ kind: "plex", state: "offline" }))).toBe("Antoine on Home (Plex) · offline");
  });

  it("reads PIN errors", () => {
    expect(pinError({ kind: "wrongPin", message: "wrong PIN" })).toBe("wrong");
    expect(pinError({ kind: "pinLocked", message: "too many attempts, try again in 42 s" })).toEqual({ locked: 42 });
    expect(pinError({ kind: "network", message: "down" })).toBeNull();
  });
});

describe("switchProfile", () => {
  beforeEach(() => useProfileSwitch.setState({ phase: "idle", target: null }));

  it("drops every cached query of the previous profile", async () => {
    queryClient.setQueryData(["home"], { rows: ["previous user's row"] });
    queryClient.setQueryData(["item", "x"], { title: "theirs" });
    await switchProfile("p2", {});
    expect(queryClient.getQueryData(["home"])).toBeUndefined();
    expect(queryClient.getQueryData(["item", "x"])).toBeUndefined();
    expect(useProfileSwitch.getState().phase).toBe("entering");
  });

  it("goes back to idle when the switch fails", async () => {
    const { api } = await import("@/ipc/api");
    vi.mocked(api.profileSwitch).mockRejectedValueOnce({ kind: "wrongPin", message: "wrong PIN" });
    await expect(switchProfile("p2", { pin: "0000" })).rejects.toBeTruthy();
    expect(useProfileSwitch.getState().phase).toBe("idle");
  });
});
```

- [ ] **Step 2: Run to see it fail**

Run: `pnpm --dir ui run test -- profiles`
Expected: FAIL — cannot find `./queryClient` / `./profiles`.

- [ ] **Step 3: Shared query client** — `ui/src/lib/queryClient.ts`

```ts
// One QueryClient for the app, importable outside React (a profile switch
// clears it).
import { QueryClient } from "@tanstack/react-query";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: { staleTime: 30_000, retry: 1, refetchOnWindowFocus: false },
  },
});
```

In `ui/src/App.tsx`: delete the local `const queryClient = new QueryClient(…)` and `QueryClient` import; `import { queryClient } from "@/lib/queryClient";`.

- [ ] **Step 4: IPC** — `ui/src/ipc/app-types.ts`, append:

```ts
export type SwitchOutcome = { failed: string[] };
export type ProfileEdit = {
  name: string | null;
  color: string | null;
  avatar: AvatarStyle | null;
  connections: ServerId[] | null;
  hidden: boolean | null;
};
```

(with `import type { AvatarStyle } from "./bindings/AvatarStyle";` and `import type { ServerId } from "./bindings/ServerId";` at the top if not present).

`ui/src/ipc/api.ts`: import `ProfileId`, `ProfileMode`, `ProfilesState` from bindings and `ProfileEdit`, `SwitchOutcome` from app-types; add before `// catalogue`:

```ts
  // profiles
  profilesState: () => call<ProfilesState>("profiles_state"),
  profilesDiscover: () => call<ProfilesState>("profiles_discover"),
  profilesConfigure: (enabled: boolean, mode: ProfileMode, askOnStartup: boolean, pin: string | null) =>
    call<ProfilesState>("profiles_configure", { enabled, mode, askOnStartup, pin }),
  profileCheckPin: (id: ProfileId, pin: string) => call<void>("profile_check_pin", { id, pin }),
  profileSwitch: (id: ProfileId, pin: string | null, plexPin: string | null) => call<SwitchOutcome>("profile_switch", { id, pin, plexPin }),
  profileCreate: (name: string, color: string) => call<ProfileId>("profile_create", { name, color }),
  profileUpdate: (id: ProfileId, edit: ProfileEdit, pin: string | null) => call<void>("profile_update", { id, edit, pin }),
  profileSetPin: (id: ProfileId, current: string | null, next: string | null) => call<void>("profile_set_pin", { id, current, next }),
  profileDetach: (id: ProfileId, connection: ServerId, pin: string | null) => call<void>("profile_detach", { id, connection, pin }),
  profileDelete: (id: ProfileId, pin: string | null) => call<ServerId[]>("profile_delete", { id, pin }),
```

Still in `api.ts`, change `serversList` (Task 9 gave the command an optional `all`):

```ts
  serversList: (all = false) => call<ServerEntry[]>("servers_list", { all }),
```

`ui/src/ipc/images.ts`, append:

```ts
/** A profile's picture (proxied by Rust; `key` changes when the picture does). */
export function avatarUrl(profile: string, key: string): string {
  return `${BASE}avatar/${encodeURIComponent(profile)}/${encodeURIComponent(key)}`;
}
```

- [ ] **Step 5: Ambient helpers** — `ui/src/lib/ambient.ts`, append:

```ts
/** No artwork, just soft light in `color` (the profile picker). The base
 * stays dark so white text keeps its contrast. */
export function ambientColor(color: string) {
  wanted = "";
  window.clearTimeout(timer);
  const base = `color-mix(in srgb, ${color} 12%, black)`;
  useAmbient.setState({ image: null, item: null, palette: { colors: [color, color], base, accent: color } });
  const root = document.documentElement.style;
  root.setProperty("--ambient-base", base);
  root.setProperty("--ambient-accent", color);
}

/** Forget the last artwork (a profile switch: nothing of the previous one stays). */
export function ambientReset() {
  wanted = "";
  window.clearTimeout(timer);
  useAmbient.setState({ image: null, palette: null, item: null });
}
```

- [ ] **Step 6: Implement `lib/profiles.ts`**

```ts
// Multi-user profiles on the UI side: the state query, the switch
// sequence (fade out, swap in Rust, drop every cached query, fade in) and
// small pure helpers the screens share.
import { create } from "zustand";
import { api, asError } from "@/ipc/api";
import type { SwitchOutcome } from "@/ipc/app-types";
import type { ProfileAccount } from "@/ipc/bindings/ProfileAccount";
import type { ProfileCard } from "@/ipc/bindings/ProfileCard";
import type { ProfileId } from "@/ipc/bindings/ProfileId";
import { ambientReset } from "@/lib/ambient";
import { loadSettings } from "@/lib/settings";
import { queryClient } from "./queryClient";

export const profilesQuery = { queryKey: ["profiles"], queryFn: () => api.profilesState() };

/** Every connection on this computer (profile sheets, "Other User"); the
 * plain `serversQuery` lists only the active profile's. Invalidating
 * `["servers"]` refreshes both. */
export const allServersQuery = { queryKey: ["servers", "all"], queryFn: () => api.serversList(true) };

/** Mirrors `PROFILE_COLORS` in crates/storage/src/profiles.rs. */
export const PROFILE_COLORS = ["#5e8bff", "#ff6b6b", "#3ecf8e", "#ffb547", "#b07cff", "#ff7ac6", "#35c6d6", "#a3a3a3"];

type SwitchPhase = "idle" | "leaving" | "entering";
export const useProfileSwitch = create<{ phase: SwitchPhase; target: ProfileId | null }>(() => ({ phase: "idle", target: null }));

/** Content fade before the swap (the Shell animates on `phase`). */
const LEAVE_MS = 200;
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

export async function switchProfile(id: ProfileId, opts: { pin?: string; plexPin?: string }): Promise<SwitchOutcome> {
  useProfileSwitch.setState({ phase: "leaving", target: id });
  try {
    const [outcome] = await Promise.all([api.profileSwitch(id, opts.pin ?? null, opts.plexPin ?? null), wait(LEAVE_MS)]);
    // Never show one frame of the previous profile's data.
    queryClient.clear();
    ambientReset();
    await loadSettings();
    useProfileSwitch.setState({ phase: "entering" });
    return outcome;
  } catch (e) {
    useProfileSwitch.setState({ phase: "idle", target: null });
    throw e;
  }
}

/** Called once the new profile's screen has faded in. */
export function finishSwitch() {
  useProfileSwitch.setState({ phase: "idle", target: null });
}

export function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return "?";
  return words
    .slice(0, 2)
    .map((w) => w[0]!.toLocaleUpperCase())
    .join("");
}

export function avatarGradient(color: string): string {
  return `radial-gradient(120% 120% at 30% 20%, color-mix(in srgb, ${color} 80%, white), ${color} 45%, color-mix(in srgb, ${color} 55%, black))`;
}

export const visibleProfiles = (cards: ProfileCard[]) => cards.filter((c) => !c.hidden);
export const needsPlexPin = (card: ProfileCard) => card.accounts.some((a) => a.plexPin);
export const pendingSignIns = (card: ProfileCard) => card.accounts.filter((a) => a.state === "pending" && a.kind === "jellyfin" && a.needsPassword);

export function accountTitle(a: ProfileAccount): string {
  const base = `${a.userName} on ${a.serverName} (${a.kind === "plex" ? "Plex" : "Jellyfin"})`;
  if (a.state === "pending") return `${base} · sign-in needed`;
  if (a.state === "offline") return `${base} · offline`;
  return base;
}

export function pinError(e: unknown): "wrong" | { locked: number } | null {
  const err = asError(e);
  if (err.kind === "wrongPin") return "wrong";
  if (err.kind === "pinLocked") return { locked: Number(/(\d+)/.exec(err.message)?.[1] ?? 30) };
  return null;
}

export type PickStage = "pin" | "plex-pin" | "sign-in" | "loading";
const STAGES: PickStage[] = ["pin", "plex-pin", "sign-in", "loading"];

/** What comes after `after` when entering `card` (Flick PIN, Plex PIN, sign-ins, then load). */
export function nextStage(card: ProfileCard, after: PickStage | null): PickStage {
  const needed: Record<PickStage, boolean> = {
    pin: card.locked,
    "plex-pin": needsPlexPin(card),
    "sign-in": pendingSignIns(card).length > 0,
    loading: true,
  };
  const from = after ? STAGES.indexOf(after) + 1 : 0;
  return STAGES.slice(from).find((s) => needed[s]) ?? "loading";
}
```

- [ ] **Step 7: Run tests and typecheck**

Run: `pnpm --dir ui run test && pnpm --dir ui run typecheck`
Expected: PASS; no type error.

- [ ] **Step 8: Commit**

```bash
git add ui/src
git commit -m "UI: profiles IPC, switch sequence and helpers

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 12: UI — avatar, pastilles de provenance, pavé PIN

**Files:**
- Create: `ui/src/components/tv/ProfileAvatar.tsx`, `ui/src/components/tv/AccountPills.tsx`, `ui/src/components/tv/PinPad.tsx`

**Interfaces:**
- Consumes: `avatarUrl`, `initials`, `avatarGradient`, `accountTitle`, `ProviderLogo`
- Produces:
  - `<ProfileAvatar profile={Pick<ProfileCard,"id"|"name"|"color"|"avatarKey">} className? layoutId? />` (size and initials font size come from `className`, e.g. `"size-36 text-5xl"`)
  - `<AccountPills accounts={ProfileAccount[]} className? />`
  - `<PinPad title hint? onSubmit={(pin) => Promise<"ok" | "wrong" | { locked: number }>} onCancel />`

- [ ] **Step 1: `ProfileAvatar`**

```tsx
// A profile's round picture: the server's avatar when there is one, else
// initials on a gradient of the profile's colour. `layoutId` lets the same
// avatar fly from the picker to the sidebar.
import { motion } from "motion/react";
import { useState } from "react";
import type { ProfileCard } from "@/ipc/bindings/ProfileCard";
import { avatarUrl } from "@/ipc/images";
import { avatarGradient, initials } from "@/lib/profiles";
import { cn } from "@/lib/utils";

type Props = { profile: Pick<ProfileCard, "id" | "name" | "color" | "avatarKey">; className?: string; layoutId?: string };

export function ProfileAvatar({ profile, className, layoutId }: Props) {
  const [broken, setBroken] = useState(false);
  const src = profile.avatarKey && !broken ? avatarUrl(profile.id, profile.avatarKey) : undefined;
  return (
    <motion.div
      layoutId={layoutId}
      className={cn("relative grid shrink-0 place-items-center overflow-hidden rounded-full select-none", className)}
      style={{ background: avatarGradient(profile.color) }}
    >
      {src ? (
        <img src={src} alt="" draggable={false} decoding="async" onError={() => setBroken(true)} className="size-full object-cover" />
      ) : (
        <span aria-hidden className="font-heading leading-none font-bold tracking-tight text-white/95">
          {initials(profile.name)}
        </span>
      )}
    </motion.div>
  );
}
```

- [ ] **Step 2: `AccountPills`**

```tsx
// Where a profile's accounts come from, said quietly: one small pill per
// server. Solid = signed in, dashed = to sign in, faded = offline.
import type { ProfileAccount } from "@/ipc/bindings/ProfileAccount";
import { accountTitle } from "@/lib/profiles";
import { cn } from "@/lib/utils";
import { ProviderLogo } from "./ServerBadge";

export function AccountPills({ accounts, className }: { accounts: ProfileAccount[]; className?: string }) {
  return (
    <div className={cn("flex flex-wrap justify-center gap-1.5", className)}>
      {accounts.map((a) => (
        <span
          key={`${a.kind}:${a.serverName}:${a.userName}`}
          title={accountTitle(a)}
          className={cn(
            "inline-flex max-w-[10rem] items-center gap-1 rounded-full px-2 py-0.5 text-[0.75rem] font-medium text-white/50",
            a.state === "connected" && "bg-white/[0.07]",
            a.state === "pending" && "border border-dashed border-white/25",
            a.state === "offline" && "bg-white/[0.04] opacity-50",
          )}
        >
          <ProviderLogo kind={a.kind} className="size-3" />
          <span className="truncate">{a.serverName}</span>
          {a.state === "offline" && <span className="shrink-0">· offline</span>}
        </span>
      ))}
    </div>
  );
}
```

- [ ] **Step 3: `PinPad`**

```tsx
// Four-digit PIN on glass. Digits are focusable keys (remote, controller);
// the keyboard's digits and Backspace work too. A wrong PIN shakes the dots
// and clears them; a lockout shows its countdown.
import { Delete, type LucideIcon } from "lucide-react";
import { motion, useAnimationControls } from "motion/react";
import { useEffect, useRef, useState } from "react";
import { focusSpring } from "@/lib/motion";
import { cn } from "@/lib/utils";
import { FocusGroup, useTv } from "@/nav/Focusable";
import { onAction } from "@/nav/input";

export type PinResult = "ok" | "wrong" | { locked: number };
type Props = { title: string; hint?: string; onSubmit: (pin: string) => Promise<PinResult>; onCancel: () => void };

const LENGTH = 4;

export function PinPad({ title, hint, onSubmit, onCancel }: Props) {
  const [pin, setPin] = useState("");
  const [busy, setBusy] = useState(false);
  const [locked, setLocked] = useState(0);
  const shake = useAnimationControls();
  const submit = useRef(onSubmit);
  submit.current = onSubmit;
  const cancel = useRef(onCancel);
  cancel.current = onCancel;
  const blocked = busy || locked > 0;

  useEffect(() => {
    if (locked <= 0) return;
    const t = setTimeout(() => setLocked((s) => s - 1), 1000);
    return () => clearTimeout(t);
  }, [locked]);

  const press = (d: string) => !blocked && setPin((p) => (p.length < LENGTH ? p + d : p));
  const erase = () => !blocked && setPin((p) => p.slice(0, -1));

  // Keyboard digits; captured before the app's key routing (Backspace = Back there).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (/^[0-9]$/.test(e.key)) press(e.key);
      else if (e.key === "Backspace") erase();
      else return;
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  useEffect(
    () =>
      onAction((a) => {
        if (a.type !== "back") return false;
        cancel.current();
        return true;
      }),
    [],
  );

  useEffect(() => {
    if (pin.length !== LENGTH) return;
    setBusy(true);
    void submit.current(pin).then(async (r) => {
      if (r === "ok") return; // the parent moves on
      if (r !== "wrong") setLocked(r.locked);
      await shake.start({ x: [0, -14, 12, -8, 6, 0], transition: { duration: 0.42 } });
      setPin("");
      setBusy(false);
    });
  }, [pin, shake]);

  return (
    <FocusGroup focusKey="pin-pad" boundary autoFocus className="flex flex-col items-center gap-7">
      <div className="flex flex-col items-center gap-1.5 text-center">
        <h2 className="text-2xl font-bold tracking-tight">{title}</h2>
        <p className="min-h-[1.4em] text-[0.9375rem] text-white/60">{locked > 0 ? `Too many attempts. Try again in ${locked} s.` : (hint ?? "")}</p>
      </div>
      <motion.div animate={shake} className="flex gap-4" role="status" aria-label={`${pin.length} of ${LENGTH} digits`}>
        {Array.from({ length: LENGTH }, (_, i) => (
          <motion.span
            key={i}
            animate={{ scale: i < pin.length ? 1 : 0.7, backgroundColor: i < pin.length ? "rgb(255 255 255)" : "rgb(255 255 255 / 0.2)" }}
            transition={focusSpring}
            className="size-3.5 rounded-full"
          />
        ))}
      </motion.div>
      <div className="grid grid-cols-3 gap-3">
        {["1", "2", "3", "4", "5", "6", "7", "8", "9"].map((d) => (
          <PinKey key={d} label={d} autoFocus={d === "5"} disabled={blocked} onPress={() => press(d)} />
        ))}
        <span />
        <PinKey label="0" disabled={blocked} onPress={() => press("0")} />
        <PinKey label="Delete" icon={Delete} disabled={blocked} onPress={erase} />
      </div>
    </FocusGroup>
  );
}

function PinKey({ label, icon: Icon, autoFocus, disabled, onPress }: { label: string; icon?: LucideIcon; autoFocus?: boolean; disabled?: boolean; onPress: () => void }) {
  const tv = useTv<HTMLButtonElement>({ autoFocus, scroll: false });
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      aria-label={label}
      disabled={disabled}
      onClick={onPress}
      animate={{ scale: tv.showFocus ? 1.1 : 1 }}
      whileTap={{ scale: 0.92 }}
      transition={focusSpring}
      className={cn(
        "grid size-[4.5rem] cursor-pointer place-items-center rounded-full bg-white/[0.08] text-2xl font-semibold tabular-nums transition-colors hover:bg-white/15 disabled:opacity-40",
        tv.showFocus && "bg-white text-black hover:bg-white",
      )}
    >
      {Icon ? <Icon className="size-6" /> : label}
    </motion.button>
  );
}
```

- [ ] **Step 4: Typecheck**

Run: `pnpm --dir ui run typecheck`
Expected: no error.

- [ ] **Step 5: Commit**

```bash
git add ui/src/components/tv
git commit -m "UI: profile avatar, account pills and PIN pad

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 13: UI — écran de sélection « Who's watching? »

**Files:**
- Create: `ui/src/features/profiles/ProfilePicker.tsx`, `ProfileTile.tsx`, `AccountSignIn.tsx`, `OtherUserDialog.tsx`, `ProfileGate.tsx`
- Modify: `ui/src/App.tsx` (route `/profiles` outside the Shell, `<ProfileGate />`, `<LayoutGroup>`)

**Interfaces:**
- Consumes: Tasks 11–12; `AmbientBackdrop`, `TitleBar`, `FlickMark`, `TvDialog`, `TextField`, `Button`, `Notice`
- Produces: route `/profiles` (optional `?pick=<profile-id>` starts entering that profile); `<ProfileGate />` sends to `/profiles` at startup when multi-user is on and nobody is active.

- [ ] **Step 1: `ProfileTile`**

```tsx
// One person on the picker: big avatar that lifts like a tvOS poster, the
// name, where their accounts come from.
import { Lock } from "lucide-react";
import { motion } from "motion/react";
import { useState } from "react";
import { AccountPills } from "@/components/tv/AccountPills";
import { ProfileAvatar } from "@/components/tv/ProfileAvatar";
import type { ProfileCard } from "@/ipc/bindings/ProfileCard";
import { focusSpring, panelSpring } from "@/lib/motion";
import { cn } from "@/lib/utils";
import { useTv } from "@/nav/Focusable";

type Props = { card: ProfileCard; index: number; dimmed: boolean; onSelect: () => void; onEdit?: () => void; onFocused: () => void };

export function ProfileTile({ card, index, dimmed, onSelect, onEdit, onFocused }: Props) {
  const tv = useTv<HTMLButtonElement>({ focusKey: `profile:${card.id}`, onFocused, scroll: false });
  const [hover, setHover] = useState(false);
  const [sheen, setSheen] = useState({ x: 30, y: 20 });
  const lifted = tv.showFocus || hover;
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      onClick={onSelect}
      onContextMenu={(e) => {
        e.preventDefault();
        onEdit?.();
      }}
      onPointerEnter={() => {
        setHover(true);
        onFocused();
      }}
      onPointerLeave={() => setHover(false)}
      onPointerMove={(e) => {
        const r = e.currentTarget.getBoundingClientRect();
        setSheen({ x: ((e.clientX - r.left) / r.width) * 100, y: ((e.clientY - r.top) / r.height) * 100 });
      }}
      initial={{ opacity: 0, y: 24, scale: 0.92, filter: "blur(8px)" }}
      animate={{ opacity: dimmed ? 0.55 : 1, y: 0, scale: 1, filter: "blur(0px)", transitionEnd: { filter: "none" } }}
      exit={{ opacity: 0, scale: 0.9, filter: "blur(6px)" }}
      transition={{ ...panelSpring, delay: index * 0.06 }}
      aria-label={`${card.name}${card.locked ? ", PIN protected" : ""}`}
      className="flex w-[11rem] cursor-pointer flex-col items-center gap-4 outline-none"
    >
      <motion.div
        animate={{ scale: lifted ? 1.1 : 1, y: lifted ? -6 : 0 }}
        transition={focusSpring}
        className="relative rounded-full"
        style={{ boxShadow: lifted ? "0 30px 60px -18px rgb(0 0 0 / 0.85)" : "0 12px 30px -16px rgb(0 0 0 / 0.6)" }}
      >
        <ProfileAvatar profile={card} layoutId={`profile-avatar-${card.id}`} className="size-36 text-5xl" />
        <span
          aria-hidden
          className={cn("pointer-events-none absolute inset-0 rounded-full transition-opacity duration-300", lifted ? "opacity-100" : "opacity-0")}
          style={{ background: `radial-gradient(60% 60% at ${sheen.x}% ${sheen.y}%, rgb(255 255 255 / 0.28), transparent 70%)` }}
        />
        <span aria-hidden className={cn("pointer-events-none absolute -inset-1 rounded-full ring-white transition-[box-shadow] duration-200", tv.showFocus ? "ring-4" : "ring-0")} />
        {card.locked && (
          <span className="absolute right-1 bottom-1 grid size-8 place-items-center rounded-full bg-black/70 text-white/90">
            <Lock className="size-4" />
          </span>
        )}
      </motion.div>
      <span className={cn("max-w-full truncate text-lg font-semibold transition-colors", lifted ? "text-white" : "text-white/80")}>{card.name}</span>
      <AccountPills accounts={card.accounts} className="-mt-2" />
    </motion.button>
  );
}
```

- [ ] **Step 2: `AccountSignIn`**

```tsx
// Signing a pending Jellyfin account in, once, during profile selection:
// password or Quick Connect, or skip it for now.
import { motion } from "motion/react";
import { useEffect, useState } from "react";
import { Button } from "@/components/tv/Button";
import { Notice } from "@/components/tv/Feedback";
import { ProviderLogo } from "@/components/tv/ServerBadge";
import { TextField } from "@/components/tv/TextField";
import { api, asError } from "@/ipc/api";
import type { ProfileAccount } from "@/ipc/bindings/ProfileAccount";
import { panelSpring } from "@/lib/motion";
import { FocusGroup } from "@/nav/Focusable";

export function AccountSignIn({ account, onDone }: { account: ProfileAccount; onDone: () => void }) {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [quick, setQuick] = useState<{ code: string; secret: string } | null>(null);

  const signIn = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.jellyfinLogin(account.baseUrl, account.userName, password);
      onDone();
    } catch (e) {
      setError(asError(e).message);
      setBusy(false);
    }
  };

  const startQuick = async () => {
    setError(null);
    try {
      setQuick(await api.jellyfinQuickConnectStart(account.baseUrl));
    } catch (e) {
      setError(asError(e).message);
    }
  };

  useEffect(() => {
    if (!quick) return;
    const t = setInterval(() => {
      api.jellyfinQuickConnectPoll(account.baseUrl, quick.secret).then(
        (d) => d && onDone(),
        (e) => setError(asError(e).message),
      );
    }, 2000);
    return () => clearInterval(t);
  }, [quick, account.baseUrl, onDone]);

  return (
    <motion.div
      initial={{ opacity: 0, y: 40 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: 20 }}
      transition={panelSpring}
      className="glass-strong flex w-[min(30rem,90vw)] flex-col gap-5 rounded-[2rem] p-8"
    >
      <header className="flex items-center gap-2 text-white/70">
        <ProviderLogo kind={account.kind} />
        <span className="font-medium">{account.serverName}</span>
      </header>
      <p className="text-[0.9375rem] text-white/80">
        Sign in as <strong className="text-white">{account.userName}</strong> once; Flick remembers it.
      </p>
      {quick ? (
        <div className="flex flex-col items-center gap-2">
          <p className="text-4xl font-bold tracking-[0.3em] tabular-nums">{quick.code}</p>
          <p className="text-center text-sm text-white/60">Enter this code in Jellyfin, under Quick Connect.</p>
        </div>
      ) : (
        <TextField label="Password" type="password" value={password} onChange={setPassword} autoFocus onEnter={() => void signIn()} />
      )}
      {error && <Notice tone="error">{error}</Notice>}
      <FocusGroup className="flex flex-wrap gap-3">
        {!quick && (
          <Button variant="primary" disabled={busy} onClick={() => void signIn()}>
            Sign In
          </Button>
        )}
        {!quick && <Button onClick={() => void startQuick()}>Quick Connect</Button>}
        <Button variant="ghost" onClick={onDone}>
          Skip
        </Button>
      </FocusGroup>
    </motion.div>
  );
}
```

- [ ] **Step 3: `OtherUserDialog`** (mode B, users hidden from a Jellyfin sign-in screen)

```tsx
// "Other User": sign in to a Jellyfin server as someone its sign-in screen
// does not list. The new connection forms (or joins) a profile by name.
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "@/components/tv/Button";
import { Notice } from "@/components/tv/Feedback";
import { Segmented } from "@/components/tv/Segmented";
import { TextField } from "@/components/tv/TextField";
import { TvDialog } from "@/components/tv/TvDialog";
import { api, asError } from "@/ipc/api";
import { allServersQuery } from "@/lib/profiles";

export function OtherUserDialog({ open, onClose, onAdded }: { open: boolean; onClose: () => void; onAdded: () => void }) {
  const servers = useQuery(allServersQuery).data ?? [];
  const jellyfins = servers.map((e) => e.server).filter((s, i, all) => s.kind === "jellyfin" && all.findIndex((o) => o.remoteId === s.remoteId) === i);
  const [server, setServer] = useState<string | null>(null);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const chosen = jellyfins.find((s) => s.id === server) ?? jellyfins[0];

  const submit = async () => {
    if (!chosen) return;
    setError(null);
    try {
      await api.jellyfinLogin(chosen.baseUrl, username, password);
      onAdded();
      onClose();
    } catch (e) {
      setError(asError(e).message);
    }
  };

  return (
    <TvDialog open={open} onClose={onClose} title="Other User" description="Sign in as someone the server does not list. Plex Home members are listed already.">
      {jellyfins.length === 0 ? (
        <Notice>Add a Jellyfin server first.</Notice>
      ) : (
        <>
          {jellyfins.length > 1 && (
            <Segmented label="Server" value={chosen?.id ?? ""} options={jellyfins.map((s) => ({ value: s.id, label: s.name }))} onChange={setServer} />
          )}
          <TextField label="Username" value={username} onChange={setUsername} autoFocus />
          <TextField label="Password" type="password" value={password} onChange={setPassword} onEnter={() => void submit()} />
          {error && <Notice tone="error">{error}</Notice>}
          <Button variant="primary" disabled={!username} onClick={() => void submit()}>
            Sign In
          </Button>
        </>
      )}
    </TvDialog>
  );
}
```

- [ ] **Step 4: `ProfilePicker`**

```tsx
// "Who's watching?": full screen, outside the Shell. The Flick mark breathes
// while cached profiles load, rises, then the people cascade in. Choosing
// one flies its avatar to the centre, asks what must be typed (Flick PIN,
// Plex PIN, one-time sign-ins), rings while the profile loads, and lands on
// Home with the avatar flying into the sidebar.
import { useQuery } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { type ReactNode, useCallback, useEffect, useState } from "react";
import { useNavigate, useSearchParams } from "react-router";
import { toast } from "sonner";
import { FlickMark } from "@/components/tv/FlickMark";
import { type PinResult, PinPad } from "@/components/tv/PinPad";
import { ProfileAvatar } from "@/components/tv/ProfileAvatar";
import { api, asError } from "@/ipc/api";
import type { ProfileAccount } from "@/ipc/bindings/ProfileAccount";
import type { ProfileCard } from "@/ipc/bindings/ProfileCard";
import type { ProfileId } from "@/ipc/bindings/ProfileId";
import { ambientColor, ambientReset } from "@/lib/ambient";
import { focusSpring, panelSpring } from "@/lib/motion";
import { nextStage, type PickStage, pendingSignIns, pinError, profilesQuery, switchProfile, visibleProfiles } from "@/lib/profiles";
import { queryClient } from "@/lib/queryClient";
import { cn } from "@/lib/utils";
import { FocusGroup, useTv } from "@/nav/Focusable";
import { onAction } from "@/nav/input";
import { AmbientBackdrop } from "@/shell/AmbientBackdrop";
import { TitleBar } from "@/shell/TitleBar";
import { AccountSignIn } from "./AccountSignIn";
import { OtherUserDialog } from "./OtherUserDialog";
import { ProfileEditor } from "./ProfileEditor";
import { ProfileTile } from "./ProfileTile";

const INTRO_MS = 700;

type Step = { id: ProfileId; stage: PickStage; pin?: string; plexPin?: string; pending: ProfileAccount[]; signIn: number; plexWrong?: boolean };

export function ProfilePicker() {
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const query = useQuery(profilesQuery);
  const [intro, setIntro] = useState(true);
  const [focused, setFocused] = useState<ProfileId | null>(null);
  const [step, setStep] = useState<Step | null>(null);
  const [progress, setProgress] = useState(0);
  const [otherUser, setOtherUser] = useState(false);
  const [editing, setEditing] = useState<ProfileCard | "new" | null>(null);

  const data = query.data;
  const cards = visibleProfiles(data?.profiles ?? []);
  const selected = step ? cards.find((c) => c.id === step.id) : undefined;
  const focusedCard = cards.find((c) => c.id === focused);
  const ready = !intro && !!data;

  useEffect(() => {
    const t = setTimeout(() => setIntro(false), INTRO_MS);
    return () => clearTimeout(t);
  }, []);

  // Silent refresh of server users; the cached list is already on screen.
  useEffect(() => {
    api.profilesDiscover().then(
      (s) => queryClient.setQueryData(profilesQuery.queryKey, s),
      () => undefined,
    );
  }, []);

  // The light follows the focused (or chosen) profile's colour.
  const light = (selected ?? focusedCard ?? cards[0])?.color;
  useEffect(() => {
    if (light) ambientColor(light);
  }, [light]);
  useEffect(() => () => ambientReset(), []);

  const choose = useCallback((card: ProfileCard) => {
    setProgress(0);
    setStep({ id: card.id, stage: nextStage(card, null), pending: pendingSignIns(card), signIn: 0 });
  }, []);

  // ?pick=<id>: arriving from the sidebar for a profile that needs typing.
  useEffect(() => {
    const pick = params.get("pick");
    const card = pick ? cards.find((c) => c.id === pick) : undefined;
    if (ready && card && !step) choose(card);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready]);

  // Digits 1–9 choose the nth profile.
  useEffect(() => {
    if (step || !ready) return;
    const onKey = (e: KeyboardEvent) => {
      const n = Number(e.key);
      const card = Number.isInteger(n) && n >= 1 ? cards[n - 1] : undefined;
      if (!card) return;
      e.preventDefault();
      choose(card);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [step, ready, cards, choose]);

  // Back cancels a choice; on the tiles there is nowhere to go back to.
  // Menu (or right click) on a focused tile opens its sheet.
  useEffect(
    () =>
      onAction((a) => {
        if (a.type === "back") {
          setStep((s) => (s && s.stage !== "loading" ? null : s));
          return true;
        }
        if (a.type === "menu" && !step && focusedCard) {
          setEditing(focusedCard);
          return true;
        }
        return false;
      }),
    [step, focusedCard],
  );

  const advance = (s: Step, card: ProfileCard, patch: Partial<Step> = {}) => setStep({ ...s, ...patch, stage: nextStage(card, s.stage) });

  const onPin = async (pin: string): Promise<PinResult> => {
    if (!step || !selected) return "wrong";
    try {
      await api.profileCheckPin(step.id, pin);
      advance(step, selected, { pin });
      return "ok";
    } catch (e) {
      const p = pinError(e);
      if (p) return p;
      toast.error(asError(e).message);
      return "wrong";
    }
  };

  const onPlexPin = async (plexPin: string): Promise<PinResult> => {
    if (!step || !selected) return "wrong";
    advance(step, selected, { plexPin, plexWrong: false }); // plex.tv checks it while loading
    return "ok";
  };

  const onSignedIn = () => {
    if (!step || !selected) return;
    if (step.signIn + 1 < step.pending.length) setStep({ ...step, signIn: step.signIn + 1 });
    else advance(step, selected);
  };

  // Loading: switch in Rust, fill the ring, land on Home.
  useEffect(() => {
    if (step?.stage !== "loading") return;
    let alive = true;
    switchProfile(step.id, { pin: step.pin, plexPin: step.plexPin }).then(
      (outcome) => {
        if (!alive) return;
        setProgress(1);
        setTimeout(() => {
          navigate("/", { replace: true });
          if (outcome.failed.length) toast(`${outcome.failed.join(", ")} did not answer`, { description: "The rest of your library is here." });
        }, 280);
      },
      (e) => {
        if (!alive) return;
        if (pinError(e) === "wrong" && step.plexPin) {
          setStep({ ...step, stage: "plex-pin", plexPin: undefined, plexWrong: true });
          return;
        }
        toast.error(asError(e).message);
        setStep(null);
      },
    );
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step?.stage]);

  const addTile = data?.mode === "serverUsers" ? "Other User" : "Add Profile";

  return (
    <div className="relative h-full overflow-hidden">
      <AmbientBackdrop />
      <TitleBar />
      <div className="relative flex h-full flex-col items-center justify-center gap-12 px-[var(--gutter)]">
        <motion.div layout transition={panelSpring} className="flex flex-col items-center gap-6">
          <motion.div
            animate={intro ? { scale: [0.96, 1, 0.96], opacity: 1 } : { scale: 0.62, opacity: 0.9 }}
            transition={intro ? { duration: 2.4, repeat: Infinity, ease: "easeInOut" } : panelSpring}
            className="drop-shadow-[0_0_40px_rgb(255_255_255/0.25)]"
          >
            <FlickMark title="Flick" className="h-14 w-auto" />
          </motion.div>
          <AnimatePresence>
            {ready && !step && (
              <motion.h1
                key="title"
                initial={{ opacity: 0, y: 8 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.45, ease: [0.32, 0.72, 0, 1] }}
                className="text-[2.75rem] leading-none font-bold tracking-tight"
              >
                Who’s watching?
              </motion.h1>
            )}
          </AnimatePresence>
        </motion.div>

        {/* The chosen avatar carries the layoutId from its tile to the centre. */}
        <AnimatePresence>
          {ready && !step && (
            <FocusGroup key="tiles" focusKey="profiles" autoFocus className="flex max-w-6xl flex-wrap justify-center gap-x-10 gap-y-12">
              {cards.map((card, i) => (
                <ProfileTile
                  key={card.id}
                  card={card}
                  index={i}
                  dimmed={focused !== null && focused !== card.id}
                  onFocused={() => setFocused(card.id)}
                  onSelect={() => choose(card)}
                  onEdit={() => setEditing(card)}
                />
              ))}
              <AddTile label={addTile} index={cards.length} onSelect={() => (data?.mode === "serverUsers" ? setOtherUser(true) : setEditing("new"))} />
            </FocusGroup>
          )}
          {step && selected && (
            <motion.div key="chosen" className="flex flex-col items-center gap-8">
              <div className="relative">
                <ProfileAvatar profile={selected} layoutId={`profile-avatar-${selected.id}`} className="size-40 text-6xl" />
                {step.stage === "loading" && <ProgressRing done={progress >= 1} />}
              </div>
              <motion.p initial={{ opacity: 0 }} animate={{ opacity: 1 }} className="text-2xl font-semibold">
                {selected.name}
              </motion.p>
              <AnimatePresence mode="wait">
                {step.stage === "pin" && (
                  <PinPanel key="pin">
                    <PinPad title="Enter PIN" hint={`${selected.name}’s profile is protected.`} onSubmit={onPin} onCancel={() => setStep(null)} />
                  </PinPanel>
                )}
                {step.stage === "plex-pin" && (
                  <PinPanel key="plex-pin">
                    <PinPad title="Plex PIN" hint={step.plexWrong ? "Wrong Plex PIN. Try again." : "This Plex Home member is protected by Plex."} onSubmit={onPlexPin} onCancel={() => setStep(null)} />
                  </PinPanel>
                )}
                {step.stage === "sign-in" && step.pending[step.signIn] && <AccountSignIn key={`sign-in-${step.signIn}`} account={step.pending[step.signIn]!} onDone={onSignedIn} />}
              </AnimatePresence>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
      <OtherUserDialog open={otherUser} onClose={() => setOtherUser(false)} onAdded={() => void query.refetch()} />
      <ProfileEditor target={editing} onClose={() => setEditing(null)} />
    </div>
  );
}

function PinPanel({ children }: { children: ReactNode }) {
  return (
    <motion.div initial={{ opacity: 0, y: 48 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: 24 }} transition={panelSpring} className="glass-strong rounded-[2rem] p-8">
      {children}
    </motion.div>
  );
}

/** Thin ring around the avatar: spins while loading, closes when ready. */
function ProgressRing({ done }: { done: boolean }) {
  return (
    <motion.svg
      viewBox="0 0 100 100"
      className="pointer-events-none absolute -inset-3 size-[calc(100%+1.5rem)]"
      animate={done ? { rotate: 0 } : { rotate: 360 }}
      transition={done ? focusSpring : { duration: 1.2, repeat: Infinity, ease: "linear" }}
    >
      <circle cx="50" cy="50" r="48" fill="none" stroke="rgb(255 255 255 / 0.12)" strokeWidth="1.5" />
      <motion.circle
        cx="50"
        cy="50"
        r="48"
        fill="none"
        stroke="white"
        strokeWidth="1.5"
        strokeLinecap="round"
        initial={{ pathLength: 0.22 }}
        animate={{ pathLength: done ? 1 : 0.22 }}
        transition={{ duration: 0.28, ease: [0.32, 0.72, 0, 1] }}
      />
    </motion.svg>
  );
}

function AddTile({ label, index, onSelect }: { label: string; index: number; onSelect: () => void }) {
  const tv = useTv<HTMLButtonElement>({ focusKey: "profile:add", scroll: false });
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      onClick={onSelect}
      initial={{ opacity: 0, y: 24, scale: 0.92 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      transition={{ ...panelSpring, delay: index * 0.06 }}
      className="flex w-[11rem] cursor-pointer flex-col items-center gap-4 outline-none"
    >
      <motion.span
        animate={{ scale: tv.showFocus ? 1.1 : 1 }}
        transition={focusSpring}
        className={cn("grid size-36 place-items-center rounded-full border-2 border-dashed border-white/25 text-white/60 transition-colors", tv.showFocus && "border-white bg-white text-black")}
      >
        <Plus className="size-10" />
      </motion.span>
      <span className="text-lg font-semibold text-white/70">{label}</span>
    </motion.button>
  );
}
```

`ProfileEditor` is created in Task 15. To keep this task compiling and testable on its own, create a placeholder now — `ui/src/features/profiles/ProfileEditor.tsx`:

```tsx
// Replaced in Task 15 (profile sheet: name, colour, PIN, connections).
import type { ProfileCard } from "@/ipc/bindings/ProfileCard";

export function ProfileEditor(_: { target: ProfileCard | "new" | null; onClose: () => void }) {
  return null;
}
```

- [ ] **Step 5: `ProfileGate`**

```tsx
// At startup, with multi-user on and nobody picked (Rust resumed no
// profile), show the picker once.
import { useQuery } from "@tanstack/react-query";
import { useEffect, useRef } from "react";
import { useLocation, useNavigate } from "react-router";
import { profilesQuery, visibleProfiles } from "@/lib/profiles";

export function ProfileGate() {
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const { data } = useQuery(profilesQuery);
  const done = useRef(false);
  useEffect(() => {
    if (!data || done.current) return;
    done.current = true;
    if (data.enabled && !data.active && visibleProfiles(data.profiles).length > 0 && pathname !== "/profiles") navigate("/profiles", { replace: true });
  }, [data, pathname, navigate]);
  return null;
}
```

- [ ] **Step 6: Wire into `App.tsx`**

```tsx
import { LayoutGroup, MotionConfig } from "motion/react";
import { ProfileGate } from "@/features/profiles/ProfileGate";
import { ProfilePicker } from "@/features/profiles/ProfilePicker";
// …
        <BrowserRouter>
          <GlobalActions />
          <ProfileGate />
          {/* One layout group: a profile's avatar flies from the picker to the sidebar. */}
          <LayoutGroup>
            <Routes>
              <Route path="/play" element={<PlayerRoute />} />
              <Route path="/profiles" element={<ProfilePicker />} />
              <Route element={<Shell />}>
                {/* unchanged */}
              </Route>
            </Routes>
          </LayoutGroup>
        </BrowserRouter>
```

- [ ] **Step 7: Typecheck and tests**

Run: `pnpm --dir ui run typecheck && pnpm --dir ui run test`
Expected: PASS.

- [ ] **Step 8: Visual check in the app** (multi-user can be turned on from the dev tools console until Task 15: `window.__TAURI_INTERNALS__.invoke("profiles_configure", { enabled: true, mode: "serverUsers", askOnStartup: true, pin: null })`, then restart)

Expected, on restart:
- the Flick mark breathes, rises; "Who’s watching?" fades in; tiles cascade (60 ms apart);
- arrows move focus: focused avatar lifts with white ring, others dim; the light behind changes to the focused profile's colour;
- 1–9 choose directly; choosing flies the avatar to the centre, the ring spins, then Home appears and the avatar lands in the sidebar (the sidebar avatar is Task 14; until then it fades out);
- a profile with a pending Jellyfin account shows the sign-in card; Skip continues;
- with *Reduce motion* (Settings › animations at 0), only fades.

- [ ] **Step 9: Commit**

```bash
git add ui/src
git commit -m "UI: profile picker

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 14: UI — changement rapide (sidebar, TabBar, transition)

**Files:**
- Create: `ui/src/features/profiles/ProfileSwitcher.tsx`
- Modify: `ui/src/shell/Sidebar.tsx`, `ui/src/shell/TabBar.tsx`, `ui/src/shell/Shell.tsx`

**Interfaces:**
- Consumes: `profilesQuery`, `switchProfile`, `finishSwitch`, `useProfileSwitch`, `PinPad`, `ProfileAvatar`, `AccountPills`, `needsPlexPin`, `pendingSignIns`, `visibleProfiles`, `pinError`
- Produces: `<SidebarProfile />`, `<TabBarProfile />`

- [ ] **Step 1: `ProfileSwitcher.tsx`**

```tsx
// Quick switch: the active profile in the sidebar (or the Flick Frame tab
// bar) opens a glass popover of the other people. A Flick PIN is typed in
// place; anything else to type (Plex PIN, sign-ins) goes through the picker.
import { useQuery } from "@tanstack/react-query";
import { ChevronsUpDown, Lock, Settings2, Users } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { type ReactNode, type RefObject, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useNavigate } from "react-router";
import { toast } from "sonner";
import { AccountPills } from "@/components/tv/AccountPills";
import { type PinResult, PinPad } from "@/components/tv/PinPad";
import { ProfileAvatar } from "@/components/tv/ProfileAvatar";
import { api, asError } from "@/ipc/api";
import type { ProfileCard } from "@/ipc/bindings/ProfileCard";
import type { ProfilesState } from "@/ipc/bindings/ProfilesState";
import { focusSpring, panelSpring } from "@/lib/motion";
import { needsPlexPin, pendingSignIns, pinError, profilesQuery, switchProfile, useProfileSwitch, visibleProfiles } from "@/lib/profiles";
import { useSidebar } from "@/lib/sidebar";
import { cn } from "@/lib/utils";
import { FocusGroup, useTv } from "@/nav/Focusable";
import { onAction } from "@/nav/input";
import { focusKey } from "@/nav/spatial";

function useActive() {
  const data = useQuery(profilesQuery).data;
  const active = data?.enabled ? data.profiles.find((p) => p.id === data.active) : undefined;
  return { data, active };
}

// The focusable only exists while there is a profile to show: a registered
// focus target without an element would trap arrow navigation.
export function SidebarProfile() {
  const { data, active } = useActive();
  return data && active ? <SidebarProfileButton data={data} active={active} /> : null;
}

export function TabBarProfile() {
  const { data, active } = useActive();
  return data && active ? <TabBarProfileButton data={data} active={active} /> : null;
}

function SidebarProfileButton({ data, active }: { data: ProfilesState; active: ProfileCard }) {
  const [open, setOpen] = useState(false);
  const collapsed = useSidebar((s) => s.collapsed);
  const tv = useTv<HTMLButtonElement>({ focusKey: "nav:profile", scroll: false });
  return (
    <>
      <motion.button
        ref={tv.ref}
        type="button"
        {...tv.props}
        onClick={() => setOpen(true)}
        title={collapsed ? active.name : undefined}
        aria-label={`Profile: ${active.name}. Switch profile`}
        animate={{ scale: tv.showFocus ? 1.04 : 1 }}
        transition={focusSpring}
        className={cn(
          "mb-3 flex h-11 w-full cursor-pointer items-center gap-3 rounded-[1rem] px-2 text-left transition-colors",
          tv.showFocus ? "bg-white text-black" : "hover:bg-white/[0.07]",
        )}
      >
        <ProfileAvatar profile={active} layoutId={`profile-avatar-${active.id}`} className="size-7 text-[0.7rem]" />
        <span className="min-w-0 flex-1 truncate text-[0.9375rem] font-medium transition-opacity delay-200 duration-300 in-data-[sidebar=collapsed]:opacity-0 in-data-[sidebar=collapsed]:delay-0">
          {active.name}
        </span>
        <ChevronsUpDown className="size-4 shrink-0 opacity-50 in-data-[sidebar=collapsed]:opacity-0" />
      </motion.button>
      <ProfilePopover open={open} anchor={tv.ref} side="left" state={data} onClose={() => setOpen(false)} />
    </>
  );
}

function TabBarProfileButton({ data, active }: { data: ProfilesState; active: ProfileCard }) {
  const [open, setOpen] = useState(false);
  const tv = useTv<HTMLButtonElement>({ focusKey: "nav:profile", scroll: false });
  return (
    <>
      <motion.button
        ref={tv.ref}
        type="button"
        {...tv.props}
        onClick={() => setOpen(true)}
        aria-label={`Profile: ${active.name}. Switch profile`}
        animate={{ scale: tv.showFocus ? 1.12 : 1 }}
        transition={focusSpring}
        className={cn("ml-1 grid size-11 cursor-pointer place-items-center rounded-full", tv.showFocus && "ring-2 ring-white")}
      >
        <ProfileAvatar profile={active} layoutId={`profile-avatar-${active.id}`} className="size-9 text-xs" />
      </motion.button>
      <ProfilePopover open={open} anchor={tv.ref} side="right" state={data} onClose={() => setOpen(false)} />
    </>
  );
}

function ProfilePopover({
  open,
  anchor,
  side,
  state,
  onClose,
}: {
  open: boolean;
  anchor: RefObject<HTMLElement | null>;
  side: "left" | "right";
  state: ProfilesState;
  onClose: () => void;
}) {
  const navigate = useNavigate();
  const [pinFor, setPinFor] = useState<ProfileCard | null>(null);
  const switching = useProfileSwitch((s) => s.phase !== "idle");
  const [pos, setPos] = useState({ top: 0, left: 0, right: 0 });
  const close = useRef(onClose);
  close.current = onClose;

  useEffect(() => {
    if (!open) return;
    const r = anchor.current?.getBoundingClientRect();
    if (r) setPos({ top: r.bottom + 8, left: r.left, right: window.innerWidth - r.right });
    setPinFor(null);
    return onAction((a) => {
      if (a.type !== "back") return false;
      close.current();
      focusKey("nav:profile");
      return true;
    });
  }, [open, anchor]);

  const others = visibleProfiles(state.profiles).filter((p) => p.id !== state.active);

  const leaveTo = (path: string) => {
    onClose();
    navigate(path);
  };

  const go = async (card: ProfileCard, pin?: string) => {
    onClose();
    try {
      const outcome = await switchProfile(card.id, { pin });
      navigate("/");
      if (outcome.failed.length) toast(`${outcome.failed.join(", ")} did not answer`);
    } catch (e) {
      toast.error(asError(e).message);
    }
  };

  const pick = (card: ProfileCard) => {
    if (switching) return;
    if (needsPlexPin(card) || pendingSignIns(card).length > 0) {
      onClose();
      navigate(`/profiles?pick=${encodeURIComponent(card.id)}`);
    } else if (card.locked) setPinFor(card);
    else void go(card);
  };

  const onPin = async (pin: string): Promise<PinResult> => {
    if (!pinFor) return "wrong";
    try {
      await api.profileCheckPin(pinFor.id, pin);
      void go(pinFor, pin);
      return "ok";
    } catch (e) {
      return pinError(e) ?? "wrong";
    }
  };

  return createPortal(
    <AnimatePresence>
      {open && (
        <>
          <motion.div key="scrim" className="fixed inset-0 z-40" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} onClick={onClose} />
          <motion.div
            key="panel"
            layout
            initial={{ opacity: 0, scale: 0.92, y: -6 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.95, y: -4 }}
            transition={panelSpring}
            style={side === "left" ? { top: pos.top, left: pos.left, transformOrigin: "top left" } : { top: pos.top, right: pos.right, transformOrigin: "top right" }}
            className="glass-strong fixed z-50 w-[21rem] overflow-hidden rounded-[1.75rem] p-2 text-white"
          >
            <FocusGroup focusKey="profile-popover" boundary autoFocus className="flex flex-col gap-0.5">
              {pinFor ? (
                <div className="p-4">
                  <PinPad title={`Enter ${pinFor.name}’s PIN`} onSubmit={onPin} onCancel={() => setPinFor(null)} />
                </div>
              ) : (
                <>
                  {others.map((p) => (
                    <PopoverRow key={p.id} onClick={() => pick(p)} icon={<ProfileAvatar profile={p} className="size-10 text-sm" />}>
                      <span className="flex min-w-0 flex-1 flex-col items-start gap-1">
                        <span className="flex items-center gap-1.5 font-semibold">
                          {p.name}
                          {p.locked && <Lock className="size-3.5 opacity-60" />}
                        </span>
                        <AccountPills accounts={p.accounts} className="justify-start" />
                      </span>
                    </PopoverRow>
                  ))}
                  {others.length > 0 && <div className="mx-3 my-1.5 h-px bg-white/10" />}
                  <PopoverRow icon={<Users className="size-5" />} onClick={() => leaveTo("/profiles")}>
                    All Profiles…
                  </PopoverRow>
                  <PopoverRow icon={<Settings2 className="size-5" />} onClick={() => leaveTo("/settings?s=profiles")}>
                    Manage Profiles
                  </PopoverRow>
                </>
              )}
            </FocusGroup>
          </motion.div>
        </>
      )}
    </AnimatePresence>,
    document.body,
  );
}

function PopoverRow({ icon, children, onClick }: { icon: ReactNode; children: ReactNode; onClick: () => void }) {
  const tv = useTv<HTMLButtonElement>({ scroll: false });
  return (
    <button
      ref={tv.ref}
      type="button"
      {...tv.props}
      onClick={onClick}
      className={cn(
        // 28 px panel, 8 px padding: rows use 20 px so the corners stay concentric.
        "flex min-h-12 w-full cursor-pointer items-center gap-3 rounded-[1.25rem] px-3 py-2 text-left text-[0.9375rem] transition-colors",
        tv.showFocus ? "bg-white text-black [&_span]:text-black/70" : "hover:bg-white/[0.08]",
      )}
    >
      <span className="grid w-10 shrink-0 place-items-center">{icon}</span>
      {children}
    </button>
  );
}
```

- [ ] **Step 2: Sidebar** — `ui/src/shell/Sidebar.tsx`: import `SidebarProfile` from `@/features/profiles/ProfileSwitcher` and render it right after `<Brand />`:

```tsx
        <Brand />
        <SidebarProfile />
```

- [ ] **Step 3: TabBar** — `ui/src/shell/TabBar.tsx`: import `TabBarProfile` and render it just before the Exit Flick Frame tab:

```tsx
        <TabBarProfile />
        <Tab id="frame" label="Exit Flick Frame" icon={<Minimize2 />} onClick={() => void toggleFrame()} />
```

- [ ] **Step 4: Content transition** — `ui/src/shell/Shell.tsx`: import `finishSwitch, useProfileSwitch` from `@/lib/profiles`; in `Shell()` add `const phase = useProfileSwitch((s) => s.phase);` and replace the content `motion.div`:

```tsx
        <motion.div
          key={location.pathname}
          // After a switch the new profile's screen fades in; ordinary route
          // changes only slide (no fade: see the note above).
          initial={phase === "entering" ? { y: 10, opacity: 0 } : { y: 10 }}
          animate={
            phase === "leaving"
              ? { y: 0, opacity: 0, filter: "blur(12px)" }
              : // Back to `none`: a leftover filter would cut glass panels off the artwork.
                { y: 0, opacity: 1, filter: "blur(0px)", transitionEnd: { filter: "none" } }
          }
          transition={phase === "leaving" ? { duration: 0.2, ease: [0.4, 0, 1, 1] } : { duration: 0.45, ease: [0.32, 0.72, 0, 1] }}
          onAnimationComplete={() => phase === "entering" && finishSwitch()}
        >
          <Outlet />
        </motion.div>
```

Update the comment above it: the fade runs only during a profile switch (≈ 200 ms), when glass panels losing their blur briefly is acceptable.

- [ ] **Step 5: Typecheck and tests**

Run: `pnpm --dir ui run typecheck && pnpm --dir ui run test`
Expected: PASS.

- [ ] **Step 6: Visual check**

With two profiles (one with a Flick PIN once Task 15 exists; until then two open ones):
- the sidebar shows avatar + name under the Flick mark; collapsed sidebar shows the avatar only; Flick Frame shows it at the right of the tab bar;
- Enter / click opens the glass popover from the avatar; arrows move inside it; Back closes it and returns focus to the avatar;
- choosing an open profile: the content blurs out (~200 ms), Home of the new profile fades in, the sidebar avatar morphs; time it with the dev tools Performance panel: < 500 ms when the cache is warm;
- no screen shows the previous profile's rows at any moment.

- [ ] **Step 7: Commit**

```bash
git add ui/src
git commit -m "UI: quick profile switch from the sidebar and tab bar

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 15: UI — Réglages › Profils et fiche de profil

**Files:**
- Create: `ui/src/features/profiles/ProfilesSettings.tsx`
- Replace: `ui/src/features/profiles/ProfileEditor.tsx` (placeholder from Task 13)
- Modify: `ui/src/features/settings/Settings.tsx`

**Interfaces:**
- Consumes: all profile commands; `SettingsGroup`, `ToggleRow`, `SelectRow`, `LinkRow`, `TvDialog`, `TextField`, `Button`, `Switch`, `PinPad`, `serversQuery`
- Produces: Settings section `profiles` ("Profiles"); `/settings?personal=1` shows only the personal sections (Appearance, Playback, Subtitles); `<ProfileEditor target onClose />`

- [ ] **Step 1: `ProfilesSettings.tsx`**

```tsx
// Settings › Profiles: turn multi-user on, choose where profiles come from,
// list and edit them. Changes that could get past a PIN ask for one.
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useNavigate } from "react-router";
import { toast } from "sonner";
import { AccountPills } from "@/components/tv/AccountPills";
import { Spinner } from "@/components/tv/Feedback";
import { type PinResult, PinPad } from "@/components/tv/PinPad";
import { LinkRow, SelectRow, SettingsGroup, ToggleRow } from "@/components/tv/SettingsList";
import { TvDialog } from "@/components/tv/TvDialog";
import { api, asError } from "@/ipc/api";
import type { ProfileCard } from "@/ipc/bindings/ProfileCard";
import type { ProfileMode } from "@/ipc/bindings/ProfileMode";
import { pinError, profilesQuery } from "@/lib/profiles";
import { queryClient } from "@/lib/queryClient";
import { loadSettings } from "@/lib/settings";
import { ProfileEditor } from "./ProfileEditor";

const MODES: { value: ProfileMode; label: string }[] = [
  { value: "serverUsers", label: "Server users" },
  { value: "local", label: "Flick profiles" },
  { value: "linked", label: "Linked profiles" },
];

const MODE_NOTES: Record<ProfileMode, string> = {
  serverUsers: "Everyone who has an account on your servers gets a profile. The same name on several servers is one person.",
  local: "Profiles made here. Each one signs in to its own servers.",
  linked: "Profiles made here, each using some of the accounts already signed in.",
};

export function ProfilesSettings() {
  const navigate = useNavigate();
  const { data } = useQuery(profilesQuery);
  const [editing, setEditing] = useState<ProfileCard | "new" | null>(null);
  const [showHidden, setShowHidden] = useState(false);
  const [retry, setRetry] = useState<((pin: string) => Promise<PinResult>) | null>(null);

  if (!data) return <Spinner />;

  const configure = async (enabled: boolean, mode: ProfileMode, ask: boolean, pin: string | null = null): Promise<PinResult> => {
    try {
      const s = await api.profilesConfigure(enabled, mode, ask, pin);
      queryClient.setQueryData(profilesQuery.queryKey, s);
      await loadSettings();
      void queryClient.invalidateQueries();
      return "ok";
    } catch (e) {
      const p = pinError(e);
      if (p && pin === null) {
        setRetry(() => (typed: string) => configure(enabled, mode, ask, typed));
        return "ok";
      }
      if (p) return p;
      toast.error(asError(e).message);
      return "ok";
    }
  };

  const hidden = data.profiles.filter((p) => p.hidden).length;
  const listed = data.profiles.filter((p) => showHidden || !p.hidden);

  return (
    <>
      <SettingsGroup note={data.enabled ? <p>{MODE_NOTES[data.mode]}</p> : undefined}>
        <ToggleRow
          label="Multiple users"
          hint="Choose who is watching. Each person keeps their own history, favourites and preferences."
          checked={data.enabled}
          onChange={(v) => void configure(v, data.mode, data.askOnStartup)}
        />
        {data.enabled && (
          <>
            <SelectRow label="Profiles come from" value={data.mode} options={MODES} onChange={(m) => void configure(true, m, data.askOnStartup)} />
            <ToggleRow label="Ask at startup" hint="Show “Who’s watching?” each time Flick opens." checked={data.askOnStartup} onChange={(v) => void configure(true, data.mode, v)} />
          </>
        )}
      </SettingsGroup>

      {data.enabled && (
        <SettingsGroup title="Profiles">
          {listed.map((p) => (
            <LinkRow key={p.id} label={`${p.name}${p.hidden ? " (hidden)" : ""}${p.id === data.active ? " · you" : ""}`} value={<AccountPills accounts={p.accounts} className="justify-end" />} onClick={() => setEditing(p)} />
          ))}
          {data.mode !== "serverUsers" && <LinkRow label="Add Profile" onClick={() => setEditing("new")} />}
          {data.mode === "serverUsers" && hidden > 0 && (
            <LinkRow label={showHidden ? "Hide Hidden Profiles" : `Show Hidden Profiles (${hidden})`} onClick={() => setShowHidden(!showHidden)} />
          )}
          <LinkRow label="Choose Who’s Watching…" onClick={() => navigate("/profiles")} />
        </SettingsGroup>
      )}

      <ProfileEditor target={editing} onClose={() => setEditing(null)} />
      <TvDialog open={!!retry} onClose={() => setRetry(null)} title="PIN Required">
        <PinPad
          title="Enter a profile PIN"
          hint="This change needs the PIN of a protected profile."
          onSubmit={async (pin) => {
            const r = (await retry?.(pin)) ?? "ok";
            if (r === "ok") setRetry(null);
            return r;
          }}
          onCancel={() => setRetry(null)}
        />
      </TvDialog>
    </>
  );
}
```

- [ ] **Step 2: `ProfileEditor.tsx`** (replaces the placeholder)

```tsx
// A profile's sheet: name, colour, picture, PIN, accounts, delete/hide.
// A protected profile is unlocked with its PIN first; that PIN then
// authorises every change in the sheet (Rust checks it again).
import { useQuery } from "@tanstack/react-query";
import { Check } from "lucide-react";
import { motion } from "motion/react";
import { useEffect, useState } from "react";
import { useNavigate } from "react-router";
import { toast } from "sonner";
import { Button } from "@/components/tv/Button";
import { Notice } from "@/components/tv/Feedback";
import { type PinResult, PinPad } from "@/components/tv/PinPad";
import { ProfileAvatar } from "@/components/tv/ProfileAvatar";
import { ProviderLogo } from "@/components/tv/ServerBadge";
import { Segmented } from "@/components/tv/Segmented";
import { Switch } from "@/components/tv/Switch";
import { TextField } from "@/components/tv/TextField";
import { TvDialog } from "@/components/tv/TvDialog";
import { api, asError } from "@/ipc/api";
import type { AvatarStyle } from "@/ipc/bindings/AvatarStyle";
import type { ProfileCard } from "@/ipc/bindings/ProfileCard";
import type { ServerId } from "@/ipc/bindings/ServerId";
import { focusSpring } from "@/lib/motion";
import { allServersQuery, PROFILE_COLORS, pinError, profilesQuery } from "@/lib/profiles";
import { queryClient } from "@/lib/queryClient";
import { cn } from "@/lib/utils";
import { FocusGroup, useTv } from "@/nav/Focusable";

type Target = ProfileCard | "new" | null;
type PinStep = null | { stage: "new" } | { stage: "confirm"; first: string };

const refresh = () => Promise.all([queryClient.invalidateQueries({ queryKey: profilesQuery.queryKey }), queryClient.invalidateQueries({ queryKey: ["servers"] })]);

export function ProfileEditor({ target, onClose }: { target: Target; onClose: () => void }) {
  const navigate = useNavigate();
  const state = useQuery(profilesQuery).data;
  // Every connection on this computer, not just the active profile's.
  const servers = useQuery(allServersQuery).data ?? [];
  const card = target && target !== "new" ? target : null;
  const [unlock, setUnlock] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [color, setColor] = useState(PROFILE_COLORS[0]!);
  const [avatar, setAvatar] = useState<AvatarStyle>("server");
  const [links, setLinks] = useState<ServerId[]>([]);
  const [pinStep, setPinStep] = useState<PinStep>(null);
  const [orphans, setOrphans] = useState<ServerId[]>([]);

  useEffect(() => {
    setUnlock(null);
    setPinStep(null);
    setOrphans([]);
    if (card) {
      setName(card.name);
      setColor(card.color);
      setAvatar(card.avatarKey ? "server" : "initials");
      setLinks(card.accounts.flatMap((a) => (a.connection ? [a.connection] : [])));
    } else if (target === "new") {
      setName("");
      setColor(PROFILE_COLORS[(state?.profiles.length ?? 0) % PROFILE_COLORS.length]!);
      setAvatar("server");
      setLinks([]);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target]);

  if (!target || !state) return null;
  const mode = state.mode;
  const derived = mode === "serverUsers";
  const locked = !!card?.locked && unlock === null;
  const isActive = card?.id === state.active;

  const fail = (e: unknown) => toast.error(asError(e).message);

  const leaveTo = (path: string) => {
    onClose();
    navigate(path);
  };

  const save = async () => {
    try {
      if (!card) {
        const id = await api.profileCreate(name, color);
        if (mode === "linked" && links.length) await api.profileUpdate(id, { name: null, color: null, avatar: null, connections: links, hidden: null }, null);
      } else {
        await api.profileUpdate(card.id, { name, color, avatar, connections: mode === "linked" ? links : null, hidden: null }, unlock);
      }
      await refresh();
      onClose();
    } catch (e) {
      fail(e);
    }
  };

  const setHidden = async (hidden: boolean) => {
    if (!card) return;
    try {
      await api.profileUpdate(card.id, { name: null, color: null, avatar: null, connections: null, hidden }, unlock);
      await refresh();
      onClose();
    } catch (e) {
      fail(e);
    }
  };

  const remove = async () => {
    if (!card) return;
    try {
      const left = await api.profileDelete(card.id, unlock);
      await refresh();
      if (isActive) navigate("/profiles");
      if (left.length) setOrphans(left);
      else onClose();
    } catch (e) {
      fail(e);
    }
  };

  const detach = async (connection: ServerId) => {
    if (!card) return;
    try {
      await api.profileDetach(card.id, connection, unlock);
      await refresh();
      onClose();
    } catch (e) {
      fail(e);
    }
  };

  const onUnlock = async (pin: string): Promise<PinResult> => {
    if (!card) return "wrong";
    try {
      await api.profileCheckPin(card.id, pin);
      setUnlock(pin);
      return "ok";
    } catch (e) {
      return pinError(e) ?? "wrong";
    }
  };

  const onNewPin = async (pin: string): Promise<PinResult> => {
    if (!card || !pinStep) return "wrong";
    if (pinStep.stage === "new") {
      setPinStep({ stage: "confirm", first: pin });
      return "ok";
    }
    if (pin !== pinStep.first) {
      setPinStep({ stage: "new" });
      return "wrong";
    }
    try {
      await api.profileSetPin(card.id, unlock, pin);
      setUnlock(pin);
      setPinStep(null);
      await refresh();
      toast.success("PIN set");
      return "ok";
    } catch (e) {
      return pinError(e) ?? "wrong";
    }
  };

  const removePin = async () => {
    if (!card) return;
    try {
      await api.profileSetPin(card.id, unlock, null);
      await refresh();
      toast.success("PIN removed");
      onClose();
    } catch (e) {
      fail(e);
    }
  };

  const title = card ? card.name : "New Profile";

  return (
    <TvDialog open onClose={onClose} title={title}>
      {locked ? (
        <PinPad title="Enter PIN" hint="This profile is protected." onSubmit={onUnlock} onCancel={onClose} />
      ) : pinStep ? (
        <PinPad key={pinStep.stage} title={pinStep.stage === "new" ? "Choose a PIN" : "Confirm the PIN"} hint="4 digits." onSubmit={onNewPin} onCancel={() => setPinStep(null)} />
      ) : orphans.length ? (
        <>
          <Notice>
            {orphans.length === 1 ? "One sign-in is" : `${orphans.length} sign-ins are`} no longer used by any profile. Remove {orphans.length === 1 ? "it" : "them"} from this computer?
          </Notice>
          <FocusGroup className="flex gap-3">
            <Button
              variant="danger"
              onClick={() =>
                void Promise.all(orphans.map((id) => api.serverRemove(id)))
                  .then(refresh)
                  .then(onClose, fail)
              }
            >
              Remove
            </Button>
            <Button onClick={onClose}>Keep</Button>
          </FocusGroup>
        </>
      ) : (
        <>
          <div className="flex items-center gap-5">
            <ProfileAvatar profile={{ id: card?.id ?? "new", name: name || "?", color, avatarKey: avatar === "server" ? (card?.avatarKey ?? null) : null }} className="size-20 text-3xl" />
            <div className="min-w-0 flex-1">
              <TextField label="Name" value={name} onChange={setName} autoFocus={!card} />
            </div>
          </div>

          <FocusGroup className="flex flex-wrap gap-3" aria-label="Colour">
            {PROFILE_COLORS.map((c) => (
              <Swatch key={c} color={c} selected={c === color} onSelect={() => setColor(c)} />
            ))}
          </FocusGroup>

          {card && card.accounts.length > 0 ? (
            <Segmented
              label="Picture"
              value={avatar}
              options={[
                { value: "server", label: "From server" },
                { value: "initials", label: "Initials" },
              ]}
              onChange={setAvatar}
            />
          ) : null}

          {card && (
            <section className="flex flex-col gap-2">
              <h3 className="text-sm font-semibold text-white/60">Accounts</h3>
              {mode === "linked"
                ? servers.map((e) => (
                    <div key={e.server.id} className="flex items-center gap-3 rounded-2xl bg-white/[0.05] px-4 py-2.5">
                      <ProviderLogo kind={e.server.kind} />
                      <span className="flex-1 truncate">
                        {e.server.user.name} · {e.server.name}
                      </span>
                      <Switch
                        label={`Use ${e.server.name} as ${e.server.user.name}`}
                        checked={links.includes(e.server.id)}
                        onChange={(on) => setLinks((l) => (on ? [...l, e.server.id] : l.filter((x) => x !== e.server.id)))}
                      />
                    </div>
                  ))
                : card.accounts.map((a) => (
                    <div key={`${a.kind}:${a.serverName}:${a.userName}`} className="flex items-center gap-3 rounded-2xl bg-white/[0.05] px-4 py-2.5">
                      <ProviderLogo kind={a.kind} />
                      <span className="flex-1 truncate">
                        {a.userName} · {a.serverName}
                        {a.state !== "connected" && <span className="text-white/50"> · {a.state === "pending" ? "sign-in needed" : "offline"}</span>}
                      </span>
                      {derived && a.connection && card.accounts.length > 1 && (
                        <Button size="sm" variant="ghost" onClick={() => void detach(a.connection!)}>
                          Detach
                        </Button>
                      )}
                    </div>
                  ))}
              {mode === "local" &&
                (isActive ? (
                  <Button size="sm" onClick={() => leaveTo("/servers")}>
                    Add a Server
                  </Button>
                ) : (
                  <p className="text-sm text-white/50">Switch to this profile to add its servers.</p>
                ))}
            </section>
          )}

          <FocusGroup className="flex flex-wrap gap-3 pt-2">
            <Button variant="primary" disabled={!name.trim()} onClick={() => void save()}>
              {card ? "Save" : "Create"}
            </Button>
            {card && <Button onClick={() => setPinStep({ stage: "new" })}>{card.locked ? "Change PIN" : "Set PIN"}</Button>}
            {card?.locked && <Button onClick={() => void removePin()}>Remove PIN</Button>}
            {card && isActive && (
              <Button variant="ghost" onClick={() => leaveTo("/settings?personal=1&s=playback")}>
                Preferences
              </Button>
            )}
            {card && derived && (
              <Button variant="ghost" onClick={() => void setHidden(!card.hidden)}>
                {card.hidden ? "Show" : "Hide"}
              </Button>
            )}
            {card && !derived && (
              <Button variant="danger" onClick={() => void remove()}>
                Delete
              </Button>
            )}
          </FocusGroup>
        </>
      )}
    </TvDialog>
  );
}

function Swatch({ color, selected, onSelect }: { color: string; selected: boolean; onSelect: () => void }) {
  const tv = useTv<HTMLButtonElement>({ scroll: false });
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      aria-label={`Colour ${color}`}
      aria-pressed={selected}
      onClick={onSelect}
      animate={{ scale: tv.showFocus ? 1.15 : 1 }}
      transition={focusSpring}
      className={cn("grid size-10 cursor-pointer place-items-center rounded-full", (selected || tv.showFocus) && "ring-2 ring-white ring-offset-2 ring-offset-black/40")}
      style={{ background: color }}
    >
      {selected && <Check className="size-5 text-white drop-shadow" />}
    </motion.button>
  );
}
```

`TvDialog` must render nothing while closed: the early `return <TvDialog open={false} …/>` keeps its exit animation path; if `TvDialog`'s `children` prop is required as `ReactNode`, `null` is valid.

- [ ] **Step 3: Settings wiring** — `ui/src/features/settings/Settings.tsx`:
  - add `["profiles", "Profiles"],` right after `["servers", "Accounts"],` in `sections`;
  - import `ProfilesSettings` from `@/features/profiles/ProfilesSettings` and add to `SectionBody`'s switch:

```tsx
    case "profiles":
      return <ProfilesSettings />;
```

  - personal-only view, in `Settings()`:

```tsx
const PERSONAL = new Set<Section>(["appearance", "playback", "subtitles"]);
// …
  const personal = params.get("personal") === "1";
  const shown = personal ? sections.filter(([id]) => PERSONAL.has(id)) : sections;
```

  use `shown` instead of `sections` for the section list, `setParams(personal ? { s: id, personal: "1" } : { s: id }, { replace: true })` in `onSelect`, and the heading `{personal ? "Your Preferences" : "Settings"}`. Under the heading, when `personal`, add `<p className="px-4 text-sm text-white/55">Saved for the current profile only.</p>`.

- [ ] **Step 4: Typecheck and tests**

Run: `pnpm --dir ui run typecheck && pnpm --dir ui run test`
Expected: PASS.

- [ ] **Step 5: End-to-end checks in the app** (with at least one Jellyfin server with two users, and a Plex server if available)

1. Settings › Profiles › Multiple users on → you stay on your current profile (sidebar shows your avatar); "Profiles come from: Server users" lists one row per person with pills.
2. Open your profile › Set PIN (1234, confirm) → the tile shows a lock. Turn Multiple users off → the PIN pad appears; 0000 shakes; 1234 applies.
3. Wrong PIN five times on the picker → "Too many attempts. Try again in 30 s." counts down.
4. Change "Profiles come from" to Flick profiles → PIN asked (a profile is protected); one profile named after the Windows user holds the current accounts; Add Profile creates a second; switching to it shows an empty Home; in Local mode, adding a server there attaches to it only.
5. Linked profiles: toggle accounts in a profile's sheet; switching to it loads only those.
6. Subtitle size changed while profile A is active does not change profile B's; audio output (machine) is shared.
7. Detach (mode B) an account merged by mistake → it becomes its own tile.
8. Restart with "Ask at startup" off → the last open profile resumes without the picker; with a PIN on it → the picker shows.

- [ ] **Step 6: Commit**

```bash
git add ui/src
git commit -m "UI: profiles settings and profile sheet

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 16: Documentation et vérification finale

**Files:**
- Modify: `ARCHITECTURE.md` (§7.1 storage row, §7.2 frontend list, §10 security), `docs/DESIGN_SYSTEM.md` (components table + a "Profils" paragraph)

- [ ] **Step 1: ARCHITECTURE.md** —
  - §7.1, `oneshot-storage` row: « Réglages, profils (`profiles.json`, regroupement, PIN), cache métadonnées… ».
  - §7.2: mention `features/profiles` (sélecteur plein écran hors Shell, changement rapide depuis la sidebar).
  - §10, add:

```markdown
- **Profils** (`profiles.json`) : un profil = un ensemble de connexions +
  des préférences personnelles ; le profil actif décide des connexions
  chargées. Le PIN (4 chiffres) est haché en argon2id, jamais stocké ni
  journalisé en clair ; 5 échecs → 30 s, puis 60 s, puis 5 min. C'est un
  **verrou d'usage local**, pas une protection contre qui a accès aux
  fichiers de la session : les tokens, eux, restent dans le trousseau.
  Désactiver le multi-utilisateurs ou changer de mode demande le PIN d'un
  profil protégé (vérifié en Rust). Le PIN Plex Home n'est jamais stocké :
  il part à plex.tv à chaque changement.
```

- [ ] **Step 2: DESIGN_SYSTEM.md** — add rows to the components table:

```markdown
| `BackButton` | Retour (fiche, grille de bibliothèque) | bouton `icon` en verre ; Home si l'historique de l'app est vide |
| `ProfileAvatar` | Avatar rond d'un profil | image serveur (proxy `oneshot-img`) ou initiales sur dégradé de sa couleur ; `layoutId` : vole du sélecteur à la sidebar |
| `AccountPills` | Provenance des comptes d'un profil | pleine = connecté, pointillés = à connecter, estompée = hors ligne |
| `PinPad` | PIN à 4 chiffres | touches focalisables + chiffres clavier ; secousse si faux ; décompte si verrouillé |
```

and a short paragraph: « **Profils** : la couleur d'un profil ne teinte que son avatar et la lumière ambiante du sélecteur ; l'interface reste monochrome. Focus d'une tuile ×1,1 + anneau blanc, autres tuiles à 55 %. »

- [ ] **Step 3: Full verification**

Run:
```bash
cargo test --workspace
cargo clippy --workspace
pnpm --dir ui run typecheck
pnpm --dir ui run test
```
Expected: all PASS, no new clippy warning, no type error. Then repeat Task 15 Step 5 checks 1, 2 and 8 once more on the final build, and Task 9 Step 9 (multi-user off = unchanged behaviour).

- [ ] **Step 4: Commit**

```bash
git add ARCHITECTURE.md docs/DESIGN_SYSTEM.md
git commit -m "Docs: multi-user profiles

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
