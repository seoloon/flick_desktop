# Flick — Architecture technique

> Client desktop unifié Jellyfin + Plex, Rust/Tauri, moteur de lecture natif.
> Ce document décrit **les choix**, **pourquoi**, et **ce qui est réellement garanti**.
> Tout ce qui est marqué ✅ a été vérifié sur une vraie machine (voir
> [`docs/PLAYBACK_VALIDATION.md`](docs/PLAYBACK_VALIDATION.md)) ; ce qui est marqué
> 🟡 est conçu mais pas encore validé sur matériel ; ❌ est une limite connue.

---

## 0. Résumé exécutif

**Le problème difficile n'est pas « lire un MKV »** — n'importe quel binding FFmpeg
le fait. Le problème est d'obtenir *simultanément* :

1. une sortie **HDR réelle** (signalisation PQ/HLG au compositeur de l'OS, pas
   seulement un décodage 10 bits),
2. un **bitstream audio** (AC3/E-AC3/DTS/DTS-HD/TrueHD/Atmos) négocié
   correctement avec le récepteur, avec un repli propre quand il refuse,
3. la **4K/60 décodée par le GPU** avec une synchro A/V stable,
4. le tout **sous une UI WebView** (Tauri) animée, transparente, avec des
   overlays, sans que la WebView ne touche jamais aux pixels vidéo,
5. avec des comportements **radicalement différents** selon Windows, macOS,
   Linux X11 et Linux Wayland.

**Décision :**

| Couche | Choix | Statut |
|---|---|---|
| Moteur média (demux, décodage, A/V sync, audio, sous-titres, tone mapping) | **libmpv** (≥ 0.41, `vo=gpu-next` / libplacebo), chargée dynamiquement | ✅ Windows |
| Présentation vidéo sous l'UI | **Stratégie par OS** derrière un trait `VideoPresenter` — pas une seule technique | voir §4 |
| Windows | Swapchain mpv en mode **composition** insérée dans un arbre **DirectComposition** *derrière* la WebView2 transparente ; repli fenêtre enfant (`--wid`) | ✅ testé |
| macOS | API de rendu libmpv (OpenGL) dans un `CAOpenGLLayer` EDR placé sous la `WKWebView` | 🟡 conçu |
| Linux X11 | Fenêtre mpv dédiée plein écran + contrôles natifs (OSD mpv) ; `--wid` ne permet pas de mélange alpha avec WebKitGTK | 🟡 conçu |
| Linux Wayland | Fenêtre mpv dédiée (HDR via `wp-color-management-v1`) + OSD natif | 🟡 conçu |
| Décision de lecture | `PlaybackDecisionEngine` pur, déterministe, testé (25 scénarios), explicable | ✅ |
| Réconciliation | Le plan (issu des métadonnées serveur) est corrigé par ce que mpv observe réellement | ✅ (cas réel Jellyfin 12.1) |
| Serveurs | Jellyfin (≥ 10.9, testé 12.1) et Plex (PMS testé 1.43) : clients natifs, tests contre de vrais serveurs | ✅ |
| Capacités | `CapabilityManager` qui **sonde** l'OS (DXGI/CCD, D3D11 VideoDevice, WASAPI IEC 61937) | ✅ Windows / 🟡 autres |

libmpv est retenu **parce que** le spike l'a confronté aux cinq exigences
ci-dessus, pas par défaut. Il n'est pas « la solution » à lui seul : il résout
(1)(2)(3) moteur, mais la présentation (4)(5) est un sous-système à part entière
que nous possédons et qui diffère par OS. La couche moteur est elle-même
abstraite (`PlayerEngine`) pour qu'un backend spécialisé (AVFoundation pour le
Dolby Vision natif sur Apple, par exemple) puisse être ajouté sans toucher l'UI.

---

## 1. Contraintes Jellyfin / Plex

### 1.1 Jellyfin (API REST officielle, OpenAPI publiée)

- **Auth** : `POST /Users/AuthenticateByName` avec l'en-tête
  `Authorization: MediaBrowser Client=…, Device=…, DeviceId=…, Version=…[, Token=…]`.
  Quick Connect disponible (`/QuickConnect/Initiate` → `/Users/AuthenticateWithQuickConnect`).
  Le token est un *access token* de session par appareil (révocable côté serveur).
- **Catalogue** : `/Items` (filtres, tri, pagination `StartIndex/Limit`),
  `/Users/{id}/Items/Resume`, `/Shows/NextUp`, `/Items/Latest`, `/Shows/{id}/Seasons`,
  `/Shows/{id}/Episodes`, `/Items/{id}/Similar`, `/Persons`, `/Genres`.
- **Lecture** : `POST /Items/{id}/PlaybackInfo` avec un **`DeviceProfile`**. Le
  serveur répond par source `SupportsDirectPlay / SupportsDirectStream /
  SupportsTranscoding` + `TranscodeReasons`. **C'est le point clé** : un client
  qui envoie un profil pauvre (ex. profil « navigateur ») force le transcodage.
  Nous générons le profil depuis les capacités réelles du moteur (§9.3).
