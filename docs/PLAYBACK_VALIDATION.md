# Validation de la lecture

État mesuré, pas supposé. Chaque ligne indique **comment** elle a été vérifiée.
Ce qui n'a pas pu l'être sur le poste de test est listé comme tel, avec la
procédure pour le faire.

## Poste de test

| | |
|---|---|
| OS | Windows 11 Pro 26200 |
| GPU | NVIDIA GeForce RTX 5060 (pilote 32.0.16.1656) + AMD Radeon iGPU |
| Écrans | Samsung Odyssey G5 2560×1440 @ 120 Hz — **HDR capable, HDR Windows désactivé** ; ViewSonic VX229 1080p |
| Audio | Focusrite USB (stéréo, défaut) · Realtek USB **S/PDIF optique** · HDMI AMD vers moniteur · NVIDIA Broadcast (virtuel) |
| libmpv | build LGPL `zhongfly/mpv-winbuild` 2026-09-26, client API 2.5 |
| Serveurs | **Jellyfin 12.1.0** et **Plex Media Server 1.43.4** réels (conteneurs Docker, `tools/dev-*.sh`) |

## Corpus

`tools/gen-test-media.sh` génère 8 fichiers synthétiques (mire + un ton
distinct par canal audio pour vérifier le mapping) :

| # | Vidéo | Audio | Autres |
|---|---|---|---|
| 01 | H.264 High 1080p23.976 | AAC 2.0 | MP4 |
| 02 | **HEVC Main10 2160p, PQ/BT.2020 + HDR10 (MaxCLL 1000)** | E-AC3 5.1 | |
| 03 | HEVC 1080p | FLAC 7.1, TrueHD 5.1, AC3 5.1 (fr) | SRT (en), **ASS forcé** (fr) |
| 04 | AV1 10-bit 1080p | FLAC 5.1 | |
| 05 | VP9 1080p | DTS 5.1 | |
| 06 | **2 pistes vidéo** H.264 | AAC | |
| 07 | H.264 **2160p60** | AC3 5.1 | |
| 08 | H.264 7 min | AAC | pour la reprise (les serveurs ignorent les points de reprise < 5 min) |

