# Profils multi-utilisateurs + bouton retour — Design

> Date : 2026-09-27 · Statut : validé en discussion, en relecture

## 0. Intention

Flick tourne souvent sur un PC de salon partagé. Chaque personne a son propre
compte sur les serveurs (historique, favoris, reprise, recommandations) et ses
propres goûts (langues, sous-titres). On veut :

- un **mode multi-utilisateurs** activable dans les Réglages ;
- un **écran de sélection** à chaque démarrage (désactivable), beau, animé,
  utilisable à la manette ;
- un **changement rapide** de profil depuis la sidebar, sans redémarrer ;
- trois façons de définir un « utilisateur », au choix dans les Réglages,
  **B par défaut** :
  - **A — Profils Flick** : profils locaux, chacun avec ses propres connexions serveurs ;
  - **B — Utilisateurs des serveurs** : profils déduits des utilisateurs Plex Home et Jellyfin, regroupés par nom ;
  - **C — Profils reliés** : profils locaux reliés à des connexions existantes.

Et, indépendamment : un **bouton retour** sur la fiche d'un titre et la grille
d'une bibliothèque.

Hors périmètre : import d'image d'avatar personnalisée, raccourci clavier
global de changement de profil, changement de profil pendant une lecture.

## 1. Modèle et stockage

### 1.1 Connexions

`servers.json` est **inchangé**. Il est désormais compris comme le réservoir de
toutes les **connexions** de la machine : une connexion = un serveur × un
utilisateur, identifiée par son `ServerId`, token dans le trousseau de l'OS
(clé = `ServerId`). Un même serveur peut y figurer plusieurs fois, une par
utilisateur (`register_server` le permet déjà : il ne dédoublonne que sur
`(kind, remote_id, user.id)`).

Les `ItemRef` (`<server-id>:<clé>`) et le cache de métadonnées sont indexés par
`ServerId`, donc déjà propres à chaque connexion.

### 1.2 `profiles.json`

Nouveau document, écrit atomiquement (`write_json`) par `oneshot-storage`.
Types dans `oneshot-core` (exportés en TS via `ts-rs`) :

```rust
pub struct ProfilesConfig {
    pub enabled: bool,               // défaut false
    pub mode: ProfileMode,           // défaut ServerUsers
    pub ask_on_startup: bool,        // défaut true
    pub last_profile: Option<ProfileId>,
    pub profiles: Vec<Profile>,      // manuels (A/C) et dérivés (B), côte à côte
    pub discovered: Vec<DiscoveredUser>, // cache de la dernière découverte (B)
}

pub enum ProfileMode { ServerUsers, Local, Linked }

pub struct Profile {
    pub id: ProfileId,
    pub name: String,
    pub avatar: Avatar,              // Server(ImageRef/Url) | Initials
    pub color: String,               // une des 8 teintes de la palette profils
    pub pin: Option<PinHash>,        // argon2id + sel ; jamais le PIN en clair
    pub connections: Vec<ServerId>,  // A/C ; ignoré en B (recalculé)
    pub prefs: PersonalSettings,
    pub origin: Origin,              // Manual | Derived { key: String }
    pub hidden: bool,                // B : « Masquer »
    pub detached: Vec<ServerId>,     // B : connexions sorties du regroupement
}

pub struct DiscoveredUser {
    pub server: ServerId,            // serveur configuré sur lequel il existe
    pub kind: ProviderKind,
    pub remote_user_id: String,      // Plex home user uuid / Jellyfin user id
    pub name: String,
    pub avatar: Option<Url>,
    pub has_password: bool,          // Jellyfin
    pub protected: bool,             // Plex Home : PIN Plex requis
}
```

Les profils `Manual` et `Derived` coexistent : changer de mode ne détruit
rien, revenir au mode précédent retrouve ses profils. Seuls les profils de
l'origine correspondant au mode actif sont proposés (`Manual` pour A et C,
`Derived` pour B).

### 1.3 Réglages : matériel vs préférences

`settings.json` garde tout `Settings` : réglages de la machine, et valeurs par
défaut des préférences.