- **Flux direct** : `/Videos/{id}/stream?static=true&MediaSourceId=…` (octets
  d'origine, *range requests*).
- **Suivi** : `POST /Sessions/Playing`, `/Sessions/Playing/Progress`,
  `/Sessions/Playing/Stopped` (positions en *ticks* de 100 ns, `PlaySessionId`).
- **Segments** (skip intro/crédits) : `GET /MediaSegments/{itemId}` (10.10+),
  alimenté par des plugins (Intro Skipper…). Absent → pas de bouton, pas de faux.
- **Images** : `/Items/{id}/Images/{type}?tag=…&maxWidth=…` (+ BlurHash dans les
  métadonnées → placeholders instantanés).
- **Admin** : `/System/Info`, `/Sessions`, `/Users`, `/ScheduledTasks`,
  `/Library/Refresh`, `/System/Logs` — autorisés par la politique
  `IsAdministrator` ; un 401/403 est remonté tel quel.

### 1.2 Plex (API semi-documentée, docs officielles développeur depuis 2025)

- **Auth** : flux **PIN** plex.tv (`POST https://plex.tv/api/v2/pins?strong=true`,
  l'utilisateur valide sur `app.plex.tv/auth#?clientID=…&code=…`, puis polling
  `GET /api/v2/pins/{id}`) → token de compte. Jamais de mot de passe saisi dans
  l'app.
- **Découverte** : `GET https://plex.tv/api/v2/resources?includeHttps=1&includeRelay=1`
  → serveurs avec **plusieurs connexions** (locale, distante, relais) et un
  **token par serveur**. Il faut sonder et choisir la meilleure connexion.
- **Catalogue** : `/library/sections`, `/library/sections/{id}/all`,
  `/library/metadata/{ratingKey}`, `/children`, `/hubs/home/…`,
  `/hubs/search?query=`, `/library/metadata/{id}/similar`.
- **Lecture** : la *part key* (`/library/parts/{id}/{ts}/file.ext`) est le fichier
  original → Direct Play. La décision serveur
  (`/video/:/transcode/universal/decision`) utilise un profil client ; un profil
  étendu passe par `X-Plex-Client-Profile-Extra`.
- **Suivi** : `/:/timeline?ratingKey=…&state=playing|paused|stopped&time=…`,
  `/:/scrobble`, `/:/unscrobble`.
- **Marqueurs** : `/library/metadata/{id}?includeMarkers=1` (intro, crédits).
- **Favoris** : Plex n'en a pas sur les titres d'une bibliothèque. Flick
  utilise la **Watchlist** du compte plex.tv
  (`GET https://discover.provider.plex.tv/library/sections/watchlist/all`,
  `PUT …/actions/addToWatchlist|removeFromWatchlist?ratingKey=`), rapprochée
  des bibliothèques par le guid `plex://…` (`/library/all?guid=`). Les titres
  de la Watchlist absents des serveurs ne sont pas montrés. API non
  documentée officiellement : vérifiée par le test `watchlist_live`.
- ❌ **Restriction commerciale** : depuis le 29/04/2025, la lecture **à distance**
  de médias personnels exige un Plex Pass (propriétaire ou utilisateur) ou un
  Remote Watch Pass ; l'application aux clients tiers utilisant l'API est
  annoncée pour 2026. Nous ne la contournons pas : l'erreur serveur est affichée
  telle quelle. La lecture locale (LAN) n'est pas concernée.
- ❌ Certaines fonctions d'admin Plex ne sont pas exposées par API publique
  stable ; nous n'exposons que ce qui l'est (sessions, bibliothèques/scan, tâches
  *butler*, infos serveur).

### 1.3 Conséquence : couche d'abstraction commune

Les deux serveurs diffèrent sur l'identité (GUID vs `ratingKey`), la structure
(Plex « hubs » vs requêtes Jellyfin), le temps (ticks vs ms), les images (tag vs
chemin transcodé) et la négociation de lecture. Le trait `MediaProvider` (§8) est
**orienté intention** (« donne-moi la ligne *Continue Watching* ») et chaque
provider mappe vers le modèle commun (`oneshot-core`). Un provider qui ne sait pas
faire renvoie `Error::Unsupported` — jamais une donnée inventée.

---

## 2. Contraintes Tauri / WebView

| Contrainte | Impact | Réponse |
|---|---|---|
| WebView différente par OS (WebView2/Chromium, WKWebView, WebKitGTK) | Codecs, HDR, perfs CSS variables | La WebView **ne lit jamais de média**. Elle ne dessine que l'UI. |
| La WebView compose en **SDR 8 bits** (UI) | Une vidéo HDR ne peut pas passer par la WebView | La vidéo est un calque natif **sous** la WebView (§4). |
| La WebView ne voit pas les pixels des calques natifs | `backdrop-filter: blur` au-dessus de la vidéo **ne floute pas la vidéo** (✅ constaté) | Voile translucide + dégradés au-dessus de la vidéo. Flou réel possible sous Windows via un effet DirectComposition sur un visuel cloné (🟡). |
| IPC JSON (commandes/événements) | Coût de sérialisation, latence ~ms | Commandes à la demande, **événements lecteur throttlés (4 Hz)** + interpolation `requestAnimationFrame` côté UI pour une timeline fluide. |
| Tokens accessibles au JS s'ils sont dans des URLs `<img>` | Fuite de credentials (devtools, extensions, logs) | Protocole personnalisé **`oneshot-img://`** : l'UI demande une image par référence opaque + taille, Rust résout l'URL authentifiée et met en cache disque. Aucun token ne quitte Rust. |
| Fenêtre transparente : macOS exige `macOSPrivateApi` | Incompatible Mac App Store | Distribution hors App Store (DMG notarisé). Documenté. |
| Tauri n'expose pas l'hébergement « visual » de WebView2 | Impossible de mettre la WebView elle-même dans notre arbre DComp | Inutile : `CreateTargetForHwnd(topmost=false)` place notre visuel vidéo **derrière** les HWND enfants (WebView2) — validé ✅. |

---

## 3. Comparatif des moteurs multimédias

Critères pondérés par la difficulté réelle (HDR, bitstream, intégration),
pas par la simple liste de codecs (que tous couvrent via FFmpeg).

| Critère | **libmpv (gpu-next)** | FFmpeg « maison » | GStreamer | libVLC 3.x | Natif OS (Media Foundation / AVFoundation) |
|---|---|---|---|---|---|
| H.264 / HEVC / AV1 / VP9 | ✅ (FFmpeg) | ✅ | ✅ (plugins) | ✅ | MF : HEVC via extension payante, AV1 via extension ; AVF : pas d'AV1 avant M3, pas de VP9 |
| MKV, PGS, ASS | ✅ (libass, PGS natif) | à écrire (libass à intégrer) | ASS ok, PGS faible | ✅ | ❌ MKV partiel, pas d'ASS/PGS |
| Décodage GPU | d3d11va, nvdec, vaapi, videotoolbox, vulkan ✅ | à écrire par API | ✅ (d3d11/va/vt) | ✅ | ✅ natif |
| Tone mapping HDR→SDR | **libplacebo** (état de l'art, dynamique, gamut mapping) | à écrire (ou libplacebo) | basique | basique (3.x) | ✅ OS |
| HDR passthrough | d3d11 / winvk / Wayland CM / macOS EDR (render API) | à écrire par OS | d3d11/d3d12 sinks seulement | Windows d3d11 | ✅ natif |
| Dolby Vision | **Reshaping RPU P5/P8** via libplacebo → PQ ou SDR (pas de signalisation DV) | ❌ | ❌ | ❌ | AVF : **vrai DV** sur écrans Apple (MP4/HLS uniquement) |
| Bitstream AC3/DTS | ✅ `audio-spdif` | à écrire (IEC 61937) | alsasink/wasapi partiel | ✅ | MF limité ; AVF ❌ |
| Bitstream TrueHD/DTS-HD (HBR) | ✅ WASAPI exclusif, ALSA direct | à écrire | ❌ en pratique | ✅ Windows | ❌ |
| Atmos | seulement en bitstream (TrueHD/E-AC3 JOC) | idem | idem | idem | AVF : Atmos E-AC3 JOC → spatial Apple |
| Intégration sous une WebView | `--wid`, **d3d11 composition** (0.41+), render API GL | totale (on possède le renderer) | VideoOverlay / appsink | `set_hwnd`/`set_nsobject` | natif par OS |
| API de contrôle | propriétés observables + commandes, très complète | à écrire | pipeline bas niveau | API C figée (4.0 jamais sortie) | par OS |
| Rust | FFI simple (18 fonctions), chargement dynamique | `ffmpeg-next` + énorme travail | `gstreamer-rs` excellent | bindings non maintenus | `windows`/`objc2` |
| Coût d'implémentation | faible | **très élevé** (= réécrire mpv) | élevé | moyen | élevé ×3 OS |
| Licence | LGPL (build `-Dgpl=false`) | LGPL/GPL | LGPL | LGPL | propriétaire |

**Pourquoi pas les autres :**

- **FFmpeg maison** : c'est réécrire mpv (horloge A/V, resampling, pipelines
  hwdec→rendu zéro copie, libass, IEC 61937, gestion des fins de flux, seek
  précis). Des années-homme pour arriver au niveau actuel. Réservé à un besoin
  que mpv ne couvre pas (aucun identifié).
- **GStreamer** : très bon framework, excellents bindings Rust, mais HDR
  (tone mapping, passthrough hors d3d11/d3d12), PGS et bitstream HBR sont en
  retrait ; on passerait notre temps sur des éléments du pipeline.
- **libVLC 3** : bon sur le bitstream Windows, mais tone mapping et HDR moins
  avancés que libplacebo, API figée depuis des années (libVLC 4 jamais publiée),
  pas de bindings Rust maintenus.
- **APIs natives** : excellentes sur *leur* terrain (AVFoundation + Dolby Vision
  sur Mac, MF + HDR sur Windows) mais aucune ne lit l'ensemble MKV/DTS/TrueHD/
  PGS/ASS. Elles sont prévues comme **backends spécialisés optionnels** (trait
  `PlayerEngine`), pas comme base.

**Précédent qui valide l'approche** : la réécriture CEF + mpv du client Jellyfin
Desktop (dont le fork Rust *Jellium Desktop*, ex-`jellyfin-labs/jellyfin-desktop-cef`,
dont nous avons lu le code) a abandonné QtWebEngine pour la même raison
et fait lui aussi composer l'UI **au-dessus** d'une surface possédée par mpv
(DirectComposition sous Windows, `CAMetalLayer` sous macOS, proxy Wayland). Nous
arrivons à la même conclusion indépendamment, avec Tauri/WebView2 au lieu de CEF.

---

## 4. Le vrai problème : présenter la vidéo sous l'UI

### 4.1 Principe

> La vidéo n'est **jamais** un élément de la WebView. C'est un calque natif
> géré par le moteur, placé **sous** une WebView transparente. L'UI réserve la
> zone vidéo en étant transparente à cet endroit.

Conséquences :
- HDR, 10 bits, cadence et synchro A/V sont entièrement gérés hors WebView.
- Les animations UI (60/120 Hz) n'impactent pas le rendu vidéo et inversement.
- L'UI peut réduire la vidéo (mini-lecteur, fond animé) en changeant la
  géométrie du calque (DComp : transformation sans re-rendu).

### 4.2 Stratégies (`VideoPresenter`)

| Stratégie | Mécanisme | OS | Qualité | Statut |
|---|---|---|---|---|
| `Composition` | mpv `--d3d11-output-mode=composition` → `display-swapchain` → `IDCompositionVisual` sur la fenêtre Tauri, cible `topmost=false` (derrière la WebView2) | Windows | Mélange alpha parfait, HDR via DWM, géométrie contrôlée par nous | ✅ validé |
| `ChildWindow` | mpv `--wid=<HWND Tauri>` : HWND enfant sous la WebView2 | Windows (repli si mpv < 0.41) | Mélange alpha correct, moins de contrôle | ✅ validé |
| `LayerRender` | API de rendu libmpv (OpenGL) → `CAOpenGLLayer` (EDR, espace PQ) sous la `WKWebView` | macOS | HDR/EDR possible (approche IINA) ; OpenGL déprécié mais fonctionnel | 🟡 |
| `DedicatedWindow` | Fenêtre plein écran possédée par mpv ; contrôles rendus par l'OSD mpv (`osd-overlay`) pilotés depuis Rust | Linux (X11/Wayland), repli universel | HDR Wayland ✅ (mpv 0.40+), UI du lecteur plus simple | 🟡 |

**Pourquoi pas `--wid` partout ?** `--wid` n'existe que pour X11, Win32 et
Android (doc mpv). Sous X11, une fenêtre enfant n'est pas composée en alpha avec
ses sœurs : une WebKitGTK transparente par-dessus ne laisse pas voir la vidéo
(le plugin `tauri-plugin-libmpv` documente l'échec de l'embarquement sous
Linux). Sous Wayland, `--wid` n'existe pas.

### 4.3 Résultats du spike Windows (RTX 5060, Win 11, WebView2)

- Les **deux** modes Windows affichent la WebView2 transparente correctement
  mélangée par-dessus la vidéo (bloc opaque, dégradé, panneau 45 % d'opacité).
- `backdrop-filter: blur()` **ne floute pas** la vidéo (attendu : la WebView ne
  possède pas ces pixels).
- En mode composition, mpv ne peut pas interroger l'écran (« Failed to query swap
  chain's output information ») : **mpv ne sait pas si l'écran est en HDR**.
  → C'est notre `CapabilityManager` qui fournit `target-trc/prim/peak` et active
  `target-colorspace-hint` quand l'écran sous la fenêtre est en HDR ; on
  recalcule quand la fenêtre change d'écran. C'est précisément le genre de
  détail qui justifie de posséder la couche de présentation.
- 4K HEVC Main10 HDR10 en composition : décodage d3d11va, tone mapping
  libplacebo, **~0,07 s CPU/s** pour tout le processus.

---

## 5. HDR — ce qui est garanti

| Source | Écran SDR | Écran HDR, HDR OS **désactivé** | Écran HDR, HDR OS **actif** |
|---|---|---|---|
| SDR | SDR | SDR | SDR (composé par l'OS au niveau de blanc SDR) |
| HDR10 / HDR10+ | tone mapping libplacebo ✅ | tone mapping + **suggestion d'activer le HDR** (on détecte « capable mais éteint » ✅) | **PQ BT.2020** signalé au compositeur 🟡 (pas d'écran HDR actif lors du test) |
| HLG | tone mapping | idem | HLG→PQ |
| Dolby Vision P5 / P8.x | reshaping RPU → SDR | idem | reshaping RPU → **HDR10/PQ** (pas de signalisation Dolby Vision) |
| Dolby Vision P7 (FEL) | couche de base + RPU, **FEL ignorée** | idem | idem |

❌ **Aucun lecteur PC Windows/Linux ne peut émettre un vrai signal Dolby Vision
(tunneling DV) vers un téléviseur.** Nous affichons « Dolby Vision → HDR10
(reshaping) », jamais « Dolby Vision » tout court. Sur macOS, un backend
AVFoundation optionnel pourrait offrir le vrai DV sur écrans Apple pour les
fichiers MP4/HLS (🟡 futur, via Direct Stream serveur).

HDR10+ : les métadonnées dynamiques sont exploitées par libplacebo pour le tone
mapping ; en passthrough, Windows ne transmet que des métadonnées statiques
HDR10 (`source-dynamic` de mpv est expérimental). Affiché comme tel.

---

## 6. Audio — ce qui est garanti

### 6.1 Constats empiriques (✅, voir docs/PLAYBACK_VALIDATION.md)

- mpv décode tous les codecs testés (AAC, AC3, E-AC3, DTS, TrueHD, FLAC 7.1).
- **Si on force un format bitstream que le périphérique refuse, mpv ne se
  replie PAS sur le PCM : l'audio échoue.** (E-AC3 sur S/PDIF, tout format sur
  l'HDMI d'un moniteur.)
- Notre sonde WASAPI `IsFormatSupported(EXCLUSIVE, IEC 61937)` prédit
  **exactement** ce que mpv obtient : S/PDIF Realtek → {AC3, DTS} ; HDMI moniteur
  → {} ; périphériques USB/virtuels → {}.

### 6.2 Stratégie

1. On ne passe à `--audio-spdif` **que** les formats sondés OK pour le
   périphérique choisi *et* autorisés par l'utilisateur.
2. Repli à chaud : si l'AO échoue après négociation (le récepteur/EDID refuse
   malgré le pilote), le lecteur retire le passthrough, `ao-reload`, et journalise
   la raison dans la décision.
