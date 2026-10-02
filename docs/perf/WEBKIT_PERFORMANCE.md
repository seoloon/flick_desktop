# Performance WebKit (macOS) : base de connaissances

Contexte : l'app est moins fluide sur Mac (WKWebView / WebKit) que sur Windows
(WebView2 / Chromium), surtout sur 8 Go de RAM avec GPU intégré. Objectif :
fluidité sur les deux OS, minimum 8 Go, sans dégrader l'expérience.

Ce document garde le travail d'optimisation fait le 2026-10-02, **mis de côté
pour le moment** (sauf le correctif de la sidebar, qui est dans le code). Il
sert de référence pour y revenir.

- Patch prêt à réappliquer : `docs/perf/webkit-optimizations.patch`
  (`git apply docs/perf/webkit-optimizations.patch`, vérifié sans conflit sur
  le commit `5990400`).
- Aucune de ces optimisations n'a été mesurée ni testée visuellement dans
  l'app : typecheck, 41 tests UI et génération des classes Tailwind
  uniquement. À valider sur un Mac avant de les garder.

## 1. Diagnostic

| Constat | Détail |
|---|---|
| Moteur imposé | Tauri 2 utilise le WebView du système : WebKit sur macOS, Chromium sur Windows. Pas de réglage pour en changer. Embarquer Chromium (CEF/Electron) coûte ~150 Mo et beaucoup de RAM : contraire à l'objectif 8 Go. |
| Point faible de WebKit | `backdrop-filter` animé ou redimensionné, gros `filter: blur()` plein écran, nombreuses couches composées. Le défilement et les transforms simples vont bien. |
| Le Rust est sain | Lectures disque en pool bloquant, cache d'images LRU, événements player à 4 Hz, boucle d'animation arrêtée hors lecture, SQLite en WAL. Rien d'urgent côté Rust. |
| Le lecteur est sain | Position en `motionValue` interpolée, rAF seulement en lecture. |

## 2. Correctif conservé : repli de la sidebar

Symptôme : replier/déplier la sidebar saccadait sur Mac, mais pas sur Windows.

Cause probable : `toggleSidebar` animait `main` (tout le contenu) en
`translateX` pendant 500 ms (technique FLIP). Pendant ce temps, toutes les
couches `backdrop-filter` (sidebar, barre de titre, bandes de flou) re-floutaient
le contenu en mouvement à chaque frame.

Correctif (`ui/src/lib/sidebar.ts`) : le contenu prend sa nouvelle marge d'un
coup, seule la sidebar glisse par-dessus. Contrepartie visuelle : le contenu
saute au lieu de glisser.

Si ça saccade encore : la largeur animée de la sidebar (`transition-[width]`
sur un panneau `.glass`) force layout + re-flou à chaque frame. Piste : la
mettre à sa largeur dépliée en permanence et animer un `clip-path`
(les icônes ne bougent pas : `p-3` + `p-3` + `px-3` = 36 px centrés dans
5,65 rem). Attention : le contour `inset` du verre serait rogné sur le bord
droit.

## 3. Optimisations mises de côté

### 3.1 Cartes en CSS pur (`components/tv/Card.tsx`)

Avant : chaque `MediaCard` créait 4 `useSpring`, 3 `useMotionValue`, un
`useMotionTemplate`, 4 composants `motion.*`, `perspective` + `preserve-3d`
(donc une couche 3D par carte) et un badge `.glass` (donc un `backdrop-filter`)
même à opacité 0. Une page Home en contient des centaines.

Après :
- Le bouton est un `<button>` simple. L'état `data-lifted` / `data-focus`
  pilote des variantes Tailwind (`group-data-[lifted]:…`).
- Zoom : propriété CSS `scale` (`transition-[scale]`, courbe
  `cubic-bezier(0.22,1,0.36,1)`, 300 ms, proche du ressort `focusSpring`
  quasi critique : ζ ≈ 0,93).
- Inclinaison et reflet : variables CSS `--rx --ry --lx --ly` écrites sur la
  seule carte survolée, `transform: perspective(900px) rotateX() rotateY()`
  avec `transition-transform 200ms`.
