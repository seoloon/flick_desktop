# Portage macOS — dossier de passation

> Pour la session Claude Code qui tourne **sur le Mac**. Ce document dit ce qui
> est décidé, ce qui existe, ce qui manque et où regarder. Il ne remplace pas la
> conception : pour un chantier qui n'a pas encore de spec, commence par
> `superpowers:brainstorming`, puis spec → plan → exécution, comme pour les
> fonctionnalités précédentes. Rédigé le 2026-09-27 depuis la machine Windows,
> sans avoir pu compiler sur macOS : tout ce qui est encore marqué *à vérifier*
> n'a jamais tourné sur une vraie machine.
>
> **Mise à jour du 2026-09-28**, depuis le Mac : le chantier 4.1 (présentation
> vidéo sous la WKWebView) a sa conception, son plan d'implémentation et son
> code fusionnés dans `master` — voir §4.1 pour l'état exact et ce qui reste.
> Spec : `docs/superpowers/specs/2026-09-28-macos-video-presenter-design.md`.
> Plan : `docs/superpowers/plans/2026-09-28-macos-video-presenter.md` (9 tâches,
> toutes implémentées et revues ; ledger de revue supprimé après fusion, le
> détail des décisions prises pendant l'exécution est dans l'historique git de
> la branche fusionnée, commits `d70f824..b144a6a`).

## 1. Objectif et décisions déjà prises

**Objectif** : Flick sur macOS avec **les mêmes exigences et fonctionnalités**
que la version Windows actuelle, sans régression côté Windows.

Décisions de l'utilisateur (ne pas les rediscuter) :

| Sujet | Décision |
|---|---|
| Façon de travailler | Claude Code tourne sur le Mac, compile et lance lui-même ; l'utilisateur juge le rendu (vidéo, HDR, animations). |
| Licence | L'app est **GPL v3** (`LICENSE`, dépôt `seoloon/flick_desktop`). Une libmpv **GPL** est donc acceptable. |
| Signature | **Pas de compte Apple Developer.** Signature *ad hoc* (`signingIdentity: "-"`), DMG non notarisé. Premier lancement sur un autre Mac : clic droit › Ouvrir, ou Réglages › Confidentialité et sécurité › Ouvrir quand même. |
| Build | `pnpm build:mac` produit une app **Apple silicon** (arm64) ; l'universel est reporté. |