3. PCM : nombre de canaux = format de mixage de l'OS (réglage « Configuration
   des haut-parleurs »). Downmix propre par mpv (matrices FFmpeg), sinon
   5.1→5.1 et 7.1→7.1 inchangés.
4. DTS-HD sans support HBR mais DTS accepté → **cœur DTS** en bitstream (perte
   déclarée : « DTS core »).
5. S/PDIF + récepteur 5.1 : option *AC3 re-encode* (filtre `lavcac3enc`) pour
   transporter du 5.1 depuis n'importe quelle source (perte déclarée).

### 6.3 Atmos / DTS:X — table de vérité

| Condition | Résultat affiché |
|---|---|
| TrueHD Atmos + HDMI vers AVR Atmos + pilote accepte MLP (HBR) | **Atmos (bitstream)** |
| E-AC3 JOC + HDMI/ARC E-AC3 accepté | **Atmos (bitstream E-AC3)** |
| Tout autre cas | « TrueHD 7.1 (Atmos non reproduit) » — lit le **lit 7.1** décodé |

❌ Windows Sonic / Dolby Atmos for Headphones / Dolby Access (API
`ISpatialAudioClient`) ne sont pas exploités par mpv : pas d'Atmos « objets »
en décodage local. ❌ macOS : CoreAudio ne permet pas le bitstream HBR
(TrueHD/DTS-HD) ; AC3/DTS seulement via `coreaudio_exclusive` (S/PDIF/HDMI).
🟡 Linux : ALSA `hdmi:`/`iec958:` direct = HBR OK ; via PipeWire, AC3/DTS/E-AC3
fonctionnent, TrueHD/DTS-HD sont signalés instables selon versions → sonde +
repli.

