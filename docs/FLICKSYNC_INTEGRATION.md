# FlickSync in Flick Desktop: what was built

Implementation notes for [`FLICKSYNC.md`](FLICKSYNC.md). The wire contract is FlickSync's own
`docs/protocol.md` (protocol version 1).

## Assessment (phase 1)

| Question | Answer in this repo |
|---|---|
| Player | One: `oneshot-player` (libmpv). Controlled by `PlayerCommand`s; position/speed/pause read live with `Player::live_timing()` (the 4 Hz snapshot is too coarse for sub-100 ms drift). |
| Media identity | `ItemRef = (local ServerId, provider key)`. A `MediaRef` is built from the server's `remote_id` + the item key; resolving it finds a connection of the same provider **and** server id. |
| Auth | Jellyfin/Plex tokens in the OS keychain. FlickSync needs a separate short-lived JWT, issued by a **Flick Server** (a hosted service that does not exist in this repo yet). |
| Networking | `reqwest` via `oneshot-net`; no WebSocket code existed → `tokio-tungstenite` (rustls, same stack). |
| UI | React + zustand; the room is a Rust-owned session, the UI only mirrors `useWatch().room`. |

## Architecture

```
UI (React)            useWatch store ◀── "flicksync" events
   │ invoke()
app/src/commands/flicksync.rs        thin commands
app/src/flicksync.rs                 Hub (client from settings, play routing, control interception)
   │                                 AppController (PlaybackController on oneshot-player + MediaRef resolution)
crates/flicksync (oneshot-flicksync)
   ├ protocol.rs   wire types, parsing, validation of untrusted data
   ├ sync.rs       SyncEngine: pure, injectable clock, no I/O      ← deterministic tests
   ├ room.rs       the one canonical RoomState + reducer
   ├ connection.rs REST, WebSocket, back-off, ws path validation
   ├ auth.rs       TokenProvider (Flick Server endpoint | local key), JWT minting, discovery
   └ client.rs     session task: reconnection, ping/RTT, reports, applies the engine to the controller
```

* The engine never touches the socket, the UI or the player; the player never knows FlickSync exists
  (`PlaybackController` is the only seam).
* **No feedback loops by construction.** Local player events are never forwarded. The only way a
  command leaves is `SyncEngine::outbound(origin, intent)`, which answers `None` for anything but
  `PlaybackOrigin::User`. While a room is active the UI's play/pause/seek/speed become room commands
  (`player_command` is intercepted in Rust, not applied locally); the canonical broadcast comes back to
  everyone including the sender.
* Drift: < 100 ms ignored, 100-500 ms ±2 % rate, 500-1500 ms ±5 %, ≥ 1500 ms hard seek (3 s
  cooldown, hysteresis). The room rate is the user's rate; the correction is a multiplier around it.
  A server `sync_correction` takes over the rate while it lasts.
* Monotonic clock for all elapsed maths; server wall-clock is only mapped through the lowest-RTT
  ping/pong sample.
* Host selects media. A guest pressing Play on a title gets a clear message; the host's Play on a title
  announces it (`select_media`) and plays it. Guests are asked by the app (`openPlayer` event) to open the
  player; their `play` call answers the pending load.

## Flick Server contract (assumed: it is not specified anywhere yet)

* `GET  {flick_server}/api/v1/flicksync` → `{ "enabled": true, "url": "https://sync…", "token_path": "api/v1/flicksync/token" }`
  (404 / `enabled:false` → feature hidden).
* `POST {flick_server}/api/v1/flicksync/token` with `{ user_id, display_name }` → `{ "token": "<JWT>" }`.

Both are isolated in `crates/flicksync/src/auth.rs` (`discover`, `EndpointTokenProvider`); change them there
when the Flick Server defines the real shape. Without a Flick Server, Settings › Watch Together accepts a
FlickSync address + signing key (`kid:server_id:secret`, kept in the OS keychain, minted into JWTs in Rust).

## Edge cases (§66), deterministic behaviour

| # | Case | Behaviour |
|---|---|---|
| 1 | Host never starts playback | Room stays `waiting`/paused; nothing is sent. |
| 2-3 | Join while playing / paused | `room_state` → load media → `resync_now` → seek to the derived canonical position, then play/pause per state. |
| 4 | Join while buffering | Engine is silent while buffering; on recovery it resyncs immediately (hard seek if ≥ 250 ms behind). |
| 5, 25 | Media not on my server | `MediaUnavailable` toast, stay connected, no crash; ids are never matched across servers/providers. |
| 6-8 | Repeated seeks, simultaneous seek/play | Server orders them; clients apply the highest sequence, lower ones are ignored. |
| 9 | Host pauses while a guest buffers | Guest keeps room state, resyncs to the canonical (paused) position when ready. |
| 10-11 | Disconnect during playback/seek | State `reconnecting`, player untouched, back-off 0.5/1/2/4/8/10 s ±20 %, fresh token each try, give up after 60 s (`unavailable`, offer rejoin). |
| 12 | Reconnect with stale sequence | `room_state` older than applied is ignored by the sequence guard (the engine keeps the newer one). |
| 13-14 | Host disconnects / reconnects | Server decides (`presence_changed`, `room_updated: host_changed`); permissions follow the latest `room_updated`. |
| 15 | Room destroyed | `room_closed` / close 4003 / 404 on reconnect → state cleared, back to lobby, toast. |
| 16 | FlickSync unavailable | Rest of Flick untouched; feature hidden or "not reachable". |
| 17 | Token expires | Fresh token before each connection; 401 → one refresh, then `authentication_failed`. |
| 18 | Non-host opens another title | Refused with the "only the host" message; room media unchanged. |
| 19 | Local speed change | Becomes a room rate command (or `CONTROL_DENIED` in host-only rooms). |
| 20 | Media ends | The protocol has no end-of-media message: nothing is sent, next episode is never started automatically in a room. |
| 21 | User closes the player | Stays in the room (`ready=false`); "Open Player" reloads and resyncs. |
| 22 | User closes Flick | Socket drops; the server keeps the seat 30 s. |
| 23-24 | Jellyfin ↔ Plex, same title on both | Identity is (provider, server_id, media_id): only a connection of exactly that provider and server resolves it. |
| 26 | Malformed data | Dropped, logged without content, connection kept; out-of-range numbers and non-id strings are rejected. |

## Not done / not verified

* **Not exercised against a real FlickSync server and a real mpv session**: protocol and sync are covered
  by unit tests and an in-process scripted server (`crates/flicksync/tests/session.rs`); the Tauri/UI wiring
  compiles, type-checks and builds, but the end-to-end watch party has not been run by hand.
* `flick://sync/<id>` links need a deep-link plugin the app does not have: joining is by room code.
* The new media is loaded by the existing `play` path, which starts playing before the engine pauses it
  (brief audio blip possible when joining a paused room).
* Thresholds and tick (400 ms) are defaults in `SyncConfig`, not yet tuned on real playback, and not exposed in Settings.
* Chat is in memory only (cleared on leave); no avatars (initials only), no per-participant buffering state
  (the protocol does not carry it).
