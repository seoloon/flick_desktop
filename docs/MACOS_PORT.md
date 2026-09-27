# Portage macOS — dossier de passation

> Pour la session Claude Code qui tourne **sur le Mac**. Ce document dit ce qui
> est décidé, ce qui existe, ce qui manque et où regarder. Il ne remplace pas la
> conception : commence par `superpowers:brainstorming` (chantier
> *architectural*), puis spec → plan → exécution, comme pour les fonctionnalités
> précédentes. Rédigé le 2026-09-27 depuis la machine Windows, sans avoir pu
> compiler sur macOS : tout ce qui est marqué *à vérifier* n'a jamais tourné.

## 1. Objectif et décisions déjà prises

**Objectif** : Flick sur macOS avec **les mêmes exigences et fonctionnalités**
que la version Windows actuelle, sans régression côté Windows.

Décisions de l'utilisateur (ne pas les rediscuter) :

| Sujet | Décision |
|---|---|
| Façon de travailler | Claude Code tourne sur le Mac, compile et lance lui-même ; l'utilisateur juge le rendu (vidéo, HDR, animations). |
| Licence | L'app est **GPL v3** (`LICENSE`, dépôt `seoloon/flick_desktop`). Une libmpv **GPL** est donc acceptable. |
| Signature | **Pas de compte Apple Developer.** Signature *ad hoc* (`signingIdentity: "-"`), DMG non notarisé. Premier lancement sur un autre Mac : clic droit › Ouvrir, ou Réglages › Confidentialité et sécurité › Ouvrir quand même. |
| Build | `pnpm build:mac` produit une app **universelle** (arm64 + x86_64). |

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
pnpm build:mac                               # app universelle + DMG (ad hoc)
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
- Ne **jamais** committer `app/icons/basic/flick-wordmark.svg` (fichier local
  non suivi de l'utilisateur), ni le `app/Cargo.toml` modifié localement.
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
| Chargement de libmpv | `crates/mpv/src/sys.rs` (noms `libmpv.2.dylib`, `libmpv.dylib`), `app/src/main.rs` `libmpv_dirs` | En debug, seul `third_party/mpv/windows-x64` est cherché : ajouter le chemin Mac (et/ou Homebrew). |
| Décision de lecture | `crates/playback` | Portable ; reste prudente tant que les capacités sont « inconnues ». |

## 4. Chantiers, du plus lourd au plus léger

### 4.1 Présenter la vidéo sous la WKWebView (le cœur)

**État** : rien. `crates/player/src/presenter/mod.rs::choose` retombe sur
`DedicatedWindow` hors Windows (`HostWindow::Other`, voir
`app/src/main.rs::host_window`) : une fenêtre mpv à part, sans l'UI du lecteur.

**Le contrat à respecter** (trait `Presenter`, même fichier) :
- `init_options()` : options mpv avant `mpv_initialize` ;
- `observed()` / `on_property()` : propriétés mpv dont le présentateur a besoin ;
- `set_viewport(mpv, Viewport)` : rectangle vidéo en **pixels physiques**,
  relatif à la zone client, envoyé par l'UI (`player_viewport` dans
  `app/src/commands/playback.rs`, `api.playerViewport` côté UI). Le mini
  lecteur et le PiP en dépendent ;
- `set_visible(bool)`.
- `UiDispatch` (fourni par `app/src/main.rs`, `run_on_main_thread`) exécute du
  code sur le thread UI. AppKit : tout ce qui touche aux vues et couches doit y
  passer.

Référence Windows à lire en entier : `crates/player/src/presenter/windows.rs`
(swapchain mpv dans un visuel DirectComposition *derrière* la WebView2, qui est
transparente là où est la vidéo).

**Pistes à trancher en conception** (ARCHITECTURE.md §3 et §4.2) :
- **A. API de rendu libmpv (OpenGL) → `CAOpenGLLayer`** sous la `WKWebView`,
  EDR (`wantsExtendedDynamicRangeContent`, espace de couleur PQ). C'est
  l'approche d'IINA ; OpenGL est déprécié mais fonctionne. C'est la stratégie
  `LayerRender` prévue par ARCHITECTURE.md.
  - Il faut ajouter l'API de rendu à `crates/mpv` : aucune liaison n'existe
    (`mpv_render_context_create`, `_render`, `_set_update_callback`,
    `_report_swap`, `_free`, `get_proc_address` OpenGL).
  - Il faut un thread ou un callback de rendu, et gérer le contexte CGL.
