# Projet — Client Jellyfin + Plex natif en Rust/Tauri

Je veux développer une application desktop moderne, premium et performante qui combine Jellyfin et Plex dans un seul client natif.

Le projet doit être pensé comme une véritable application multimédia desktop, pas comme un site web encapsulé dans Tauri.

---

## 1. Vision du produit

Créer un client unifié permettant de connecter plusieurs serveurs :

- Jellyfin
- Plex
- plusieurs serveurs simultanément
- plusieurs comptes / profils selon les capacités des plateformes
- découverte et agrégation des bibliothèques dans une interface unique

L'application doit donner la sensation d'utiliser un produit multimédia premium proche d'Apple TV / tvOS, avec l'ergonomie moderne de la nouvelle interface Plex.

La priorité absolue est :

1. qualité et fiabilité de lecture vidéo
2. performances
3. fluidité de l'interface
4. qualité UX/UI
5. architecture propre et maintenable

Ne cherche pas à faire un prototype superficiel. Je veux une base sérieuse pouvant devenir une vraie application distribuable.

---

# 2. Stack

Utiliser :

- Rust
- Tauri
- TypeScript
- frontend moderne dans le WebView uniquement pour l'UI
- backend / logique critique en Rust

Privilégier une architecture modulaire et fortement typée.

Éviter de faire reposer les fonctionnalités critiques sur JavaScript lorsque Rust peut les gérer.

Ne pas transformer l'application en simple wrapper web.

---

# 3. RÈGLE ABSOLUE : moteur de lecture natif

C'est le point le plus important du projet.

L'application NE DOIT PAS se comporter comme un navigateur vis-à-vis de Jellyfin ou Plex.

Je ne veux PAS d'une architecture basée uniquement sur :

- HTML5 `<video>`
- codecs disponibles dans Chromium/WebView
- limitations du navigateur
- simple lecture HTTP dans le WebView

L'application doit utiliser un véritable moteur multimédia natif.

Avant d'implémenter la lecture, analyse les différentes solutions réalistes pour Rust/Tauri, par exemple :

- libmpv / mpv
- FFmpeg
- GStreamer
- autres backends natifs pertinents
- APIs natives de décodage / affichage selon l'OS

Compare-les selon :

- HDR
- HDR10
- Dolby Vision lorsque techniquement possible
- HEVC/H.265
- H.264
- AV1
- VP9
- 4K
- 8K lorsque supporté
- fréquence d'image élevée
- audio PCM
- FLAC
- AAC
- AC3
- E-AC3
- DTS
- DTS-HD
- TrueHD
- Dolby Atmos
- formats multicanaux 5.1
- 7.1
- 7.1.4 et autres configurations lorsque le matériel/OS le permettent
- passthrough audio
- accélération matérielle GPU
- synchronisation audio/vidéo
- sous-titres
- changement de piste
- HDR / SDR handling
- consommation CPU/GPU
- Windows / macOS / Linux

Je veux que tu choisisses une architecture réellement adaptée à cet objectif et que tu expliques ce choix dans la documentation technique.

---

# 4. Direct Play / Direct Stream / Transcoding

Le client doit comprendre les capacités du matériel et du système local.

L'objectif est de privilégier :

### Direct Play
Le fichier est lu directement sans modification du média.

### Direct Stream / Remux
Utiliser uniquement lorsque nécessaire et sans perte de qualité.

### Transcoding serveur
Ne doit PAS être utilisé par défaut simplement parce que le client est une application Tauri.

Le client doit essayer de gérer lui-même les formats qu'il est capable de décoder.

Par exemple :

- utilisateur avec écran HDR -> lecture HDR
- utilisateur sans HDR -> conversion / tone mapping local si nécessaire
- utilisateur avec système audio Atmos compatible -> conserver le format adapté
- utilisateur avec simple stéréo -> downmix local propre
- utilisateur avec 5.1 -> conserver 5.1
- utilisateur avec 7.1 -> conserver 7.1