- Ombre : deux couches (repos / soulevée), seule l'opacité s'anime (au lieu
  d'animer `box-shadow`, qui repeint).
- Badge serveur : rendu uniquement quand la carte est soulevée.
- Pastille « vu » : `bg-black/45` au lieu de `.glass`. Barre de progression :
  `bg-black/40` au lieu de `backdrop-blur`.

Compromis : le « ressort » devient une courbe CSS (très proche, pas
identique) ; le reflet suit le curseur sans lissage.

### 3.2 Fond ambiant sans blur de 72 px (`shell/AmbientBackdrop.tsx`)

Avant : `<img>` plein écran avec `filter: blur(72px) saturate(1.6)
brightness(0.62)` et une animation de `scale`, donc un flou gaussien énorme
recalculé par le GPU à chaque composition. Le coût le plus élevé de l'écran.

Après : l'image est dessinée une seule fois dans un `<canvas>` 16×9 (recadré
comme `object-fit: cover`), puis agrandie par paliers (32, 64, 128 px) avec
`imageSmoothingQuality = "high"`. Chaque agrandissement bilinéaire étale la
couleur, l'étirement final jusqu'à la fenêtre (CSS) termine le lissage. Seul
reste en CSS `filter: saturate(1.6) brightness(0.62)` (matrice de couleur,
bon marché). L'animation d'apparition (`opacity`, `scale`) est inchangée.

Points d'attention :
- Un canvas contaminé (image d'un autre schéma, `oneshot-img://`) peut être
  affiché, mais pas relu : le code n'utilise jamais `getImageData`. Ne pas en
  ajouter sans en-tête CORS sur le protocole d'images.
- À vérifier visuellement : pas de facettes (« diamants ») dans les dégradés.
  Si visibles, augmenter `BASE_W/BASE_H` (ex. 24×14) ou `OUT_W/OUT_H`.
- `ctx.filter` (blur natif du canvas) n'a pas été utilisé : support Safari
  incertain.

### 3.3 Bandes de flou du défilement (`shell/Shell.tsx`)

Avant : 2 bords × (5 couches `backdrop-filter` + dégradé) toujours présents
au-dessus du scroller, même à opacité 0. Chaque couche re-floute le contenu à
chaque frame de scroll.

Après :
- `EDGE_BLURS = [2, 6, 16]` (3 couches par bord au lieu de 5).
- Le conteneur est `invisible` tant que son bord n'a rien à montrer, avec
  `transition: visibility 0s linear 500ms` pour ne le cacher qu'après la fin
  du fondu, et `transition-delay: 0s` quand il apparaît
  (`peer-data-fade-start:visible`, `peer-data-fade-end:visible`).
  Hors zone visible, plus aucun `backdrop-filter` ne tourne.

### 3.4 Flou progressif de la bannière (`components/tv/Hero.tsx`)

`<ProgressiveBlur layers={7}>` passé à `layers={4}` (flous 3,5 / 7 / 14 / 28 px,
même maximum de 28 px).

### 3.5 Écrans en chargement différé (`App.tsx`)

Avant : un seul bundle de 1,2 Mo de JS analysé au démarrage.
Après : 730 Ko pour le bundle principal. Admin, Debug, Detail, Favorites,
PersonPage, Libraries, LibraryGrid, PlayerView, GenreGrid, Search, Settings
et Watch passent en `React.lazy`.

- Chaque route est enveloppée dans `<Suspense fallback={null}>` (composant
  `Lazy`) **à l'intérieur** de la route, pour que `Shell` reste monté.
- `preloadScreens()` récupère les chunks probables (Detail, PlayerView,
  Libraries, LibraryGrid, Search, Settings, Favorites) en
  `requestIdleCallback`, **après la fin de l'intro** (`shown >= 0`) pour ne pas
  lui voler de frames.
- Non fait : intro Remotion en différé (~200 Ko). Le gain est faible et il
  faudrait un fallback plein écran à la couleur de la page pour éviter un flash.

### 3.6 Profil release (`Cargo.toml`)

`strip = "symbols"` : binaire plus petit. Volontairement **pas** de
`panic = "abort"` : un panic dans une tâche tokio ne doit pas tuer l'app (le
hook de panic le journalise déjà).

## 4. Idées non réalisées

| Idée | Pourquoi pas encore |
|---|---|
| Désactiver le flou des panneaux (`appearance.blur = false`) par défaut sur macOS | Retire le verre dépoli ; le réglage « Frosted glass » existe déjà pour comparer. Option la plus sûre si la fluidité reste insuffisante. |
| Fenêtre non transparente | La transparence est nécessaire : la vidéo mpv native est sous la WebView. |
| `content-visibility: auto` sur les étagères | Applique un `contain: paint` qui peut rogner les cartes soulevées et leurs ombres. |
| Animer la sidebar par `clip-path` | Voir 2. À essayer si le repli saccade encore. |
| UI native (SwiftUI) | Réécriture ; deux UI à maintenir. |

## 5. Comment mesurer

1. `pnpm desktop` avant et après (`git stash` / `git apply` du patch).
2. Activity Monitor : CPU, GPU et mémoire du processus « Flick Web Content ».
3. Safari > Develop > le WebView de Flick > Timelines (Layout & Rendering,
   Frames) pendant : scroll de Home, survol de cartes, repli de la sidebar,
   changement de sélection (fond ambiant).
4. Comparer sur la machine de référence : Mac 8 Go.
