# macOS — présentation vidéo sous la WKWebView — Design

> Date : 2026-09-28 · Statut : validé en discussion, en relecture
> Sous-projet 4.1 du portage macOS (`docs/MACOS_PORT.md`). Premier sous-projet
> traité : c'est le plus structurant, il conditionne la fenêtre transparente
> (4.2) et une partie des ajustements UI (4.5).

## 0. Intention

Flick sur macOS doit afficher la vidéo **sous** l'UI React du lecteur (pas de
fenêtre mpv séparée), comme sous Windows : mini-lecteur, PiP, Flick Frame, HDR
sur écran EDR/XDR. Aujourd'hui, `crates/player/src/presenter/mod.rs::choose`
retombe sur `DedicatedWindow` hors Windows — une fenêtre mpv à part, sans l'UI
du lecteur.

Hors périmètre (sous-projets suivants du portage) :
- sondes de capacités complètes (4.3) — ce sous-projet n'ajoute que le minimum
  nécessaire pour activer/désactiver l'EDR ;
- libmpv universelle livrée en distribution (4.4) — le dev via Homebrew
  (`brew install mpv`, arm64 seulement) suffit ici ;
- plein écran, PiP, menu d'app, clavier (4.5) ;
- packaging, signature ad hoc du binaire final (4.6).

## 1. Recherche préalable et décision retenue

`docs/MACOS_PORT.md` posait deux pistes : **A.** l'API de rendu OpenGL de mpv
dans un `CAOpenGLLayer` sous la `WKWebView` (approche IINA, déjà notée
`LayerRender` dans ARCHITECTURE.md §4.2, statut 🟡) ; **B.** un `CAMetalLayer`,
en citant « Jellium Desktop » (client Jellyfin non officiel, Rust + CEF + mpv)
comme précédent.