`PersonalSettings` (dans chaque profil) contient :

- `subtitles` (tout `SubtitleSettings`) ;
- `playback.preferred_audio_languages`, `autoplay_next`,
  `autoplay_countdown_secs`, `skip_intro`, `skip_credits`, `max_bitrate`,
  `resume` ;
- `appearance.accent`, `animation_intensity`, `background_intensity`.

Tout le reste est matériel et partagé : audio (sortie, passthrough,
exclusif…), vidéo (décodage, HDR…), réseau, cache, manette, notifications,
confidentialité, avancé, `general`, `appearance.blur` et `density`.

Rust calcule les **réglages effectifs** = `Settings` + `PersonalSettings` du
profil actif. `settings_get` renvoie les effectifs ; `settings_set` reçoit un
`Settings` complet et range chaque champ : personnel → profil actif, reste →
`settings.json`. Même signature qu'aujourd'hui : l'écran Réglages ne change
presque pas. Multi-utilisateurs désactivé : tout va dans `settings.json`,
comme aujourd'hui.

### 1.4 Migration et mode désactivé

- Pas de `profiles.json` → `ProfilesConfig::default()` (`enabled: false`).
  Profil implicite = toutes les connexions non désactivées : comportement
  actuel à l'identique.
- À l'activation : les préférences actuelles de `settings.json` deviennent
  les `prefs` initiales de chaque profil créé ou dérivé.
- Mode A activé sans profil manuel : un premier profil « nom de la session
  OS » est créé avec toutes les connexions actuelles.

### 1.5 Écran Serveurs

Multi-utilisateurs actif : liste les connexions du profil actif.
- A / C : ajouter un serveur crée la connexion et l'ajoute au profil actif.
- B : la connexion rejoint le réservoir ; le regroupement la range.

## 2. Découverte des utilisateurs (mode B)

### 2.1 Sources

**Plex Home** (nouveau, `providers/plex/src/auth.rs`) :
- `GET https://plex.tv/api/v2/home/users` avec le token du compte
  (trousseau, `plex-account`) → membres : uuid, titre, avatar, `protected`,
  `admin`, `restricted`.
- Sélection : `POST https://plex.tv/api/v2/home/users/{uuid}/switch`
  (+ `pin` si protégé) → token de ce membre. Puis `discover(token)` → jetons
  d'accès pour les serveurs Plex **déjà configurés** (même
  `machineIdentifier`) → une connexion enregistrée par serveur
  (`UserProfile.id` = id du membre).
- Pas de token `plex-account` (serveur Plex ajouté avant, ou retiré du
  trousseau) : pas de découverte Plex ; les connexions Plex existantes restent
  regroupées normalement.

**Jellyfin** (nouveau, `providers/jellyfin/src/auth.rs`) :
- `GET {base}/Users/Public` (sans authentification) sur chaque serveur
  Jellyfin configuré → id, nom, `PrimaryImageTag`, `HasPassword`.
- Sélection : `HasPassword == false` → `AuthenticateByName` avec mot de passe
  vide, directement ; sinon mot de passe ou Quick Connect (parcours
  existants). Le token est ensuite gardé : changements suivants instantanés.

### 2.2 Regroupement

Clé = nom normalisé : minuscules, diacritiques retirés (NFKD), espaces
repliés et bordures supprimées. Une tuile = une clé. Elle regroupe :
- les **connexions** existantes dont `user.name` a cette clé (sauf
  `detached`) ;
- les **utilisateurs découverts** de cette clé sans connexion
  correspondante → comptes « à connecter ».

Une connexion détachée (§5) forme son propre profil dérivé, de clé
`detached:<ServerId>`.

### 2.3 Provenance

Sous le nom de chaque tuile, une pastille par compte : `ProviderLogo` + nom du
serveur, `text-white/50`, petite taille. Pleine = connecté, contour pointillé
= à connecter, grisée + « hors ligne » = serveur injoignable.

### 2.4 Vitesse