---

## 7. Architecture retenue

```
┌──────────────────────────────── UI (WebView, React + TS strict) ──────────────────────────────────┐
│  Home · Library · Favourites · Detail · Search · Player · Settings/Servers · Profiles · Admin      │
│  Focus engine (clavier / manette / télécommande)   Design system   Adaptive background             │
└──────────────▲───────────────────────────── IPC typé (commands + events) ─────────────▲───────────┘
               │ invoke()                                  oneshot-img:// (images)     │ events 4 Hz
┌──────────────┴──────────────────────── app (Tauri shell, Rust) ─────────────────────┴────────────┐
│ commands/*  ·  image protocol  ·  window/fullscreen  ·  diagnostics (ring buffer tracing)        │
└──────┬─────────────────┬───────────────────┬─────────────────────┬──────────────────┬────────────┘
       │                 │                   │                     │                  │
┌──────▼──────┐  ┌───────▼───────┐  ┌────────▼────────┐  ┌─────────▼────────┐ ┌───────▼────────┐
│ catalog     │  │ playback      │  │ player           │  │ capabilities     │ │ storage        │
│ multi-srv   │  │ DecisionEngine│  │ PlayerEngine(mpv)│  │ CapabilityManager│ │ settings.sqlite│
│ aggregation │  │ ClientProfile │  │ VideoPresenter   │  │ DXGI/CCD, D3D11, │ │ metadata cache │
│ cache + TTL │  │ (pur, testé)  │  │ Session/progress │  │ WASAPI IEC61937  │ │ image cache    │
└──────┬──────┘  └───────────────┘  └────────┬─────────┘  └──────────────────┘ │ keyring (OS)   │
       │                                     │                                  └────────────────┘
┌──────▼───────────────────────┐    ┌────────▼────────┐
│ providers: jellyfin · plex   │    │ oneshot-mpv     │  (FFI dynamique libmpv)
└──────────────┬───────────────┘    └─────────────────┘
               │                    tous dépendent de ▼
        ┌──────▼──────────────────────────────────────────────┐
        │ oneshot-core : modèle commun, capacités, contrats   │
        └─────────────────────────────────────────────────────┘
```