L'application doit détecter les capacités de :

- GPU
- écran
- HDR
- système audio
- nombre de canaux
- codecs disponibles
- accélération matérielle
- APIs de sortie audio
- OS

et construire automatiquement une stratégie de lecture.

IMPORTANT :
ne promets jamais une capacité matériel/OS que l'application ne peut techniquement garantir.
Si un format nécessite une capacité spécifique non disponible, documenter clairement la limitation et appliquer la meilleure stratégie locale possible.

---

# 5. Client Jellyfin

Implémenter un véritable client Jellyfin avec API native.

Fonctionnalités :

- connexion serveur
- URL personnalisée
- authentification
- API key / token lorsque pertinent
- utilisateurs
- profils
- bibliothèques
- films
- séries
- saisons
- épisodes
- épisodes suivants
- historique
- reprise de lecture
- favoris
- collections
- playlists
- genres
- années
- acteurs
- réalisateurs
- recherche
- recommandations
- contenu récemment ajouté
- contenu récemment regardé
- téléchargements lorsque l'API / serveur le permettent
- synchronisation de la progression
- gestion des métadonnées
- images / posters / backdrops
- subtitles
- audio tracks
- video tracks

Utiliser au maximum les APIs officielles.

---

# 6. Client Plex

Même logique pour Plex.

Fonctionnalités :

- connexion Plex
- authentification
- serveur local / distant
- bibliothèques
- films
- séries
- épisodes
- saisons
- collections
- playlists
- historique
- reprise
- recherche
- recommandations
- métadonnées
- posters
- backdrops
- acteurs
- sous-titres
- pistes audio
- pistes vidéo
- lecture
- suivi de progression

Utiliser les APIs / mécanismes Plex appropriés.

Ne pas supposer que Plex et Jellyfin fonctionnent exactement de la même façon :
créer une couche d'abstraction commune.

---

# 7. Architecture multi-serveurs

Je veux pouvoir ajouter par exemple :

- Jellyfin serveur A
- Jellyfin serveur B
- Plex serveur C
- Plex serveur D

L'utilisateur doit voir les serveurs comme une source unifiée.

Créer un système de providers / adapters.

Exemple conceptuel :

Provider
├── JellyfinProvider
├── PlexProvider
└── future providers

Normaliser les entités :

- Media
- Movie
- Series
- Season
- Episode
- Collection
- Person
- Library
- Server
- User
- PlaybackSession

L'UI ne doit pas avoir à savoir si un élément vient de Plex ou Jellyfin.

---

# 8. Interface

Je veux une interface très moderne inspirée de :

- nouvelle interface Plex
- Apple TV / tvOS
- applications multimédia premium
- Steam Big Picture pour certains aspects

La capture fournie avec ce prompt sert de référence visuelle.

Inspiration visuelle :

- grandes images / backdrops
- navigation horizontale
- hiérarchie très claire
- éléments cinématiques
- beaucoup d'espace
- interface sombre
- typographie moderne
- animations discrètes
- transitions fluides
- profondeur
- blur / glass uniquement lorsqu'il améliore réellement la lisibilité
- absence de surcharge visuelle

Ne copie pas littéralement Plex.
Utilise ses principes comme inspiration et construis une identité propre.

---

# 9. Background adaptatif

Une fonctionnalité importante :

Le background de l'interface doit pouvoir s'adapter automatiquement au média actuellement sélectionné ou survolé.

Exemple :

l'utilisateur sélectionne une cover de film.

L'application :

1. récupère l'artwork / backdrop
2. extrait une palette dominante
3. génère un fond dynamique
4. applique éventuellement :
    - gradient
    - blur
    - vignette
    - diffusion de couleur
    - contraste adaptatif

Le résultat doit rester très subtil.

Le background ne doit jamais rendre le texte illisible.

La transition doit être fluide et quasiment imperceptible.

Éviter les effets flashy.

---

# 10. Home

Créer un dashboard multimédia moderne.

Exemple :

Hero principal
→ film / série actuellement sélectionné