`discovered` est persisté : l'écran s'affiche instantanément depuis le cache,
puis une découverte silencieuse (délai 3 s par serveur, en parallèle)
rafraîchit la liste. Nouvelles tuiles en fondu ; une tuile disparue de la
découverte reste si elle a au moins une connexion.

### 2.5 Cas particuliers

- **Utilisateurs Jellyfin masqués** de l'écran de connexion : tuile « Autre
  utilisateur » → choix du serveur → connexion manuelle.
- **Plex Home protégé** : PIN Plex redemandé à chaque changement, vérifié en
  ligne par l'appel `switch` (qui renouvelle aussi le token). Hors ligne : ce
  compte n'est pas accessible, message explicite ; le profil reste
  sélectionnable avec ses autres comptes s'il en a.
- **Serveur injoignable** : pastille grisée, profil sélectionnable avec le
  reste.

## 3. Écran de sélection

Route plein écran `/profiles`, **hors du Shell** (pas de sidebar ni de barre
de titre). Composants dans `ui/src/features/profiles/`.

### 3.1 Quand

- Démarrage : si `enabled && ask_on_startup` → `/profiles` ; si `enabled &&
  !ask_on_startup` et `last_profile` valide → activation directe (sans PIN
  si le profil n'en a pas ; sinon → `/profiles`).
- Aucune connexion configurée : pas de sélecteur, arrivée sur l'ajout de
  serveur comme aujourd'hui.

### 3.2 Fond

`AmbientBackdrop` sans affiche (pas d'œuvre d'un autre profil visible) : la
lumière douce prend la **couleur du profil focalisé** et fond d'une teinte à
l'autre (`ambientFade`). La couleur d'un profil ne teinte que son avatar et
cette lumière ; l'interface reste monochrome (cf. `docs/DESIGN_SYSTEM.md`).

### 3.3 Arrivée

1. `FlickMark` au centre, respiration lente (échelle 0,96 ↔ 1, halo) pendant
   la lecture du cache (< 100 ms en général).
2. Le logo remonte et rétrécit vers le haut ; « Qui regarde ? » en fondu.
3. Tuiles en cascade (60 ms d'écart) : y +24 px → 0, ×0,92 → 1, flou 8 px →
   0, `panelSpring`.

### 3.4 Tuiles

- Avatar rond ≈ 9 rem (×1,5 en Flick Frame) : avatar serveur, sinon
  initiales sur un dégradé de la couleur du profil.
- Nom, pastilles de provenance, cadenas si PIN.
- Focus tvOS : ×1,1, anneau blanc, ombre profonde, reflet spéculaire suivant
  le pointeur ; les autres tuiles à 55 % d'opacité.
- Dernière tuile : « + Autre utilisateur » (B) ou « + Ajouter un profil »
  (A/C).
- Menu / clic droit sur une tuile : « Modifier » (fiche du §5.2).
- Clavier : chiffres 1–9 → n-ième tuile.

### 3.5 Sélection

1. Les autres tuiles se dispersent (fondu, ×0,9, flou) ; l'avatar choisi
   glisse au centre (`layoutId` partagé).
2. **PIN Flick** : l'avatar remonte, un pavé en verre glisse du bas (4
   points, chiffres navigables à la manette, saisie clavier). Faux : secousse
   horizontale + points qui clignotent, pas de message bloquant.
   **PIN Plex** : même pavé, mention « PIN Plex ».
3. **Comptes à connecter** : une carte en verre par serveur (mot de passe ou
   Quick Connect), bouton « Passer ».
4. **Chargement** : anneau fin autour de l'avatar pendant `profile_switch`,
   qui se remplit quand la page d'accueil est prête.
5. **Atterrissage** : l'avatar rétrécit vers sa place dans la sidebar (même
   `layoutId`), la page d'accueil apparaît dessous en fondu.
6. **Échec partiel** : on atterrit quand même, toast discret (« Plex
   injoignable »).

### 3.6 Mouvement réduit

`reducedMotion` : fondus enchaînés uniquement ; l'anneau devient une barre
statique.