Chaîne de lecture (exigée par le cahier des charges) :

```
MediaItem ─► provider.playback_info(profil client) ─► CapabilityManager.report()
          ─► PlaybackDecisionEngine.decide() ─► provider.stream(decision) ─► PlayerEngine (libmpv)
          ─► VideoPresenter (calque natif)  +  AO (PCM / bitstream)  ─► rapport de progression serveur
```

### 7.1 Crates (workspace Cargo)

| Crate | Rôle | Pourquoi un crate séparé |
|---|---|---|
| `oneshot-core` | Types de domaine, capacités, contrats (`MediaProvider`), erreurs | Zéro I/O, compilé partout, source unique des types (exportés en TS) |
| `oneshot-mpv` | FFI libmpv chargée dynamiquement, `Node`, événements | `unsafe` confiné ; démarrage possible et diagnostic clair sans libmpv |
| `oneshot-net` | Politique HTTP commune (timeouts, proxy, TLS, masquage des tokens dans les logs, erreurs) | Même comportement réseau pour providers et cache d'images |
| `oneshot-capabilities` | Sondes OS (Windows complet ; macOS/Linux « inconnu » honnête) | Code `windows`/`objc2` isolé par OS |
| `oneshot-playback` | `PlaybackDecisionEngine`, `ClientProfile` | Pur et déterministe → testable exhaustivement sans OS ni serveur |
| `oneshot-player` | Session de lecture, options mpv, présentation, repli audio, progression | Seul crate qui parle à mpv et à la fenêtre |
| `oneshot-storage` | Réglages, profils (`profiles.json`, regroupement, PIN), cache métadonnées (SQLite), cache images, secrets (keyring) | Persistance et secrets hors de la logique |
| `oneshot-catalog` | Registre des serveurs, agrégation multi-serveurs, dédoublonnage | Seul endroit qui connaît « plusieurs serveurs » |
| `providers/jellyfin`, `providers/plex` | Clients API → modèle commun | Un provider = un crate, extensible |
| `app` | Shell Tauri : commandes, protocole images, fenêtres, diagnostics | Colle ; aucune logique métier |

### 7.2 Frontend

- **React 19 + TypeScript strict**, Vite, Tailwind v4. Composants de base
  shadcn/ui, animate-ui et smoothui (copiés dans `ui/src/components/`, donc
  modifiables) ; animations **Motion** ; navigation spatiale **Norigin**
  (arbre de focus et géométrie ; les entrées clavier/manette/télécommande sont
  routées par `ui/src/nav/input.ts`). Données serveur via TanStack Query,
  état local via Zustand. Le lecteur garde sa position dans une *motion
  value* : la barre de progression avance à la fréquence d'affichage sans
  re-rendu React. Sélecteur de profils (plein écran hors Shell,
  `features/profiles`) ; changement rapide depuis la sidebar. Voir `docs/DESIGN_SYSTEM.md`.
- Types IPC **générés depuis Rust** (`ts-rs`, feature `ts`) dans
  `ui/src/ipc/bindings/` → une seule définition des types (116 types). Seuls
  les petits DTO du shell (`app/src/commands`) sont écrits à la main dans
  `ui/src/ipc/app-types.ts`.
- Régénération : `cargo test -p oneshot-core -p oneshot-playback -p oneshot-player -p oneshot-catalog --features "oneshot-core/ts oneshot-playback/ts oneshot-player/ts oneshot-catalog/ts" export_bindings`.
- Arborescence : `ui/src/{App.tsx, ipc, nav, shell, components/{tv,ui,animate-ui,smoothui}, features/*, lib}`.