Puis :

- Continue Watching
- Recently Added
- Most Watched
- Recommended
- Trending / Popular
- My Libraries
- Collections
- Live TV
- éventuellement playlists

Les sections doivent être dynamiques selon les bibliothèques réellement disponibles.

---

# 11. Navigation

Navigation principale simple.

Exemple conceptuel :

Home
Movies
TV Shows
Libraries
Collections
Search
Live TV
Downloads
Servers
Settings

Mais adapte cette structure aux données disponibles.

La navigation doit pouvoir devenir une navigation "TV-style" dans le mode Maxi Frame.

---

# 12. Mode "Maxi Frame"

Créer un mode spécial inspiré de Steam Big Picture.

Nom : `Maxi Frame`

L'idée :

l'application devient presque un environnement multimédia à part entière.

Lorsque Maxi Frame est activé :

- fullscreen complet
- suppression maximale des éléments desktop inutiles
- navigation pensée pour écran TV
- gros éléments
- focus visible
- navigation clavier
- navigation manette
- navigation télécommande si techniquement possible
- raccourcis simples
- animations plus cinématiques
- UI lisible à plusieurs mètres
- interfaces pensées pour 1080p / 4K TV

Le mode normal et Maxi Frame doivent partager le même système de données mais avoir des layouts / interactions adaptés.

Prévoir un système de navigation par focus.

Exemple :

Arrow Keys / D-Pad
→ navigation

Enter / A
→ ouvrir

Escape / B
→ retour

Space
→ lecture/pause

---

# 13. Lecteur vidéo

Créer un lecteur extrêmement travaillé.

Pas juste un player basique.

Fonctionnalités :

- lecture
- pause
- seek
- skip +/- 10s
- avance rapide
- retour
- volume
- mute
- fullscreen
- changement piste audio
- changement de sous-titres
- désactivation des sous-titres
- sélection qualité si pertinent
- sélection vidéo
- informations techniques
- synchronisation
- reprise automatique
- next episode
- previous episode
- autoplay
- skip intro lorsque disponible
- timeline très fluide

Ajouter une interface inspirée des lecteurs TV premium.

L'interface du lecteur doit disparaître automatiquement.

---

# 14. Media information overlay

Prévoir un panneau permettant de voir les informations techniques du média.

Exemples :

Video
- 4K
- HEVC
- HDR10
- Dolby Vision
- bitrate
- resolution
- FPS

Audio
- French
- English
- 5.1
- 7.1
- Atmos
- codec
- bitrate

Subtitles
- French
- English
- forced
- SDH

Et afficher également la stratégie utilisée :

- Direct Play
- Direct Stream
- Local Decode
- Server Transcode

Cela doit être compréhensible pour un utilisateur normal mais peut aussi avoir un mode "Advanced".

---

# 15. Détection des capacités

Créer un Capability Manager.

Il doit pouvoir déterminer :

DisplayCapabilities
- resolution
- refresh rate
- HDR
- color depth lorsque accessible
- color space lorsque accessible

AudioCapabilities
- channel count
- supported formats
- passthrough
- device

VideoCapabilities
- codecs
- hardware acceleration
- decoder availability

SystemCapabilities
- OS
- GPU
- memory
- CPU

Puis :

PlaybackDecisionEngine

qui décide comment lire chaque média.

Architecture souhaitée :

Media
↓
Capability Manager
↓
Playback Decision Engine
↓
Native Media Backend
↓
Video Output / Audio Output

---

# 16. Paramètres ultra complets

Créer une vraie section Settings.

Catégories :

General
Appearance
Playback
Audio
Video
Subtitles
HDR
Downloads
Servers
Accounts
Network
Cache
Performance
Keyboard
Controller
Notifications
Privacy
Advanced
Debug

Exemples :

Playback
- default playback quality
- direct play
- direct stream
- transcoding policy
- resume behavior
- autoplay
- skip intro