- **B. `CAMetalLayer`** : le client Jellyfin Desktop (CEF + mpv, fork Rust
  « Jellium Desktop ») compose son UI au-dessus d'un `CAMetalLayer` possédé par
  mpv. Lire leur code avant de choisir : savoir comment ils obtiennent le rendu
  mpv dans ce layer (Vulkan/MoltenVK ? `--wid` sur une `NSView` ?). Vérifier
  aussi la doc mpv de la version embarquée sur `--wid` sous macOS :
  ARCHITECTURE.md affirme qu'il n'existe que pour X11, Win32 et Android.
  C'est à confirmer, ça a pu changer.
- Faire un **spike** d'abord : une vidéo SDR sous une WKWebView transparente
  avec un bloc opaque, un dégradé et un panneau à 45 % d'opacité par-dessus,
  comme le spike Windows (§4.3). Puis HDR10 sur un écran EDR.

Points déjà connus :
- `HostWindow` n'a qu'une variante `Win32` : ajouter une variante AppKit
  (`window.ns_view()` / `ns_window()` de Tauri).
- Coordonnées : AppKit compte en points, origine en bas à gauche ; `Viewport`
  est en pixels physiques depuis le haut à gauche. Tenir compte du
  `backingScaleFactor`.
- HDR : sous Windows, mpv ne connaît pas l'écran en mode composition ; c'est
  `crates/player/src/options.rs` (vers la ligne 160) qui pose
  `target-colorspace-hint`, `target-trc=pq` et `target-peak` à partir des
  capacités. Même logique possible avec la luminance EDR de l'écran.
- `backdrop-filter: blur()` ne floute pas la vidéo (la WebView ne possède pas
  ces pixels) : attendu, comme sous Windows.
- `PresenterChoice` (`crates/core/src/settings.rs`, Réglages › Avancé) :
  décider comment la nouvelle stratégie y apparaît.

### 4.2 Fenêtre transparente

Sans elle, rien ne se voit sous la WebView. macOS exige :
- `"macOSPrivateApi": true` dans `app.` de `tauri.macos.conf.json` ;
- la feature Cargo `macos-private-api` de `tauri` dans `app/Cargo.toml`.

*À vérifier* : `tauri-build` compare les features Cargo et la config. S'assurer
que le build Windows passe toujours, par exemple avec une dépendance `tauri`
ciblée `[target.'cfg(target_os = "macos")'.dependencies]` qui ajoute la
feature.

### 4.3 Sondes de capacités macOS

`crates/capabilities/src/windows/` (≈ 600 lignes : `display.rs`, `audio.rs`,
`video.rs`) n'a pas d'équivalent. Hors Windows, `probe_platform` renvoie
« inconnu » (`crates/capabilities/src/lib.rs`), donc pas de HDR, pas de
promesses audio. Modèle à remplir : `crates/core/src/capabilities.rs`
(`DisplayCapabilities`, `AudioCapabilities`, `VideoCapabilities`, `HdrState`).
Règle d'honnêteté : une sonde qui ne peut pas s'exécuter renvoie `Unknown` avec
une raison, jamais une valeur optimiste.

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

### 4.4 libmpv livrée dans l'app, universelle

- Dev : `brew install mpv` suffit (arm64 seulement, chemins absolus vers
  `/opt/homebrew`, donc non relocalisable). `tools/ensure-libmpv.mjs` se
  contente de vérifier Homebrew sous macOS.
- Distribution : il faut une `libmpv.2.dylib` **universelle** (lipo arm64 +
  x86_64) avec ses dépendances (FFmpeg, libplacebo, libass…), des
  `install_name` en `@loader_path`/`@rpath`, et chaque dylib signée ad hoc.
  Emplacement prévu : `third_party/mpv/macos-universal/`
  (`third_party/mpv/README.md`), livrée via
  `tauri.macos.conf.json` › `bundle.resources` (le code cherche
  `resource_dir()/libmpv`) ou `bundle.macOS.frameworks`.
- Source à choisir : build maison (scripts mpv / `mpv-build`), ou binaires
  d'un projet existant (IINA, builds CI mpv…). La GPL est acceptée.
- `third_party/mpv/README.md` dit encore « livrer une build LGPL pour ne pas
  être GPL » : c'est obsolète depuis le passage en GPL v3, à réécrire.
- Étendre `tools/ensure-libmpv.mjs` pour récupérer ou produire la dylib, comme
  `tools/fetch-libmpv.ps1` sous Windows.

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
- `docs/PLAYBACK_VALIDATION.md` : section macOS avec le matériel testé.
- `README.md` : prérequis et commandes macOS (`build:mac`, Homebrew).
- `third_party/mpv/README.md` : licence et build macOS.