---

## 8. Interfaces clés

```rust
// oneshot-core
#[async_trait]
pub trait MediaProvider: Send + Sync {
    fn kind(&self) -> ProviderKind;
    fn descriptor(&self) -> &ServerDescriptor;
    async fn status(&self) -> ServerStatus;
    async fn libraries(&self) -> Result<Vec<Library>>;
    async fn home(&self) -> Result<Vec<HomeRow>>;            // lignes sémantiques
    async fn items(&self, q: &ItemQuery) -> Result<Page<MediaItem>>;
    async fn item(&self, id: &ItemRef) -> Result<MediaItem>;  // + sources techniques
    async fn children(&self, id: &ItemRef) -> Result<Vec<MediaItem>>;
    async fn search(&self, term: &str, limit: u32) -> Result<Vec<MediaItem>>;
    async fn similar(&self, id: &ItemRef, limit: u32) -> Result<Vec<MediaItem>>;
    async fn adjacent_episodes(&self, id: &ItemRef) -> Result<Adjacent>;
    async fn markers(&self, id: &ItemRef) -> Result<Vec<Marker>>;
    async fn set_played(&self, id: &ItemRef, played: bool) -> Result<()>;
    async fn set_favorite(&self, id: &ItemRef, fav: bool) -> Result<()>;
    async fn favorites(&self, limit: u32) -> Result<Vec<MediaItem>>; // défaut : filtre favoris de items
    async fn playback_info(&self, id: &ItemRef, profile: &ClientProfile) -> Result<PlaybackInfo>;
    async fn stream(&self, req: &StreamRequest) -> Result<StreamTarget>;
    async fn report(&self, r: &PlaybackReport) -> Result<()>;
    fn image_url(&self, image: &ImageRef, size: ImageSize) -> Result<Url>; // interne
    fn admin(&self) -> Option<&dyn AdminProvider>;
}

// oneshot-playback
pub fn decide(input: &DecisionInput) -> PlaybackDecision;   // pur

// oneshot-player
pub trait VideoPresenter { fn mpv_options(&self) -> Vec<(String, String)>;
                           fn on_swapchain(&mut self, ptr: i64) -> Result<()>;
                           fn set_bounds(&mut self, rect: PhysicalRect) -> Result<()>; }
```

Identifiants : `ItemRef = (ServerId local, clé provider)` sérialisé
`"<uuid>:<clé>"` — opaque pour l'UI. `ServerId` est local (le même serveur ajouté
avec deux comptes = deux connexions distinctes).

Structures de données principales (`oneshot-core`) : `MediaItem`, `ItemKind`,
`ImageSet/ImageRef/ImageSize`, `UserState`, `Credit`, `ExternalIds`,
`EpisodeInfo`, `MediaSource`, `VideoStream` (+ `DynamicRange` incl. profils DV),
`AudioStream` (+ `SpatialAudio`), `SubtitleStream`, `Library`,
`ServerDescriptor`, `UserProfile`, `HomeRow`, `ItemQuery/Page`,
`CapabilityReport` (+ `HdrState`, `PassthroughProbe`, `HardwareDecoder`),
`ClientProfile`, `PlaybackInfo`, `PlaybackDecision` (+ plans vidéo/audio/
sous-titres et `DecisionReason` explicables), `PlaybackReport`.

---

## 9. Stratégie de lecture

### 9.1 Entrées du moteur de décision

`MediaSource` (streams réels) · `ServerPolicy` (ce que le serveur autorise pour
cet utilisateur/réseau) · `CapabilityReport` · préférences (passthrough,
formats autorisés, débit max, HDR auto/forcer SDR, préférence transcodage si
pas de décodage matériel, langues) · écran sous la fenêtre · pistes choisies.

### 9.2 Algorithme (déterministe, chaque étape produit une `DecisionReason`)

1. **Sélection des pistes** : vidéo principale ; audio = choix utilisateur, sinon
   langue préférée, sinon défaut du fichier ; sous-titres = choix, sinon forcés
   dans la langue audio, sinon préférence.
2. **Décodabilité locale** : codec connu du moteur ? décodeur matériel pour
   codec + profondeur de bits (sonde D3D11) ? Sinon décodage CPU (*Degraded*) ou,
   si l'utilisateur l'a choisi et que la source est ≥ 4K, transcodage serveur.
3. **Contraintes serveur/réseau** : direct play refusé par le serveur →
   Direct Stream si autorisé, sinon transcodage (raisons serveur recopiées) ;
   débit source > limite utilisateur → transcodage plafonné.
4. **Sortie vidéo** : table §5 selon `HdrState` de l'écran courant.
5. **Sortie audio** : table §6 selon la sonde du périphérique + préférences.
6. **Sous-titres** : toujours rendus localement (texte libass, bitmap PGS/VobSub
   natifs) → **jamais d'incrustation serveur** avec mpv.
7. **Étiquette** : `ServerTranscode` > `DirectStream` > `LocalDecode` (si
   tone mapping, downmix, perte spatiale, reshaping DV) > `DirectPlay`.

### 9.3 « Le serveur n'est pas forcé à transcoder à cause du client »

- Jellyfin : le `DeviceProfile` envoyé déclare **tous** les conteneurs et codecs
  que FFmpeg/mpv décode, `MaxStreamingBitrate` = réglage utilisateur, sous-titres
  en `Embed`/`External` → le serveur répond `SupportsDirectPlay=true` sauf
  politique serveur (droits utilisateur, limite de débit distante).
- Plex : la *part* originale est lue directement ; l'API de décision n'est
  consultée qu'en cas de besoin (transcodage) avec un profil étendu.
- Diagnostic : l'overlay « Advanced » affiche la décision locale, les raisons
  serveur brutes (`TranscodeReasons`) et ce que mpv fait réellement
  (`hwdec-current`, `video-params`, `video-target-params`, `audio-out-params`).
  Si la réalité diffère de la décision (ex. passthrough refusé), c'est visible.