Video
- hardware acceleration
- decoder
- deinterlacing
- frame synchronization
- refresh rate behavior
- HDR behavior
- tone mapping

Audio
- output device
- channels
- passthrough
- normalization
- downmixing
- exclusive mode

Subtitles
- font
- size
- color
- background
- outline
- position
- default language
- forced subtitles

Network
- concurrent requests
- buffering
- cache
- proxy
- timeout
- IPv4 / IPv6
- custom DNS settings where technically relevant

Appearance
- theme
- accent color
- animation intensity
- background intensity
- blur
- compact / comfortable UI

Advanced
- logs
- diagnostics
- player backend
- developer tools
- experimental features

---

# 17. Admin panel

Je veux également un accès aux fonctionnalités d'administration lorsque les droits / APIs du serveur le permettent.

Créer un espace :

Server Management

permettant selon la plateforme :

- informations serveur
- utilisateurs
- bibliothèques
- tâches
- sessions actives
- lecteurs connectés
- logs
- plugins / extensions lorsque supporté
- configuration serveur lorsque l'API le permet
- gestion de certains paramètres

IMPORTANT :
ne jamais contourner les permissions du serveur.

L'interface doit simplement exposer les fonctionnalités accessibles avec les droits de l'utilisateur.

---

# 18. Performance

L'application doit être très rapide.

Objectifs :

- démarrage rapide
- animations fluides
- navigation instantanée
- virtualisation des grandes listes
- cache intelligent
- chargement progressif des images
- préchargement intelligent
- faible consommation mémoire
- peu de requêtes réseau inutiles
- parallélisation Rust lorsque pertinent

Les images ne doivent pas être toutes chargées simultanément en résolution maximale.

Prévoir :

- thumbnail
- poster
- backdrop
- full resolution

selon le contexte.

---

# 19. Cache local

Créer un cache local structuré.

Stocker :

- métadonnées
- images
- historique
- préférences
- informations serveurs
- session
- état de lecture

Mais attention à la synchronisation.

Les données du serveur restent la source de vérité.

Prévoir invalidation / TTL / refresh intelligent.

---

# 20. Sécurité

Les credentials doivent être stockés de manière sécurisée.

Ne jamais stocker de mot de passe en clair.

Utiliser les mécanismes sécurisés natifs du système lorsque possible :

- Windows Credential Manager
- macOS Keychain
- Secret Service / équivalent Linux

Les tokens doivent être protégés.

---

# 21. Design system

Créer un véritable design system.

Définir :

- spacing
- typography
- radii
- shadows
- blur
- transitions
- animation timing
- icons
- buttons
- cards
- dialogs
- menus
- tabs
- sliders
- toggles

Le design doit être cohérent partout.

---

# 22. Animations

Animations très smooth.

Privilégier :

- fade
- translate subtil
- scale subtil
- blur transition
- crossfade des backgrounds
- shared element transitions lorsque pertinent

Éviter :

- animations longues
- effets exagérés
- UI qui bouge constamment

L'animation doit donner une sensation de qualité, pas distraire.

---

# 23. Responsive desktop

L'interface doit fonctionner sur :

- 1080p
- 1440p
- 4K
- écrans ultrawide
- TV

Prévoir différentes densités selon la taille d'écran.

---

# 24. Architecture du projet

Avant de coder :

1. analyse les contraintes Jellyfin / Plex
2. analyse les contraintes Tauri
3. analyse les moteurs multimédias natifs
4. propose une architecture
5. explique les trade-offs
6. définis les modules
7. définis les interfaces
8. définis les structures de données
9. définis la stratégie de lecture
10. seulement ensuite commence l'implémentation

Je veux un document ARCHITECTURE.md expliquant cela.

---

# 25. Architecture souhaitée

Quelque chose dans cet esprit :