## 4. Changement rapide

### 4.1 Sidebar et TabBar

- Sidebar : ligne profil sous la marque Flick (avatar 28 px + nom +
  chevron), focusable dans le `FocusGroup` de navigation. Repliée : avatar
  seul, infobulle.
- Flick Frame : avatar à l'extrémité droite de la `TabBar`.
- Absent si le multi-utilisateurs est désactivé.

### 4.2 Popover

Entrée / clic → panneau en verre depuis l'avatar (`panelSpring`) :
- autres profils (avatar moyen, nom, provenance, cadenas) ;
- séparateur ;
- « Tous les profils… » → `/profiles` ;
- « Gérer les profils » → Réglages › Profils.

Navigation manette, Retour ferme. PIN requis → le popover s'agrandit et
affiche le pavé PIN sur place.

### 4.3 Mécanique

Rust — `profile_switch(id, pin: Option<String>, plex_pin: Option<String>)` :
1. vérifie le PIN Flick (et la temporisation, §6.1) ; pour un compte Plex
   Home protégé, appelle `switch` avec le PIN Plex ;
2. vide le `Catalog`, y ajoute les providers des connexions du profil (non
   désactivées, token présent) ;
3. applique ses préférences aux réglages effectifs (et à ce qui en dépend,
   comme `Catalog::set_ttl` s'il y a lieu) ;
4. enregistre `last_profile` ;
5. émet `profile-changed` (id du profil).

Renvoie la liste des connexions qui ont échoué (pour le toast).

UI :
- flou + fondu du contenu (≈ 200 ms) ;
- `queryClient.clear()` (pas une invalidation : jamais une frame des données
  du profil précédent) ;
- `navigate("/")`, réinitialisation du fond ambiant ;
- la page d'accueil se sert du cache de métadonnées (propre à chaque
  connexion) ; morph de l'avatar de la sidebar.

Objectif : < 500 ms cache chaud.

### 4.4 Garde-fous

- Pas de changement pendant la lecture (le lecteur est hors Shell).
- Un changement en cours bloque les autres (verrou côté Rust + état UI).

## 5. Réglages › Profils

### 5.1 Groupe

- **Multi-utilisateurs** (`ToggleRow`), désactivé par défaut.
- **Mode** (`SelectRow`) : Utilisateurs des serveurs / Profils Flick /
  Profils reliés, avec une ligne d'explication propre au mode.
- **Demander au démarrage** (`ToggleRow`), activé par défaut.
- Un `LinkRow` par profil (avatar, nom, provenance).
- A/C : « Ajouter un profil ». B : « Afficher les profils masqués » s'il y en a.

### 5.2 Fiche d'un profil

Panneau en verre (vocabulaire `TvDialog`) :
- nom ; avatar (serveur ou initiales) ; couleur (8 teintes lisibles sur
  noir) ;
- PIN : Définir / Modifier / Retirer ;
- « Réglages de ce profil » → Réglages filtrés sur la partie personnelle ;
- **Connexions** :
  - A : ses serveurs + « Ajouter un serveur » (parcours actuel) ;
  - B : lecture seule, avec « Détacher » par connexion (mauvais regroupement
    → profil à part) ;
  - C : toutes les connexions de la machine, un interrupteur chacune ;
- Supprimer (A/C) ou Masquer (B). Supprimer un profil ne supprime pas les
  connexions qu'un autre profil utilise ; une connexion utilisée par aucun
  profil est proposée à la suppression (token compris).

### 5.3 Verrous

- Modifier / supprimer un profil protégé : son PIN.
- Désactiver le multi-utilisateurs ou changer de mode : le PIN d'un profil
  protégé, s'il en existe un (sinon le profil implicite « voit tout »
  contournerait les PIN).
- Vérifiés côté Rust (commandes refusées sans PIN valide), pas seulement
  dans l'UI.

## 6. Sécurité et erreurs

### 6.1 PIN

- 4 chiffres, hash **argon2id** + sel aléatoire, stocké dans
  `profiles.json`. Jamais en clair, jamais journalisé.