### 9.4 Réconciliation à l'exécution

La décision est prise sur les **métadonnées du serveur**, qui peuvent être
fausses. Cas réel mesuré : Jellyfin 12.1 annonce `VideoRangeType=SDR` pour un
HEVC Main10 PQ/BT.2020 avec métadonnées HDR10 (Plex, lui, le voit HDR10). Une
fois le flux ouvert, `oneshot-player` compare le plan à ce que mpv rapporte
(`video-params/gamma`, `hwdec-current`, échec d'ouverture de l'AO) et corrige :

| Observation | Correction | Raison affichée |
|---|---|---|
| `gamma=pq/hlg` alors que le plan est SDR | tone mapping (ou passthrough si l'écran est en HDR) | `metadata-mismatch` |
| plan HDR mais flux SDR | SDR | `metadata-mismatch` |
| GPU prévu, `hwdec-current=no` | signalé (décodage CPU) | `hwdec-fallback` |
| AO refuse le bitstream malgré la sonde | `audio-spdif=` + `ao-reload` → PCM | `bitstream-runtime-failure` |

Les pistes sont associées par **index de flux du conteneur** (`ff-index` côté
mpv = `Index` Jellyfin/Plex), les sous-titres externes par leur URL.

### 9.5 Réglages mpv dérivés de la décision (exemples)

| Décision | Options |
|---|---|
| Présentation Windows composition | `vo=gpu-next gpu-context=d3d11 d3d11-output-mode=composition d3d11-composition-size=WxH` |
| Décodage matériel | `hwdec=auto-safe` (d3d11va/nvdec/vaapi/videotoolbox selon OS) |
| HDR passthrough | `target-colorspace-hint=yes target-trc=pq target-prim=bt.2020 target-peak=<nits écran>` |
| Tone mapping | `target-colorspace-hint=no` (+ `tone-mapping`/`hdr-compute-peak` selon réglages) |
| Bitstream | `audio-device=<mpv_name> audio-spdif=<formats sondés∩autorisés> audio-exclusive=yes` |
| PCM | `audio-spdif= audio-channels=<layout OS>` |
| Sous-titres | `sid=<n>`, styles utilisateur (`sub-font`, `sub-scale`, `sub-color`, `sub-border-size`, `sub-pos`) |

---

## 10. Sécurité

- **Profils** (`profiles.json`) : un profil = un ensemble de connexions +
  des préférences personnelles ; le profil actif décide des connexions
  chargées (règle unique : `oneshot_storage::profiles::loadable`). Le
  catalogue et les images ne servent, même depuis le cache, que les connexions
  chargées.
- **PIN de profil** : 4 chiffres hachés en argon2id, jamais stockés ni
  journalisés en clair ; 5 échecs → 30 s, puis 60 s, puis 5 min (même
  verrouillage pour les demandes de PIN lors de la désactivation du
  multi-utilisateurs ou d'un changement de mode). C'est un **verrou d'usage
  local**, pas une protection contre qui a accès aux fichiers de la session :
  les tokens, eux, restent dans le trousseau. Modes A/C : relier à un profil
  une connexion qu'utilise un autre profil protégé demande le PIN de ce
  dernier. **Limitation** : le PIN verrouille un profil, pas une connexion ;
  une connexion qu'aucun profil protégé n'utilise reste libre.
- **PIN Plex Home** : jamais stocké, il part à plex.tv à chaque changement de
  profil. Les connexions d'un membre Plex Home protégé ne chargent qu'une fois
  son PIN vérifié par plex.tv pendant cette exécution, au choix de son profil
  (multi-utilisateurs désactivé, elles ne chargent donc pas) ; si plex.tv ou
  la connexion du compte plex.tv est indisponible (ou si le PIN est passé), ce
  membre reste indisponible mais le reste du profil charge.
- **Aucun mot de passe stocké.** Jellyfin : mot de passe envoyé une fois pour
  obtenir un token (ou Quick Connect). Plex : flux PIN, jamais de mot de passe.
- Tokens dans le **trousseau de l'OS** (`keyring` : Windows Credential Manager,
  macOS Keychain, Secret Service), clé = `ServerId`. Plex garde en plus le
  token plex.tv du compte (`plex-account`) et celui de chaque utilisateur
  (`plex-user:<id>`, pour sa Watchlist), ainsi que la clé TMDB (`tmdb-key`)
  saisie dans Réglages › Metadata, jamais renvoyée à la WebView. La base SQLite ne contient aucun
  secret.
- Les tokens **ne quittent jamais Rust** : images via `oneshot-img://`, flux
  passés à mpv avec en-têtes HTTP (`http-header-fields`) quand le serveur le
  permet.
- CSP stricte dans la WebView, aucune origine distante chargée.
- Logs : en-têtes `Authorization`/`X-Plex-Token`/`api_key` et valeurs de
  PIN (`pin=`) masqués.
- Admin : aucune action n'est tentée si le serveur ne l'autorise pas ; 403
  affiché tel quel.

## 11. Cache & performance

- **Métadonnées** : SQLite (`rusqlite`, bundled), entrées `(server, clé,
  payload JSON, fetched_at, ttl)`. Stale-while-revalidate : l'UI reçoit le cache
  immédiatement puis la version serveur. Le serveur reste la source de vérité ;
  toute mutation (vu/favori/progression) invalide les entrées concernées.
- **Images** : 5 tailles fixes (`Tiny/Card/Large/Hero/Original`) → cache disque
  adressé par (image, taille), LRU plafonné (défaut 1 Go). BlurHash/`Tiny` pour
  les fonds et placeholders.
- **Listes** : virtualisation (fenêtrage) des grilles et rangées.
- **Réseau** : `reqwest` + pool de connexions HTTP/2, parallélisme borné
  (réglage), agrégation multi-serveurs en parallèle avec délai par serveur (un
  serveur lent ne bloque pas la Home).

## 12. UI, Flick Frame, focus

- Design system : tokens CSS (espacements 4 px, typographie, rayons, ombres,
  durées/courbes d'animation) + composants (Button, Card, Row, Dialog, Menu,
  Tabs, Slider, Toggle). Documenté dans `docs/DESIGN_SYSTEM.md`.
- **Focus engine** unique (clavier, manette via Gamepad API, télécommande =
  flèches/Enter/Back) : navigation spatiale par géométrie, mémoire de focus par
  rangée. Le mode normal l'utilise aussi (accessibilité clavier).
- **Flick Frame** : même données, layouts dédiés (échelle ×1,5–2, safe area TV,
  focus très visible, transitions plus cinématiques), plein écran, curseur
  masqué.
- **Fond adaptatif** : palette extraite côté Rust depuis l'image `Tiny` (k-means
  sur ~2 000 pixels, couleurs contraintes en luminance pour garantir le contraste
  du texte), fondu croisé CSS de 900 ms entre deux calques.