Vérification faite avant de trancher :
- `--wid` n'est pas une voie d'embarquement viable sous macOS (aucune
  application mpv en production ne l'utilise ainsi ; confirme ARCHITECTURE.md).
- IINA utilise aujourd'hui, en production, l'API de rendu mpv (OpenGL) dans un
  `CAOpenGLLayer` (`ViewLayer.swift`, `class ViewLayer: CAOpenGLLayer`), avec
  `wantsExtendedDynamicRangeContent = true` pour l'EDR. C'est vivant, maintenu,
  et le HDR y fonctionne réellement.
- L'API de rendu mpv (`render.h`) n'expose **aucun backend Metal natif** :
  seulement OpenGL, logiciel (SW), et un Vulkan encore expérimental et non
  mainliné.
- **« Jellium Desktop » ne fait pas ce que le dossier de passation supposait.**
  Lecture de son code (`andrewrabert/jellium-desktop`) : son `CAMetalLayer` sert
  uniquement à empiler les surfaces de **CEF** (l'UI du navigateur), pas mpv.
  Son commentaire de code est explicite : « macOS / Windows: mpv owns its
  window outright » — mpv a sa **propre fenêtre séparée** sur macOS, comme la
  stratégie `DedicatedWindow` de Flick sous Linux. Il n'y a donc aucun
  précédent vérifié pour l'option B telle que décrite ; le dossier de
  passation contenait une erreur sur ce point, à corriger (§6).

**Décision : option A, `CAOpenGLLayer` via l'API de rendu OpenGL de mpv.**
C'est la seule option avec un précédent technique réel et actuel aujourd'hui,
et elle confirme la stratégie `LayerRender` déjà notée dans ARCHITECTURE.md.

## 2. Composants

- **`crates/mpv`** : ajouter les liaisons de l'API de rendu (`render.h`) à
  côté de l'API client existante dans `sys.rs` — `mpv_render_context_create`,
  `_render`, `_set_update_callback`, `_report_swap`, `_free`, la structure
  `mpv_render_param`, et la résolution de fonctions OpenGL
  (`get_proc_address`). Aucune liaison de ce type n'existe aujourd'hui.
- **`crates/player/src/presenter/mod.rs`** :
  - `HostWindow` gagne une variante `AppKit { ns_view: *mut c_void }` (le
    pointeur `NSView*` de la vue hébergeant la WebView, fourni par Tauri) —
    aujourd'hui seul `Win32` existe, tout le reste retombe sur `Other`.
  - `PresenterKind` gagne `LayerRender`.
  - `choose()` route vers le nouveau presenter macOS quand `host` est
    `HostWindow::AppKit`.
- **`crates/player/src/presenter/macos.rs`** (nouveau fichier, miroir de
  `windows.rs`) : `LayerPresenter`, qui possède le `CAOpenGLLayer`, crée le
  contexte de rendu mpv en mode OpenGL, l'ajoute comme sous-calque de la vue
  racine, et implémente `set_viewport`/`set_visible`/`observed`/`on_property`.
- **`app/src/main.rs`** : `host_window()` retourne `HostWindow::AppKit` sur
  macOS (au lieu de `Other`), via `ns_view()` de Tauri.

## 3. Threading

L'API de rendu mpv exige un contexte OpenGL courant sur le thread qui appelle
`mpv_render_context_render`, et un callback de mise à jour
(`mpv_render_context_set_update_callback`) invocable depuis n'importe quel
thread mpv pour signaler qu'une frame est prête.

- Un `NSOpenGLContext` dédié au rendu vidéo (distinct de celui de l'UI, s'il y
  en a un).
- Le callback de mise à jour ne fait qu'un travail minimal (poser un drapeau /
  réveiller le calque) : il est appelé depuis le thread de décodage mpv, donc
  pas de travail lourd dedans.
- Le rendu (`mpv_render_context_render` + `CAOpenGLLayer.canDrawInCGLContext`)
  tourne sur le thread que CoreAnimation choisit pour ce calque — modèle
  standard `CAOpenGLLayer`, pas un thread géré par nous.
- Toute mutation de la hiérarchie de vues/calques (ajout du `CAOpenGLLayer`,
  changement de géométrie) passe par `UiDispatch` (comme sous Windows), car
  elle doit se faire sur le thread principal AppKit.

## 4. Coordonnées (`set_viewport`)

`Viewport` reste en pixels physiques depuis le coin haut-gauche (contrat déjà
fixé par le trait `Presenter`, inchangé). AppKit compte en points depuis le
coin bas-gauche. Le presenter macOS convertit à la frontière :

```
y_appkit = hauteur_vue_points - (viewport.y + viewport.height) / backingScaleFactor
largeur_points  = viewport.width  / backingScaleFactor
hauteur_points  = viewport.height / backingScaleFactor
```

Le `CAOpenGLLayer` a son `contentsScale` réglé sur `backingScaleFactor` pour
rester net sur écran Retina.

## 5. HDR minimal

Pas de `crates/capabilities/src/macos/` complet ici (réservé au sous-projet
4.3). Seul le minimum nécessaire pour piloter
`target-colorspace-hint`/`target-trc=pq`/`target-peak` côté
`crates/player/src/options.rs` (même point d'entrée que sous Windows) :

- une fonction `macos::edr_headroom(ns_screen) -> Option<f64>` basée sur
  `maximumPotentialExtendedDynamicRangeColorComponentValue` /
  `maximumExtendedDynamicRangeColorComponentValue` de `NSScreen` ;
- appelée au changement d'écran de la fenêtre (déjà branché sur
  `ScaleFactorChanged` dans `app/src/main.rs`) ;
- le `CAOpenGLLayer` active `wantsExtendedDynamicRangeContent` et
  `wantsExtendedDynamicRangeOpenGLSurface` dès que l'écran est capable,
  indépendamment de la lecture en cours (comme IINA).

## 6. Erreurs et repli

- Échec de `mpv_render_context_create` (pilote GL absent, VM sans
  accélération…) : log + repli sur `DedicatedWindow` — pas de crash silencieux.
- `macOSPrivateApi` non actif, ou `CAOpenGLLayer` refusé par la fenêtre : même
  repli.
- `PresenterChoice::DedicatedWindow` en Réglages › Avancé reste le forçage
  manuel déjà prévu par le contrat, inchangé.
- Pas de repli logiciel (SW render) : sans accélération GPU, une fenêtre
  séparée vaut mieux qu'une dégradation silencieuse des perfs vidéo.

## 7. Validation attendue

Reprend le protocole du spike Windows (ARCHITECTURE.md §4.3) et §5 du dossier
de passation :

1. **Spike SDR** : vidéo sous une `WKWebView` transparente avec un bloc
   opaque, un dégradé et un panneau à 45 % d'opacité par-dessus — valide le
   mélange alpha avant d'aller plus loin.
2. **HDR10 sur écran EDR/XDR réel** : PQ affiché ; comparé à un écran SDR
   (tone mapping attendu).
3. **Géométrie** : redimensionnement/déplacement du viewport (mini-lecteur),
   pas de déchirement ni de décalage.
4. **Repli** : forcer `DedicatedWindow` en réglages, vérifier que le chemin
   existant marche toujours (non-régression).
5. **Windows** : `pnpm test`, `pnpm lint`, `pnpm build:win` toujours verts.

## 8. Docs à corriger à la fin

- `docs/MACOS_PORT.md` §4.1 : retirer l'option B ou la corriger — Jellium
  Desktop n'y est pas un précédent valable (mpv y a sa propre fenêtre, ni
  `CAMetalLayer` partagé, ni rendu mpv dedans).
- `ARCHITECTURE.md` §4.2 : faire passer `LayerRender` de 🟡 conçu à ✅ validé
  une fois le spike §7.1 confirmé sur machine réelle.