- Après 5 échecs sur un profil : temporisation 30 s, puis 1 min, puis 5 min
  (en mémoire, par profil). Le pavé affiche le décompte.
- Le PIN Plex n'est jamais stocké : transmis à plex.tv uniquement.
- Documenté honnêtement (`ARCHITECTURE.md` §10) : verrou d'usage local, pas
  une protection contre qui a accès aux fichiers de la session. Les tokens
  restent dans le trousseau.

### 6.2 Erreurs

- `profiles.json` illisible : copie `profiles.corrupt.json`, repli sur
  `ProfilesConfig::default()` (désactivé). Jamais de plantage.
- Connexion supprimée ou token expiré : ignorée au changement, signalée « à
  reconnecter » dans l'écran Serveurs ; le profil reste utilisable.
- `last_profile` introuvable : `/profiles`.
- Découverte en échec : tuiles en cache conservées, mention « hors ligne »
  sur la pastille.

## 7. Bouton retour

- Composant `ui/src/components/tv/BackButton.tsx` : bouton rond en verre
  (`Button`, `size="icon" (44 px)`, icône `ArrowLeft`), fixé en haut à gauche du
  contenu (après la sidebar, dans la gouttière), au-dessus du héros.
- Sur `features/detail/Detail.tsx` et `features/library/LibraryGrid.tsx`.
- Action : `navigate(-1)` s'il existe un historique dans l'app
  (`window.history.state.idx > 0`), sinon `navigate("/")`.
- Focusable (flèche haut depuis les actions de la fiche) ; Retour / Échap
  continue d'appeler la même logique via `setBackFallback`.

## 8. Tests

**Rust (unitaires)**
- normalisation et regroupement : casse, accents, espaces, détachement,
  utilisateurs découverts sans connexion ;
- fusion / séparation des réglages : `settings_set` → `settings_get`
  aller-retour, champ personnel rangé dans le profil, champ matériel dans
  `settings.json` ;
- migration : pas de `profiles.json` → désactivé, profil implicite ;
- `profiles.json` corrompu → défaut + copie ;
- PIN : hash / vérification, temporisation ;
- `profile_switch` charge exactement les connexions attendues dans le
  `Catalog` ;
- verrous : désactivation et changement de mode refusés sans PIN valide.

**Providers** : parsing de `home/users` (Plex) et `Users/Public` (Jellyfin)
sur des fixtures JSON ; tests `live` optionnels comme les existants.

**UI** : vitest sur les helpers purs (ordre des tuiles, libellés de
provenance). Vérification visuelle dans l'app lancée : sélecteur, popover,
PIN, échec partiel, mouvement réduit, bouton retour.

**Bindings** : régénération `ts-rs` des nouveaux types.

## 9. Fichiers principaux

| Zone | Fichiers |
|---|---|
| Modèle | `crates/core/src/profile.rs` (nouveau), `crates/core/src/settings.rs` (`PersonalSettings`, fusion/séparation) |
| Stockage | `crates/storage/src/lib.rs` (`profiles()` / `save_profiles()`), `crates/storage/src/profiles.rs` (regroupement, PIN) |
| Providers | `crates/providers/plex/src/auth.rs` (home users, switch), `crates/providers/jellyfin/src/auth.rs` (`Users/Public`) |
| App | `app/src/state.rs` (profil actif, catalogue), `app/src/commands/profiles.rs` (nouveau), `app/src/commands/system.rs` (réglages effectifs), `app/src/main.rs` (enregistrement) |
| UI | `ui/src/features/profiles/*` (nouveau), `ui/src/shell/Sidebar.tsx`, `ui/src/shell/TabBar.tsx`, `ui/src/features/settings/Settings.tsx`, `ui/src/App.tsx`, `ui/src/ipc/api.ts`, `ui/src/components/tv/BackButton.tsx` |
| Docs | `ARCHITECTURE.md` (§10, §7), `docs/DESIGN_SYSTEM.md` (tuiles profil, pavé PIN, BackButton) |
