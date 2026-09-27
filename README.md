# Flick

Client desktop Jellyfin + Plex, natif (Rust/Tauri), avec un vrai moteur de
lecture (libmpv) : Direct Play réel, décodage GPU, HDR, passthrough audio,
multi-serveurs, mode TV « Flick Frame ».

- **Pourquoi ces choix, et ce qui est garanti ou non** → [`ARCHITECTURE.md`](ARCHITECTURE.md)
- **Ce qui a été mesuré, sur quel matériel** → [`docs/PLAYBACK_VALIDATION.md`](docs/PLAYBACK_VALIDATION.md)
- **Design system** → [`docs/DESIGN_SYSTEM.md`](docs/DESIGN_SYSTEM.md)

## Prérequis

- Rust ≥ 1.85, Node ≥ 20, pnpm
- **libmpv ≥ 0.41 (build LGPL)**, chargée à l'exécution. Sous Windows elle est
  téléchargée automatiquement au premier `pnpm desktop` ; sous macOS/Linux la
  libmpv du système est utilisée (`brew install mpv`, `libmpv2`…), ou
  `ONESHOT_LIBMPV=/chemin/vers/libmpv`.

## Commandes (depuis la racine)

```sh
pnpm install        # une fois
pnpm desktop        # tauri dev : Vite + l'application, rechargement à chaud
pnpm build          # tauri build : installeur de production
pnpm build:win      # installeur Windows x64
pnpm build:mac      # app macOS universelle + DMG, signée ad hoc (voir docs/MACOS_PORT.md)
pnpm test           # tests Rust + tests et typecheck de l'UI
pnpm lint           # clippy + typecheck
pnpm bindings       # régénère les types TypeScript depuis Rust (ts-rs)
```

## Outils de diagnostic (optionnels)

```sh
cargo run -p oneshot-capabilities --example report     # écrans/HDR, audio/passthrough, décodeurs GPU
bash tools/gen-test-media.sh                           # corpus de test (ffmpeg) → test-media/
cargo run -p oneshot-mpv --example probe -- test-media/*   # ce que mpv fait réellement de chaque fichier
```

Tests d'intégration contre **votre** serveur (ignorés sinon) :
`ONESHOT_JELLYFIN_URL=http://…:8096 cargo test -p oneshot-jellyfin --test live`
(compte `oneshot`/`oneshot` attendu) et `ONESHOT_PLEX_URL=http://…:32400 cargo test -p oneshot-plex --test live`
(serveur non réclamé accessible sans token).

## Organisation

```
crates/core           modèle de domaine, capacités, contrats (aucune I/O)
crates/mpv            FFI libmpv (chargement dynamique)
crates/capabilities   sondes OS (DXGI/CCD HDR, D3D11 décodeurs, WASAPI IEC 61937)
crates/playback       moteur de décision (pur) + profil client
crates/player         session de lecture, présentation vidéo, réconciliation, rapports
crates/catalog        multi-serveurs : agrégation, fusion, cache
crates/storage        réglages, trousseau OS, cache SQLite, cache images
crates/net            politique HTTP commune
crates/providers/*    Jellyfin, Plex
app/                  shell Tauri (commandes, protocole images, fenêtre)
ui/                   React 19 + TypeScript strict (Tailwind v4, Motion, Norigin)
```

## Licence

MPL-2.0 pour le code du projet. Distribuer avec une libmpv **LGPL** : une
build GPL de libmpv imposerait la GPL à l'application entière.