Limites acceptées d'avance (documentées dans ARCHITECTURE.md §6 et §14) :
- pas de bitstream TrueHD / DTS-HD / Atmos TrueHD (CoreAudio n'a pas de HBR) ;
- pas de Mac App Store (fenêtre transparente = API privée) ;
- Dolby Vision : reshaping comme sous Windows, pas de vrai DV (un backend
  AVFoundation est une idée future, hors périmètre).

« Mêmes fonctionnalités » veut dire en particulier : l'interface React du
lecteur **par-dessus** la vidéo (pas de fenêtre mpv séparée), le PiP, Flick
Frame en plein écran avec l'intro, le HDR sur les écrans EDR/XDR, les profils
avec PIN, les favoris, les pages personnes TMDB, la clé TMDB dans le Trousseau.

## 2. Démarrer sur le Mac

```sh
xcode-select --install                       # outils de compilation
rustup target add aarch64-apple-darwin x86_64-apple-darwin
brew install mpv                             # libmpv de développement (arm64 seulement)
pnpm install
pnpm desktop                                 # tauri dev
pnpm test                                    # cargo test + vitest + tsc
pnpm lint                                    # clippy (0 avertissement exigé) + tsc
pnpm build:mac                               # app arm64 + DMG (ad hoc), libmpv embarquée
```

Ajouté depuis Windows, **à vérifier sur le Mac** :
- `package.json` : `build:win` (`--target x86_64-pc-windows-msvc`) et
  `build:mac` (`--target universal-apple-darwin`).
- `app/tauri.windows.conf.json` : la DLL libmpv n'est plus dans la config
  commune, sinon le build Mac échouait sur un fichier absent.
- `app/tauri.macos.conf.json` : `bundle.macOS.signingIdentity: "-"` (ad hoc).

Conventions du dépôt (voir aussi `ARCHITECTURE.md`, `docs/DESIGN_SYSTEM.md`) :
- Commentaires de code en anglais, docs en français. Style : phrases complètes,
  densité de commentaires comme le code voisin.
- Commits **signés GPG** ; ne jamais contourner la signature. Si elle échoue,
  préparer le commit et demander à l'utilisateur.
- Ne pas committer le `app/Cargo.toml` modifié localement par l'utilisateur
  (s'il l'est encore).
- Les docs publiques sont en anglais : `README.md` (vitrine) et `TECHNICAL.md`
  (build, commandes, organisation).
- Rust : pas de verrou `parking_lot` tenu à travers un `.await` ; une commande
  Tauri `async` qui prend un `State` doit renvoyer `Result`.
- Pas de formateur configuré côté UI : lignes longues (~180-200 colonnes).
  Si tu passes Prettier, `--print-width 200`, et vérifie le diff.
- TDD pour la logique testable ; le natif (couches, rendu) se valide à l'écran
  avec l'utilisateur.

## 3. Ce qui est déjà portable

| Partie | Où | État macOS |
|---|---|---|
| UI React, Tailwind, navigation | `ui/` | Détection Mac : `ui/src/shell/TitleBar.tsx` (`isMac`, `data-platform="mac"`), feux tricolores via `tauri.macos.conf.json` (`titleBarStyle: Overlay`, `trafficLightPosition`). SF Pro prévue. |
| Jellyfin, Plex, TMDB, catalogue, réseau | `crates/providers/*`, `crates/catalog`, `crates/net` | Rust pur. |
| Profils, PIN (Argon2id), réglages, cache | `crates/storage` | Rust pur. |
| Secrets (tokens, clé TMDB) | `crates/storage/src/secrets.rs` (`keyring` 4) | Le Trousseau macOS est déjà dans le `Cargo.lock` (`apple-native-keyring-store`). *À vérifier* : une invite d'accès au Trousseau peut apparaître à chaque build ad hoc (signature différente). |
| Protocole d'images `oneshot-img` | `app/src/images.rs`, `ui/src/ipc/images.ts` | L'UI utilise déjà `oneshot-img://localhost/` hors Windows ; la CSP (`tauri.conf.json`) l'autorise. |
| Chargement de libmpv | `crates/mpv/src/sys.rs` (noms `libmpv.2.dylib`, `libmpv.dylib`), `app/src/main.rs` `libmpv_dirs` | **Fait** pour le dev : `/opt/homebrew/lib` et `/usr/local/lib` cherchés en debug sur macOS. Distribution : libmpv livrée dans l'app, arm64 seulement (`tools/bundle-libmpv-macos.mjs` → `third_party/mpv/macos-arm64/`, embarquée en ressource `libmpv/`). Reste l'Intel/universel. |
| Décision de lecture | `crates/playback` | Portable ; reste prudente tant que les capacités sont « inconnues ». |

## 4. Chantiers, du plus lourd au plus léger

### 4.1 Présenter la vidéo sous la WKWebView (le cœur)

**État : fait et fusionné dans `master`** (commits `d70f824..b144a6a`, plan
`docs/superpowers/plans/2026-09-28-macos-video-presenter.md`, 9 tâches). Reste
une validation à l'écran et deux trous connus — voir « Ce qui reste » plus bas.

**Décision tranchée** (la question « pistes à trancher » ci-dessous n'est plus
ouverte) : **option A, `CAOpenGLLayer` via l'API de rendu OpenGL de mpv**,
comme IINA. L'option B (`CAMetalLayer`) a été écartée après vérification :
« Jellium Desktop » (`andrewrabert/jellium-desktop`) n'est *pas* un précédent
valable pour elle — son `CAMetalLayer` n'empile que les surfaces de **CEF**
(l'UI du navigateur), pas mpv ; son code dit explicitement que mpv y possède
sa **propre fenêtre séparée** sur macOS, comme la stratégie `DedicatedWindow`
de Flick sous Linux. L'API de rendu mpv n'a d'ailleurs pas de backend Metal
natif (seulement OpenGL, logiciel, et un Vulkan encore expérimental non
mainliné). Le détail de cette vérification est dans la spec, §1 :
`docs/superpowers/specs/2026-09-28-macos-video-presenter-design.md`.

**Ce qui a été construit** :
- `crates/mpv/src/sys.rs` : liaisons FFI de l'API de rendu mpv
  (`mpv_render_context_create`, `_render`, `_set_update_callback`,
  `_report_swap`, `_free`, les structures `mpv_render_param`/
  `mpv_opengl_init_params`/`mpv_opengl_fbo`).
- `crates/mpv/src/render.rs` : `RenderContext`, wrapper sûr au-dessus de ces
  liaisons (`create_opengl`, `render`, `report_swap`, `set_update_callback`,
  `Drop` qui libère le contexte).
- `crates/player/src/presenter/mod.rs` : `HostWindow::AppKit { ns_view }`,
  `PresenterKind::LayerRender`, hook `Presenter::on_mpv_ready` (appelé juste
  après `mpv_initialize`, c'est lui qui crée le contexte de rendu côté
  macOS).
- `crates/player/src/presenter/macos.rs` (nouveau, ~870 lignes) :
  `LayerPresenter`. Sous-classe `CAOpenGLLayer` via `objc2::define_class!`
  (choix du pixel format/contexte CGL avec repli 3 niveaux, dessin en
  appelant `RenderContext::render`/`report_swap`), registre `thread_local!`
  (`LAYERS`, sur le thread principal) plutôt qu'un champ `self.layer` — évite
  le piège Send/Sync qu'un champ directement dans `LayerPresenter` aurait
  posé. Un thread dédié `mpv-display` réveillé par le callback de mise à jour
  mpv appelle `-display` puis `CATransaction::flush` (le patron d'IINA et du
  backend `cocoa-cb` de mpv, pas du polling `canDrawInCGLContext:`). Conversion
  de coordonnées `appkit_rect` : `Viewport` (pixels physiques, haut-gauche)
  → points AppKit (bas-gauche), avec `backingScaleFactor`. Le flip d'image
  (mpv rend à l'envers dans le repère CoreAnimation) se fait par une
  transformation de calque, pas par `MPV_RENDER_PARAM_FLIP_Y` (que
  `RenderContext` n'expose pas — amélioration future possible côté
  `crates/mpv`).
- `crates/capabilities/src/macos/mod.rs` : sonde EDR minimale (`NSScreen`),
  pas les sondes audio/décodeur complètes (ça reste 4.3). Voir HDR plus bas.
- `app/src/main.rs` : `host_window()` retourne `HostWindow::AppKit` via
  `window.ns_view()` de Tauri ; `libmpv_dirs()` cherche `/opt/homebrew/lib`
  et `/usr/local/lib` en debug sur macOS.
- `app/tauri.conf.json` : `macOSPrivateApi: true` (dans la config **de base**,
  pas dans l'overlay `tauri.macos.conf.json` — un overlay ne suffit pas
  puisque `app/Cargo.toml` active la feature Cargo `macos-private-api` sans
  condition de plateforme ; la vérification par tauri-build compare
  fonctionnalité Cargo et config *fusionnée pour la cible*, donc il fallait
  que le flag soit présent pour toutes les cibles). `app/Cargo.toml` :
  `tauri = { features = ["macos-private-api"] }`, sans gate `cfg` — vérifié
  sans danger sur Windows/Linux car la feature ne fait qu'activer du code déjà
  gaté en interne par `#[cfg(any(not(target_os = "macos"), feature =
  "macos-private-api"))]` côté tauri/wry.

**HDR — état honnête** : la sonde `crates/capabilities/src/macos/mod.rs`
détecte la capacité EDR réelle de l'écran (`NSScreen.maximumPotentialExtended…`)
mais **rétrograde volontairement tout résultat `Active` en `SupportedButOff`**
avant de le renvoyer. Raison : le `CAOpenGLLayer` actuel a un pixel format CGL
entier 8 bits, sans espace de couleur PQ posé sur le calque — si la sonde
annonçait `Active`, le pipeline enverrait `target-trc=pq`/`target-prim=bt.2020`
à mpv pour un calque qui ne peut pas les afficher correctement (couleurs
fausses/délavées sur un vrai écran EDR, pas juste « pas de HDR »). Tant que ce
correctif restera en place, tout contenu HDR sera donc tone-mappé en SDR par
mpv sur macOS — correct et sûr, mais aucun vrai HDR tant que le calque n'est
pas géré en couleur (voir « Ce qui reste »).

**Vérifications faites sans écran réel** (revue de code, `cargo check
--workspace`, `cargo test`, `pnpm test`/`pnpm lint`, tests réels contre
Homebrew libmpv 0.41 avec `--ignored`) : compilation propre sur les trois
plateformes (vérifié par lecture directe du code source de
tauri-build/tauri-utils pour Windows/Linux, faute de pouvoir y compiler ici),
libmpv se charge, `RenderContext::create_opengl` échoue proprement (repli
`DedicatedWindow`) mais réussit dans le harnais de test avec un vrai contexte
GL. **Jamais lancé dans l'app réelle contre une vraie `WKWebView`** — c'est le
tout premier test à faire, voir « Ce qui reste ».

**Ce qui reste** (dans l'ordre où ça bloque le suivant) :

1. **Lancer l'app et regarder** (`pnpm desktop`). Chercher dans les logs
   `starting mpv engine kind=LayerRender`. Vérifier que la vidéo apparaît
   sous l'UI, pas dans une fenêtre à part.
2. **Spike SDR** : bloc opaque, dégradé, panneau à 45 % d'opacité par-dessus
   la vidéo (même protocole que le spike Windows, ARCHITECTURE.md §4.3).
3. **Repli vers `DedicatedWindow` non implémenté** : si
   `RenderContext::create_opengl` échoue, `on_mpv_ready` se contente de logguer
   et de ne rien afficher (audio sans vidéo) — il ne bascule pas sur
   `DedicatedWindow` comme le prévoyait la spec §6. Le corriger demande de
   faire retourner `Result` à `Presenter::on_mpv_ready`, de faire remonter une
   erreur distincte depuis `Engine::start`, et de faire réessayer
   `ensure_engine` avec `DedicatedWindow` — un vrai changement d'architecture,
   volontairement laissé de côté lors de la revue finale plutôt que précipité
   dans la dernière vague de correctifs.
4. ~~Décalage d'identifiant d'écran pour le HDR multi-écrans~~ **corrigé** :
   `screens()` identifie maintenant les écrans comme `tao` (`Monitor #<numéro de
   modèle CGDisplay>`, via `NSScreenNumber`), ce qui est ce que
   `current_display` renvoie. Limite : deux écrans du même modèle partagent
   l'identifiant (`tao` ne fait pas mieux).
5. **HDR10 réel sur un écran EDR/XDR** : une fois qu'un vrai calque géré en
   couleur existe (espace colorimétrique PQ posé sur le `CAOpenGLLayer`,
   pixel format flottant), retirer la rétrogradation `Active → SupportedButOff`
   décrite plus haut et valider sur un vrai écran XDR/EDR.
6. Le reste du protocole de validation (§5 plus bas) : sous-titres, pistes
   audio, Flick Frame, profils/PIN, favoris — rien de spécifique à ce chantier
   ne les bloque a priori, mais rien n'a été testé non plus.
7. Mettre à jour ARCHITECTURE.md §4.2 : faire passer `LayerRender` de 🟡 conçu
   à ✅ validé une fois les points 1 et 2 confirmés sur machine réelle.

**Repères utiles pour reprendre** :
- Contrat `Presenter` (trait) dans `crates/player/src/presenter/mod.rs` —
  inchangé dans sa forme, `on_mpv_ready` est le seul ajout depuis le portage
  Windows.
- Référence Windows à relire si besoin de comparaison : `windows.rs` dans le
  même dossier (swapchain mpv dans un visuel DirectComposition derrière la
  WebView2).
- `PresenterChoice` (`crates/core/src/settings.rs`, Réglages › Avancé) :
  `DedicatedWindow` force déjà le repli manuel sur macOS, inchangé.

### 4.2 Fenêtre transparente

**État : fait**, livré avec 4.1 plutôt qu'en chantier séparé (nécessaire pour
voir quoi que ce soit sous la WebView, donc pas séparable en pratique).

- `"macOSPrivateApi": true` est dans `app/tauri.conf.json` (config **de
  base**, pas l'overlay `tauri.macos.conf.json` — voir §4.1 pour pourquoi :
  la feature Cargo est sans condition de plateforme, donc la config doit
  l'être aussi pour que la vérification de `tauri-build` passe sur toutes
  les cibles).
- `app/Cargo.toml` : `tauri = { features = ["macos-private-api"] }`, sans
  gate `cfg`. Vérifié sans danger sur Windows/Linux (la feature n'active que
  du code déjà gaté par `#[cfg(any(not(target_os = "macos"), feature =
  "macos-private-api"))]` côté tauri/wry — donc un no-op ailleurs).
- Build Windows toujours vert : vérifié par lecture directe du code source
  de `tauri-build`/`tauri-utils` (pas de build Windows réel possible depuis
  ce Mac) ; à reconfirmer avec un vrai `pnpm build:win` dès qu'une machine
  Windows est disponible.

### 4.3 Sondes de capacités macOS

**État : minimal seulement**, livré avec 4.1 pour débloquer le HDR (voir §4.1)
— pas le chantier complet.

`crates/capabilities/src/macos/mod.rs` existe mais ne couvre que l'écran
(`NSScreen`, capacité EDR) : `hdr_state_from_edr_headroom` (pure, testée) et
`screens()` (lit `NSScreen` hors du thread principal via un
`MainThreadMarker::new_unchecked()` documenté — les lectures de propriétés
`NSScreen` ne sont pas garanties thread-safe par Apple mais le sont en
pratique, `CapabilityManager::refresh()` tournant sur un thread d'arrière-plan
par conception). Le résultat HDR est actuellement toujours rétrogradé en
`SupportedButOff` avant de sortir de `probe()` — voir §4.1, HDR.

**Vidéo : fait** (`crates/capabilities/src/macos/video.rs`,
`VTIsHardwareDecodeSupported` par codec : H.264, HEVC, AV1, VP9 ; HEVC Main10
supposé sur Apple silicon, non déclaré sur Intel).

**Audio : fait** (`crates/capabilities/src/macos/audio.rs`, CoreAudio via
`objc2-core-audio`) : périphériques ayant des flux de sortie, UID (nom mpv
`coreaudio/<UID>`), type de connexion, canaux, fréquence nominale, périphérique
par défaut, et **passthrough** : mêmes critères que `ao_coreaudio_exclusive` de mpv
(un flux dont les formats physiques incluent `ac-3`/`cac3` → {AC3, DTS} ; un format
à 192 kHz en plus → E-AC3 ; jamais TrueHD/DTS-HD/Atmos, pas de HBR). Sur les
haut-parleurs du MacBook la sonde dit `{}` et mpv refuse pareil (« No usable
substream found »). Le cas positif (AVR/TV en HDMI, DAC optique) n'a **pas** pu être
lu sur du matériel : voir `docs/PLAYBACK_VALIDATION.md`, section macOS.

`crates/capabilities/src/windows/` (≈ 600 lignes : `display.rs`, `audio.rs`,
`video.rs`) reste le modèle à suivre pour le reste du chantier. Modèle à
remplir : `crates/core/src/capabilities.rs` (`DisplayCapabilities`,
`AudioCapabilities`, `VideoCapabilities`, `HdrState`). Règle d'honnêteté
(déjà respectée par le code existant, à garder) : une sonde qui ne peut pas
s'exécuter renvoie `Unknown` avec une raison, jamais une valeur optimiste.

À couvrir (via `objc2` / frameworks, isolé dans un module `macos/` comme
prévu par ARCHITECTURE.md §7.1) :
- **Écrans** : `NSScreen` (EDR : `maximumPotentialExtendedDynamicRangeColorComponentValue`,
  `maximumExtendedDynamicRangeColorComponentValue`), `CGDisplay` (id, bornes,
  rafraîchissement). Mapper vers `HdrState` (capable / actif).
- **Audio** : CoreAudio (périphériques, périphérique par défaut, canaux). Pas
  de bitstream HBR ; AC3/E-AC3 en S/PDIF selon le périphérique, à vérifier.
- **Vidéo** : VideoToolbox (`VTIsHardwareDecodeSupported` par codec : H.264,
  HEVC, HEVC Main10, AV1 sur M3+, VP9). hwdec mpv = `videotoolbox`.
- **GPU** : Metal (`MTLCopyAllDevices`) ou IOKit pour `platform_gpus`.
- Rafraîchissement quand la fenêtre change d'écran (déjà branché sur
  `ScaleFactorChanged` dans `app/src/main.rs`).

### 4.4 libmpv livrée dans l'app

**État : fait pour arm64** (`tools/bundle-libmpv-macos.mjs` copie la libmpv de
Homebrew et ses dépendances dans `third_party/mpv/macos-arm64/`, relocalisées en
`@loader_path` et re-signées ad hoc ; livrée en ressource `libmpv/`, cherchée par
`libmpv_dirs` dans `app/src/main.rs`).

**Piège du hardened runtime** (découvert au premier essai du DMG, invisible en
dev) : Tauri signe le bundle avec le flag `runtime`. Sans entitlements, macOS
refuse alors de charger les dylibs ad hoc (« different Team IDs »), puis tue le
process quand LuaJIT (scripts Lua intégrés de mpv) écrit du code machine
(`SIGKILL Code Signature Invalid`). `app/entitlements.macos.plist`, référencé par
`bundle.macOS.entitlements`, pose `disable-library-validation`, `allow-jit` et
`allow-unsigned-executable-memory`. Vérifié : lecture lancée depuis le DMG sur
deux Mac Apple silicon. Un test sans écran reste possible : signer un petit
binaire en `-o runtime` avec ces entitlements et lui faire `dlopen` la libmpv
du bundle, puis `mpv_initialize`.

**Reste** :
- Intel / universel : `lipo` arm64 + x86_64 avec toutes les dépendances
  (`third_party/mpv/macos-universal/`), et `build:mac` en `universal-apple-darwin`.
- `third_party/mpv/README.md` dit encore « livrer une build LGPL pour ne pas
  être GPL » : obsolète depuis le passage en GPL v3, à réécrire.

### 4.5 Ajustements UI et fenêtre

- **Plein écran** : Flick Frame utilise `set_fullscreen`
  (`app/src/commands/system.rs`) ; sous macOS c'est un Space animé (~0,7 s).
  L'intro attend que la page ait la taille de l'écran (`fullscreenSized`,
  plafond 1,5 s, `ui/src/lib/intro.ts`) : vérifier que ça suffit, sinon
  envisager `set_simple_fullscreen`. `html.frame` dimensionne l'UI en `vw`
  (`ui/src/index.css`).
- **PiP** (`window_pip`, même fichier) : `always_on_top`, taille calculée sur la
  `work_area` du moniteur. À vérifier avec les Spaces et le plein écran.
- **Menu d'app** : aucun menu n'est défini (`app/src/main.rs`). Sous macOS,
  sans menu Édition, Cmd+C / Cmd+V ne marchent pas dans les champs (clé TMDB,
  PIN, adresse serveur). Vérifier le menu par défaut de Tauri 2, et Cmd+Q /
  Cmd+W / Cmd+,.
- **Clavier** : `ui/src/nav/input.ts` ignore les touches avec Ctrl/Cmd/Alt.
  Vérifier que rien ne dépend de Ctrl, et la touche Menu ou son équivalent pour
  Flick Frame.
- **Manette** : Gamepad API dans WKWebView, *à vérifier* (focus fenêtre
  requis).
- **Intro Remotion** et animations Motion dans WebKit : *à vérifier*
  (performances, `backdrop-filter`).
- **Barre de titre** : `docs/DESIGN_SYSTEM.md` (section fenêtre) décrit la
  bande de déplacement Mac ; la capsule réduire/agrandir/fermer n'est affichée
  que hors Mac.

### 4.6 Packaging

- `pnpm build:mac` → `target/universal-apple-darwin/release/bundle/` (`.app`,
  `.dmg`). Icône `icons/icon.icns` déjà présente.
- Ad hoc : vérifier que Tauri signe bien le binaire universel **et** les dylib
  embarquées (`codesign -dv --verbose=4`, `codesign --verify --deep`).
- `pnpm build:mac` produit pour l'instant l'arm64 seul (`target/aarch64-apple-darwin/`).
- Tester le DMG sur un Mac « propre » : quarantaine Gatekeeper, lancement,
  Trousseau.

## 5. Validation attendue (à la manière de docs/PLAYBACK_VALIDATION.md)

1. SDR 1080p/4K : lecture fluide, UI du lecteur par-dessus, mini lecteur, PiP.
2. Bloc opaque, dégradé, panneau translucide par-dessus la vidéo (comme §4.3).
3. HDR10 sur écran EDR/XDR : PQ affiché, panneau Avancé cohérent ;
   sur écran SDR : tone mapping.
4. Sous-titres ASS et PGS, changement de piste audio, 5.1 PCM.
5. Flick Frame : entrée avec intro sans saut, sortie, 85 % d'échelle.
6. Profils (sélecteur, PIN), favoris Jellyfin et Plex, pages personnes TMDB
   (clé stockée dans le Trousseau, jamais envoyée à la WebView).
7. Windows : `pnpm test`, `pnpm lint` et `pnpm build:win` toujours verts.

## 6. Docs à mettre à jour à la fin

- `ARCHITECTURE.md` : §4.2 (statut de la stratégie macOS), §7.1, §14, §16.
- `docs/PLAYBACK_VALIDATION.md` : section macOS **faite** (M1, haut-parleurs) ; à compléter avec un écran EDR/XDR et un ampli.
- `README.md` : prérequis et commandes macOS (`build:mac`, Homebrew).
- `third_party/mpv/README.md` : licence et build macOS.