⚠️ Limites du corpus : le « HDR » est une mire SDR étiquetée PQ (valide la
**signalisation**, pas la qualité d'image). Le codeur TrueHD de FFmpeg ne sait
pas produire du 7.1 (il repasse silencieusement en 5.1) et ses codeurs
E-AC3/DTS plafonnent à 5.1 : **TrueHD 7.1, DTS-HD MA, Atmos et Dolby Vision
doivent être validés avec de vrais fichiers** (dossier `test-media-real/`,
ignoré par git).

## Résultats

### Décodage et rendu (moteur seul — `cargo run -p oneshot-mpv --example probe`)

| Fichier | hwdec observé | Vidéo entrée → sortie | Audio entrée → sortie (Focusrite stéréo) |
|---|---|---|---|
| 01 H.264 | **d3d11va** | bt.709/bt.1886 → idem | stereo → stereo |
| 02 HEVC10 HDR10 4K | **d3d11va** (p010) | **bt.2020/pq, MaxCLL 1000 → bt.709/gamma2.2** (tone mapping, HDR Windows off) | 5.1 → stereo (downmix) |
| 03 HEVC | d3d11va | SDR | FLAC 7.1 → stereo |
| 04 AV1 10-bit | **d3d11va** | SDR | FLAC 5.1 → stereo |
| 05 VP9 | d3d11va | SDR | DTS 5.1 → stereo |
| 06 multi-vidéo | d3d11va | 2 pistes vidéo listées | |
| 07 H.264 4K60 | d3d11va | 60 fps ; 7 images perdues sur les 2,5 premières secondes (inclut la création de la fenêtre) — à re-mesurer sur une lecture longue | AC3 5.1 → stereo |

CPU mesuré pour 4K HEVC10 HDR (tone mapping inclus) dans l'app Tauri :
**≈ 0,07 s de CPU par seconde** pour tout le processus.

### Passthrough (bitstream)

| Périphérique | AC3 | DTS | E-AC3 | Sonde `IsFormatSupported` (notre Capability Manager) |
|---|---|---|---|---|
| Realtek S/PDIF | ✅ `spdif-ac3` | ✅ `spdif-dts` | ❌ **échec AO** | {AC3, DTS} — **identique** |
| HDMI moniteur (AMD) | ❌ échec AO | ❌ | ❌ | {} — **identique** |

➡ Découverte clé : quand un format bitstream imposé est refusé, **mpv ne se
replie pas sur le PCM**. D'où : sonde par périphérique, `audio-spdif` restreint
au seul format décidé, et repli à chaud (`ao-reload` sans passthrough) si le
récepteur refuse malgré le pilote.

### Chaîne complète (serveur réel → décision → mpv → rapport) — `cargo run -p oneshot-player --example e2e`

| Vérification | Jellyfin 12.1 | Plex 1.43 |
|---|---|---|
| Direct Play accordé par le serveur avec notre profil (8/8 fichiers) | ✅ 0 raison de transcodage | ✅ (`mdeDecisionCode` 1000, `Part.decision=directplay`) |
| Flux HTTP avec auth en **en-têtes** (pas de token dans l'URL) | ✅ | ✅ |
| Décodage GPU sur les 8 fichiers | ✅ d3d11va | ✅ d3d11va |
| Métadonnées HDR du serveur | ❌ **Jellyfin annonce SDR** pour le fichier PQ/BT.2020 | ✅ `colorTrc=smpte2084` → HDR10 |
| Réconciliation à l'exécution | ✅ plan corrigé en *tone mapping* (« metadata-mismatch ») | n/a (déjà correct) |
| Pistes multiples (3 audio, 2 sous-titres, 2 vidéo) exposées | ✅ | ✅ |
| Reprise : démarrage à 60 s respecté | ✅ | ✅ |
| Progression : seek 150 s + stop → point de reprise serveur | ✅ **151,8 s** | ✅ **151,8 s** |
| Vu / non vu | ✅ | ✅ (scrobble) |
| Favoris | ✅ | ❌ n'existe pas dans Plex → `Unsupported` (pas simulé) |
| Admin (infos, sessions, tâches) | ✅ | ✅ |

### Application (Tauri + WebView2 + présentation composition)

| Vérification | Statut |
|---|---|
| Vidéo sous la WebView transparente, mélange alpha correct (mode composition **et** fenêtre enfant) | ✅ captures `PrintWindow` |
| `backdrop-filter` au-dessus de la vidéo | ❌ ne floute pas (attendu) → voile translucide |
| Home agrégée Jellyfin + Plex, Continue Watching fusionné | ✅ |
| Images via `oneshot-img://` (aucun token dans le DOM) | ✅ |
| Navigation clavier spatiale (rangées, mémoire de focus) | ✅ via CDP, bug « saut de rangée » corrigé + test |
| Panneau technique : décision + mesures mpv + options appliquées | ✅ (« Local Decode : 7.1 mixed down to Stereo », d3d11va, presenter composition) |
| Flick Frame (mise en page TV) | ✅ (testé sans plein écran pour ne pas perturber le poste) |

## Critères du cahier des charges (§29)

| Critère | Statut | Preuve |
|---|---|---|
| H.264 lu | ✅ | probe + e2e + app |
| HEVC lu | ✅ | idem |
| 4K lu | ✅ | 02 (HEVC10) et 07 (H.264 60 fps) |
| HDR lu « lorsque le système le permet » | 🟡 partiel | tone mapping HDR→SDR ✅ ; **passthrough HDR non vérifié** : l'écran est HDR mais le HDR Windows était désactivé (réglage utilisateur non modifié par nos soins). Procédure ci-dessous. |
| 5.1 réellement envoyé en 5.1 | 🟡 | le moteur décide et configure `audio-channels=5.1` (tests unitaires) ; sortie physique 5.1 non vérifiable : aucun périphérique multicanal PCM sur le poste |
| 7.1 correctement géré | 🟡 | FLAC 7.1 décodé (s32 7.1), downmix stéréo vérifié ; sortie 7.1 physique non vérifiable ici |
| Atmos quand OS/matériel le permettent | ❌ non vérifiable ici | aucun récepteur Atmos HDMI ; la décision (bitstream TrueHD/E-AC3 si sondé, sinon « Atmos non reproduit ») est couverte par tests |
| Sous-titres | ✅ | SRT + ASS forcé listés/sélectionnables, sélection par préférences testée |
| Plusieurs pistes audio | ✅ | fichier 03 |
| Plusieurs pistes vidéo | ✅ | fichier 06 |
| Progression remonte au serveur | ✅ | 151,8 s sur les deux serveurs |
| Reprise | ✅ | démarrage à 60 s |
| Direct Play réellement utilisé | ✅ | `DeliveryRequest::Direct` → URL `static=true` / part originale |
| Serveur non forcé à transcoder par le client | ✅ | 0 `TranscodeReasons` sur 8/8 fichiers, deux serveurs |
| App fluide pendant la lecture | ✅ | CPU ≈ 7 % d'un cœur en 4K HDR ; UI animée indépendante du rendu vidéo |
| Diagnostics « pourquoi Direct Play / Stream / Transcode » | ✅ | panneau Avancé + Debug (raisons codées) |

## Procédures restantes (matériel requis)

1. **HDR passthrough Windows** : activer *Paramètres > Affichage > Utiliser le
   HDR*, lancer le fichier 02 (ou un vrai HDR10). Attendu dans le panneau
   Avancé : *Picture* = « HDR10 sent to the display », *Video out* =
   `bt.2020 / pq`. Le mode composition impose `target-colorspace-hint=yes`,
   `target-trc=pq`, `target-peak=<nits de l'écran>` (mpv ne peut pas lire
   l'écran lui-même dans ce mode).
2. **Bitstream HDMI HBR** (TrueHD/DTS-HD/Atmos) : PC → HDMI → AVR compatible.
   Vérifier la sonde (*Settings > Audio*, « Accepted by this device ») puis
   *Audio out* = `spdif-truehd` et l'indicateur Atmos de l'AVR.
3. **PCM 5.1/7.1** : régler Windows en 5.1/7.1 (Configuration des
   haut-parleurs) ; le fichier 03 contient un ton différent par canal.
4. **Dolby Vision P5/P8** et **TrueHD 7.1 réel** : fichiers réels dans
   `test-media-real/`.
5. **Linux** : non construit ni testé (voir ARCHITECTURE.md §4 et §14).
6. **macOS** : voir la section suivante pour ce qui est mesuré et ce qui reste
   à faire sur du matériel (HDR EDR/XDR, ampli HDMI/optique, Mac Intel).

## macOS (Apple silicon)

Section ajoutée le 2026-09-30. Même corpus (`tools/gen-test-media.sh`, dossier
`test-media/`). Chaque ligne dit comment elle a été vérifiée ; ce qui ne l'a pas
été est marqué comme tel.

### Poste de test

| | |
|---|---|
| OS | macOS 26 (Darwin 25.6), MacBook Air **Apple M1** |
| Écran | intégré (pas de XDR/mini-LED), aucun écran externe branché |
| Audio | Haut-parleurs MacBook Air (stéréo, 48 kHz) ; aucun périphérique HDMI, optique ou USB |
| libmpv | 0.41.0 (Homebrew), la même que celle embarquée dans l'app (`third_party/mpv/macos-arm64`) |
| App | `pnpm build:mac`, DMG ad hoc, lancée sur ce Mac et sur un second Mac Apple silicon |

### Décodage, conversion locale (mpv 0.41, `vo=gpu-next`, `--hwdec=auto-safe`)

Mesuré avec le binaire `mpv` de Homebrew piloté par IPC (l'exemple
`oneshot-mpv --example probe` ouvre sa propre fenêtre et échoue sur macOS avec
« no NSApplication initialized » ; la mesure passe donc par le binaire mpv, dont
la libmpv est identique). Options d'audio comme le moteur du lecteur : `audio-channels=stereo`.

| Fichier | hwdec observé | Vidéo entrée → cible d'affichage | Audio entrée → sortie (haut-parleurs stéréo) |
|---|---|---|---|
| 01 H.264 | **videotoolbox** | bt.709/bt.1886 → idem | stereo → stereo |
| 02 HEVC10 HDR10 4K | **videotoolbox** | **bt.2020/pq, MaxCLL 1000 → bt.709/gamma2.2** (tone mapping) | 5.1 → stereo (downmix) |
| 03 HEVC | videotoolbox | SDR | FLAC 7.1 → stereo (downmix) |
| 04 AV1 10-bit | **aucun (logiciel, dav1d)** : le M1 n'a pas de décodeur AV1 matériel | SDR | FLAC 5.1 → stereo |
| 05 VP9 | videotoolbox | SDR | DTS 5.1 → stereo |
| 07 H.264 4K60 | videotoolbox | 60 fps, 0 image perdue sur 4,5 s | AC3 5.1 → stereo |

- **Conversion HDR → SDR : validée.** Même résultat avec les options SDR de
  l'app (`target-colorspace-hint=no`, `target-trc=auto`…) : cible bt.709/gamma2.2,
  pic 1.0, contre un contenu PQ de pic 4,93 (≈ 1000 nits). `video-out-params`
  n'est pas la bonne propriété pour ça (c'est la sortie des filtres) : il faut
  `video-target-params`.
- **Downmix 5.1/7.1 → stéréo : validé** pour E-AC3, FLAC, DTS et AC3, sur la vraie
  sortie CoreAudio.
- **Sondes cohérentes avec mpv** : VideoToolbox annonce H.264 ✅, HEVC ✅, AV1 ❌,
  VP9 ❌ sur ce M1, et mpv ne décode effectivement pas l'AV1 en matériel. ⚠️ Pour le
  VP9, mpv utilise bien `videotoolbox` alors que `VTIsHardwareDecodeSupported`
  répond faux (décodage VT logiciel d'Apple) : la sonde est conservatrice, le
  réglage « transcoder sans décodage matériel » verrait donc du VP9 comme non
  accéléré.
- Non mesuré ici : Dolby Vision, HDR10 réel (voir plus bas).

### Passthrough (bitstream)

| Périphérique | Sonde (`crates/capabilities/src/macos/audio.rs`) | mpv `--audio-spdif=ac3 --audio-exclusive=yes` |
|---|---|---|
| Haut-parleurs MacBook Air | `{}` (aucun format physique compressé) | ❌ `coreaudio_exclusive`: « No usable substream found » — **identique** |

- La sonde reproduit le critère de `ao_coreaudio_exclusive` : un flux dont les
  formats physiques incluent `ac-3` / `cac3`. Avec un tel flux : {AC3, DTS} ;
  {AC3, DTS, E-AC3} si un format à 192 kHz (4x) existe. **TrueHD, DTS-HD et
  Atmos ne sont jamais annoncés** (CoreAudio n'a pas de HBR).
- Comme sous Windows, quand mpv refuse un format imposé il **ne se replie pas sur
  le PCM** (il tente même un autre driver) : le garde-fou reste la sonde par
  périphérique plus le repli à chaud du lecteur.
- ❌ **Non vérifié, matériel absent** : le cas positif (AVR/TV en HDMI, DAC
  optique) — la logique est couverte par des tests unitaires (`formats_from_rates`)
  mais aucune lecture bitstream réelle n'a été faite sur macOS.

### Application (Tauri + WKWebView + `CAOpenGLLayer`)

| Vérification | Statut |
|---|---|
| Lancement depuis le DMG ad hoc, libmpv embarquée chargée (entitlements hardened runtime) | ✅ deux Mac Apple silicon |
| Vidéo sous l'UI, lecture, fenêtre transparente | ✅ à l'œil (utilisateur) |
| PiP : entrée, sortie et restauration de la fenêtre maximisée | ✅ mesuré (2880×1740 restauré à l'identique) après correctif |
| Sous-titres, Flick Frame plein écran + intro, profils/PIN, favoris, TMDB/Trousseau, manette, Cmd+C/V | ⚠️ non vérifiés méthodiquement (« tout a l'air de marcher ») |
| DMG sur un Mac « propre » (Gatekeeper) | 🟡 lancé sur un second Mac ; procédure clic droit › Ouvrir non documentée pas à pas |

### Procédures restantes (macOS)

1. **HDR10 réel** sur un écran EDR/XDR : nécessite un calque géré en couleur
   (`CAOpenGLLayer` PQ, pixel format flottant), puis retirer la rétrogradation
   `Active → SupportedButOff` de `crates/capabilities/src/macos/mod.rs`.
2. **Bitstream** : Mac → HDMI → AVR (ou DAC optique) ; vérifier la sonde dans
   Réglages › Audio, puis *Audio out* = `spdif-ac3`/`spdif-dts`.
3. **PCM 5.1/7.1** : régler l'ampli/écran en multicanal (Configuration Audio
   MIDI) ; le fichier 03 a un ton par canal.
4. **Mac Intel / universel** : libmpv x86_64, HEVC Main10 non déclaré par la sonde.

## AirPlay : conversion locale (ffmpeg)

Un titre que le récepteur ne lit pas tel quel (MKV, audio DTS, AV1, H.264
entrelacé ou 10 bits, sous-titre incrusté) est converti sur l'ordinateur en
HLS fMP4 par un ffmpeg embarqué. Voir `TECHNICAL.md`, « AirPlay conversion ».

**Mesuré** (macOS Apple silicon, ffmpeg 9.0.2 embarqué dans
`third_party/mpv/macos-arm64`) :

- `cargo test -p oneshot-core -p oneshot-cast` : 48 + 29 tests verts, dont le
  planificateur (10), la ligne de commande ffmpeg (8), la recherche de ffmpeg,
  le relais de dossier (plages d'octets, `..`, `%2f`, `\`), la règle de seek.
- `a_real_clip_gets_a_playlist_and_the_process_dies_with_the_job` et
  `a_bad_input_fails_with_the_conversion_code_and_no_hang` passent avec
  `ONESHOT_FFMPEG` pointant sur le binaire embarqué : les dylibs relocalisées
  se chargent, `h264_videotoolbox`, `libx264` et `aac` sont présents.
- `cargo check -p Flick` et `tsc --noEmit` : propres.
- Aucun processus `ffmpeg` ni dossier `flick-airplay-*` restant après les tests.

**Limite connue** : le ffmpeg de Homebrew est compilé sans libass, donc sans le
filtre `subtitles`. Un sous-titre *texte* (SRT/ASS) à incruster fait échouer la
conversion (`FLK-CAST-016`) ; les sous-titres bitmap (PGS, VobSub) passent par
`overlay` et ne sont pas concernés. Le script d'embarquement le signale. Il faut
un ffmpeg avec libass (par ex. le tap `homebrew-ffmpeg/ffmpeg`) pour lever la limite.

**Non vérifié (matériel requis)** :

1. Lecture réelle sur un récepteur AirPlay (Apple TV, TV) d'un MKV : démarrage,
   pause/reprise, scrub dans la partie produite (instantané), scrub loin devant
   (redémarrage, quelques secondes de « Buffering… »).
2. Arrêt du cast ou fermeture de l'app : plus de `ffmpeg` dans `pgrep -fl ffmpeg`
   ni de dossier `flick-airplay-*` dans le dossier temporaire (garanti par
   `kill_on_drop` et `TempDir`, mais pas observé sur une vraie session).
3. Build Windows : `tools/fetch-ffmpeg.ps1`, la ressource `libmpv/ffmpeg.exe` et
   `CREATE_NO_WINDOW` n'ont pas été exécutés (pas de machine Windows).
4. Linux : ffmpeg système, non essayé.