src/
├── core/
│   ├── media/
│   ├── playback/
│   ├── capabilities/
│   ├── servers/
│   ├── accounts/
│   ├── cache/
│   └── search/
│
├── providers/
│   ├── jellyfin/
│   └── plex/
│
├── player/
│   ├── backend/
│   ├── renderer/
│   ├── audio/
│   ├── video/
│   ├── subtitles/
│   └── playback_engine/
│
├── ui/
│   ├── home/
│   ├── library/
│   ├── search/
│   ├── player/
│   ├── settings/
│   ├── servers/
│   └── maxi_frame/
│
└── infrastructure/

L'organisation exacte peut être différente si tu proposes mieux.

---

# 26. Qualité du code

Je veux :

- Rust idiomatique
- TypeScript strict
- types forts
- architecture modulaire
- tests
- gestion propre des erreurs
- logs structurés
- aucune dette technique volontaire inutile
- pas de gros fichier monolithique
- pas de duplication excessive
- pas de hacks temporaires qui deviennent définitifs

Chaque abstraction doit avoir une vraie raison d'exister.

---

# 27. Observabilité

Créer un système de diagnostics.

Logger :

- connexion serveur
- requêtes importantes
- erreurs API
- choix de playback
- codec
- accélération matérielle
- stratégie HDR
- sortie audio
- événements du player

Prévoir un panneau Debug permettant de consulter ces informations.

---

# 28. Priorité d'implémentation

Ne développe pas 50 fonctionnalités superficielles avant d'avoir une lecture parfaite.

Ordre recommandé :

### Phase 1
Architecture

### Phase 2
Connexion Jellyfin

### Phase 3
Connexion Plex

### Phase 4
Modèle de données unifié

### Phase 5
Bibliothèque UI

### Phase 6
Moteur de lecture natif

### Phase 7
HDR / audio / hardware acceleration / Direct Play

### Phase 8
Player premium

### Phase 9
Multi-serveurs

### Phase 10
Maxi Frame

### Phase 11
Settings avancés

### Phase 12
Admin

### Phase 13
Performance / cache / polishing

---

# 29. Critères de validation importants

Avant de considérer le projet comme fonctionnel, vérifier concrètement :

- un fichier H.264 peut être lu
- un fichier HEVC peut être lu
- un fichier 4K peut être lu
- un fichier HDR peut être lu lorsque le système le permet
- un fichier 5.1 est réellement envoyé / décodé en 5.1
- un fichier 7.1 est correctement géré
- les formats Atmos compatibles sont correctement gérés lorsque l'OS et le matériel le permettent
- les sous-titres fonctionnent
- plusieurs pistes audio fonctionnent
- plusieurs pistes vidéo fonctionnent lorsque disponibles
- la progression remonte au serveur
- la reprise fonctionne
- Direct Play est réellement utilisé lorsqu'il est possible
- le serveur n'est pas forcé à transcoder à cause du client
- l'application reste fluide pendant la lecture
- la navigation reste réactive

Créer également des diagnostics permettant de vérifier pourquoi une lecture utilise Direct Play, Direct Stream ou Transcoding.

---

# 30. Très important : ne pas faire semblant

Si une fonctionnalité est limitée par :

- Windows
- macOS
- Linux
- Tauri
- WebView
- GPU
- API Plex
- API Jellyfin
- DRM
- Dolby
- HDMI
- pilotes

ne masque pas la limitation.

Documente-la.

Ne construis pas une fausse abstraction qui prétend supporter quelque chose alors que ce n'est pas réellement le cas.

Je préfère une architecture techniquement honnête avec certaines limitations documentées plutôt qu'une application qui affiche "Atmos / HDR / Direct Play" sans réellement assurer ces fonctionnalités.

---

# 31. Résultat attendu

Je veux à terme une application ressemblant à un mélange de :

Plex moderne
+
Apple TV
+
un vrai lecteur multimédia desktop natif
+
Steam Big Picture pour Maxi Frame

mais avec une identité propre.

Le produit doit donner une sensation de :

- premium
- rapide
- cinématique
- fluide
- sobre
- moderne
- technologiquement sérieux

Commence par analyser le projet et produire l'architecture technique complète avant de générer une grosse quantité de code.