## 13. Observabilité

`tracing` structuré partout (cibles `provider`, `playback`, `player`, `mpv`,
`capabilities`, `cache`), couche *ring buffer* en mémoire exposée au panneau
Debug + fichier journal rotatif. Chaque lecture produit un **rapport de
lecture** : décision, raisons, options mpv appliquées, propriétés effectives
observées (hwdec, colorimétrie in/out, format audio in/out, pertes d'images).

## 14. Limitations documentées

| Domaine | Limite | Origine |
|---|---|---|
| Dolby Vision | Pas de signalisation DV vers l'écran (reshaping → HDR10/SDR) ; FEL P7 ignorée | Pilotes/OS PC, licence Dolby |
| Atmos / DTS:X | Uniquement en bitstream ; décodage local = lit de canaux | Pas de décodeur objets libre ; mpv n'utilise pas `ISpatialAudioClient` |
| macOS audio | Pas de bitstream TrueHD/DTS-HD | CoreAudio (pas de HBR) |
| HDR Windows | Nécessite « Utiliser le HDR » activé par l'utilisateur ; on détecte et on le suggère | Windows |
| HDR Linux | Wayland + compositeur avec `wp-color-management-v1` (KDE 6.x, GNOME 48+) ; X11 : pas de HDR | Linux |
| Présentation Linux | UI du lecteur via OSD mpv (fenêtre dédiée) | WebKitGTK/X11/Wayland (§4.2) |
| Flou « verre » sur la vidéo | Voile translucide seulement (flou natif DComp possible sous Windows) | WebView |
| Plex distant | Plex Pass / Remote Watch Pass requis pour les médias personnels à distance | Politique Plex (2025-2026) |
| Changement de fréquence d'écran | Non automatique au lancement ; `video-sync=display-resample` pour la fluidité | À implémenter (ChangeDisplaySettingsEx) |
| DRM | Aucun contenu DRM (Live TV chiffrée, etc.) | Hors périmètre |
| Distribution | App Store macOS impossible (`macOSPrivateApi`) | Tauri |

## 15. Validation (critères du cahier des charges)

L'état courant, mesuré, est tenu dans
[`docs/PLAYBACK_VALIDATION.md`](docs/PLAYBACK_VALIDATION.md) (corpus synthétique
généré par `tools/gen-test-media.sh` + procédure pour médias réels). Les
critères qui dépendent de matériel absent du poste de test (AVR HDMI, écran HDR
actif, macOS, Linux) y sont listés comme **non vérifiés** avec la procédure pour
les vérifier.

## 16. Phases

| Phase | État |
|---|---|
| 1. Architecture + spikes | ✅ ce document, spikes présentation/audio mesurés |
| 2. Jellyfin | ✅ auth (mot de passe, Quick Connect), catalogue, PlaybackInfo/DeviceProfile, flux, rapports, segments, admin — testé sur 12.1 |
| 3. Plex | ✅ PIN plex.tv, découverte + classement des connexions, hubs, décision MDE, part directe, timeline, marqueurs, admin — testé sur PMS 1.43 (PIN non testable sans compte) |
| 4. Modèle unifié | ✅ `oneshot-core` |
| 5. UI bibliothèque | ✅ Home, bibliothèques (grille virtualisée), détail, recherche |
| 6. Moteur natif | ✅ Windows (composition + fenêtre enfant) ; 🟡 macOS/Linux conçus, non implémentés |
| 7. HDR / audio / hwdec / Direct Play | ✅ Windows : sondes, décision, réconciliation ; 🟡 passthrough HDR et HBR à valider sur matériel |
| 8. Lecteur premium | ✅ timeline interpolée, pistes, skip intro, épisode suivant, panneau technique |
| 9. Multi-serveurs | ✅ agrégation, fusion par IDs externes, délai par serveur |
| 10. Flick Frame | ✅ layout TV, clavier/manette/télécommande |
| 11. Réglages | ✅ 18 catégories, appliqués à chaud (aucun réglage factice) |
| 12. Admin | ✅ selon les droits |
| 13. Perf / cache | ✅ cache SQLite TTL, cache images LRU, virtualisation ; 🟡 préchargement intelligent à faire |

**Non fait / à faire** (honnêtement) : présentateurs macOS (`LayerRender`) et
Linux (`DedicatedWindow` + OSD natif) ; sondes de capacités macOS/Linux
(actuellement « inconnu », donc décisions conservatrices) ; téléchargements
hors-ligne ; changement automatique de fréquence d'écran ; profils Plex Home
(changement d'utilisateur) ; Live TV ; packaging/signature des installeurs.

L'ordre d'implémentation réel a remonté les phases 6–7 en *spikes* avant les
phases 2–5 parce que ce sont elles qui invalidaient potentiellement toute
l'architecture (présentation vidéo sous Tauri).
