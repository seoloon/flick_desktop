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
5. **macOS / Linux** : non construits ni testés dans cette session (voir
   ARCHITECTURE.md §4 et §14).
