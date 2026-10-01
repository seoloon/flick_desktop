# FlickSync — Flick Desktop Integration

You are implementing the FlickSync client-side integration inside Flick Desktop.

This document describes how Flick Desktop should integrate with the FlickSync server.

The goal is NOT to implement the FlickSync server here.

The goal is to make Flick Desktop capable of:

- creating watch rooms
- joining watch rooms
- leaving rooms
- displaying room participants
- allowing the room host to choose media
- synchronizing playback between participants
- correcting playback drift
- handling play/pause/seek/rate changes
- reconnecting after network interruptions
- optionally providing room chat

Before modifying code, inspect the existing Flick Desktop architecture thoroughly and understand:

- application structure
- Tauri commands
- Rust backend
- frontend framework
- media player implementation
- playback state management
- Jellyfin integration
- Plex integration
- navigation
- settings
- server configuration
- authentication/session management
- existing WebSocket/networking code

Reuse existing abstractions wherever appropriate.

Do not create a second media playback system.

Do not create a second authentication system.

Do not duplicate existing Jellyfin/Plex abstractions.

---

# 1. Product concept

FlickSync is a server-side companion service for Flick.

It is separate from:

- Jellyfin
- Plex

It does NOT stream media.

It does NOT access the user's media server directly.

It only coordinates playback between Flick clients.

The architecture is:

    Jellyfin / Plex
          │
          │ media
          ▼
    Flick Desktop
          │
          │ playback state/events
          ▼
      FlickSync
          │
          │ synchronization events
          ▼
    Other Flick Desktop clients

Every Flick client continues to obtain and play media directly from its own configured Jellyfin/Plex server.

FlickSync only synchronizes the playback state.

---

# 2. User experience

The intended experience is:

    User opens Flick
          ↓
    User has configured a Flick Server
          ↓
    FlickSync becomes available
          ↓
    User clicks "Create room"
          ↓
    Room is created
          ↓
    User shares room code/link
          ↓
    Friends join
          ↓
    Host selects a movie/episode
          ↓
    All clients resolve the media locally
          ↓
    Host starts playback
          ↓
    Everyone watches together

The experience should feel native to Flick.

Users should not have to understand:

- WebSockets
- synchronization
- latency
- server timestamps
- drift correction
- playback clocks

---

# 3. Important architecture rule

Flick Desktop owns LOCAL MEDIA PLAYBACK.

FlickSync owns SHARED ROOM STATE.

Do not reverse these responsibilities.

FlickSync tells Flick:

    "The room is currently playing media X at approximately 42.31 seconds."

Flick then:

    resolves media X
    opens it locally
    plays/seeks/pauses it

FlickSync must never become the playback engine.

---

# 4. Existing Flick architecture

First inspect the existing repository.

Identify:

- how playback is currently represented
- how current position is read
- how play/pause is triggered
- how seeking works
- how playback rate works
- how media changes are handled
- how Jellyfin IDs are represented
- how Plex IDs are represented
- how server connections are represented
- how user authentication works
- where global application state lives
- where settings are persisted
- how navigation/UI panels are implemented

Do not make assumptions about the player.

Adapt the FlickSync integration to the actual playback architecture.

If there are multiple playback implementations, create a small abstraction layer rather than duplicating synchronization logic.

---

# 5. FlickSync client architecture

Create a dedicated FlickSync client/service.

Conceptually:

    FlickSyncClient
        │
        ├── connection
        ├── authentication
        ├── room state
        ├── participant state
        ├── protocol
        ├── synchronization
        └── reconnection

The synchronization engine must be independent from the UI.

The UI should consume state from the FlickSync client rather than directly manipulating the WebSocket.

Avoid:

    UI → WebSocket → playback

Prefer:

    UI
      ↓
    FlickSync state/actions
      ↓
    FlickSync client
      ↓
    synchronization engine
      ↓
    playback abstraction

---

# 6. Suggested Rust architecture

If Flick already has an appropriate networking architecture, integrate with it.

Otherwise, conceptually:

    src/
        flicksync/
            mod.rs
            client.rs
            connection.rs
            protocol.rs
            room.rs
            participant.rs
            sync.rs
            errors.rs
            state.rs

Do not blindly follow this structure if the existing project suggests something better.

The important separation is:

- protocol
- networking
- room state
- synchronization
- playback integration

---

# 7. Connection

Flick should connect to FlickSync using WebSocket.

Conceptually:

    wss://flicksync.example.com/api/v1/rooms/<room_id>/ws

The exact endpoint must follow the FlickSync server protocol.

Do not hard-code URLs.

The FlickSync server URL must be configurable through Flick's existing server configuration if appropriate.

---

# 8. Authentication

Do not introduce another username/password system.

Flick already knows who the user is through the Flick Server.

The FlickSync connection should use an authentication token issued by the Flick Server or the existing Flick authentication system.

The client should:

1. authenticate with Flick Server
2. obtain the appropriate FlickSync authorization
3. connect to FlickSync
4. authenticate the WebSocket
5. establish the session

Do not store authentication secrets in frontend JavaScript if the existing architecture allows secure handling in the Rust side.

Use the existing secure credential/session mechanisms.

---

# 9. FlickSync server discovery

Flick should know which FlickSync server to use.

Prefer a server configuration model such as:

    Flick Server
        └── FlickSync endpoint

rather than asking the user to manually configure a second unrelated server whenever possible.

For example:

    Flick Server:
        https://flick.example.com

    FlickSync:
        wss://sync.example.com

The exact model should follow the existing Flick Server architecture.

If Flick Server can expose the FlickSync endpoint automatically, prefer that.

The user experience should be:

    Connect Flick Server
        ↓
    FlickSync available automatically

This is a core product requirement.

---

# 10. Connection state

Expose a clear FlickSync connection state:

    disconnected
    connecting
    connected
    reconnecting
    authentication_failed
    unavailable
    error

The UI should never have to infer connection state from arbitrary WebSocket events.

---

# 11. Room state

Represent the current room explicitly.

Conceptually:

    RoomState {
        room_id
        host_id
        participants
        media
        playback
        sequence
        connection_state
    }

Do not let multiple UI components maintain independent copies of room state.

There should be one canonical client-side room state.

---

# 12. Creating a room

When the user clicks:

    "Create watch room"

Flick should:

1. authenticate if necessary
2. request room creation
3. connect to the room WebSocket
4. become host
5. receive canonical room state
6. display the room UI

The UI should immediately show:

- room code
- share option
- participants
- host status

Do not expose raw server IDs unless appropriate.

---

# 13. Joining a room

The user should be able to enter:

    room code

or use a shareable room link.

Example conceptually:

    flick://sync/<room-id>

If the existing application supports custom URL schemes, integrate with them.

Otherwise, implement room-code joining first.

Joining flow:

    enter code
        ↓
    validate
        ↓
    authenticate
        ↓
    connect
        ↓
    receive room state
        ↓
    resolve current media
        ↓
    synchronize playback

---

# 14. Host permissions

The host is the only person who can select the media.

If a non-host opens a movie or episode from their library, do NOT automatically replace the room media.

The UI should make the permission clear.

For example:

    "Only the host can choose what the room watches."

Playback control can be shared depending on the server's permission model.

The client must still enforce UI-level restrictions, but never rely on UI restrictions for security.

The server remains authoritative.

---

# 15. Media identity

FlickSync should not send arbitrary URLs.

When the host selects a movie/episode, Flick should send a media reference compatible with the FlickSync protocol.

The reference may contain:

    provider
    server_id
    media_id
    media_type
    season_id
    episode_id

Use the actual media model already present in Flick.

The important rule is:

    FlickSync identifies the media.
    Flick resolves the media.

---

# 16. Resolving shared media

When a room reports:

    media_selected

Flick should attempt to resolve that media through the user's own configured media server.

For example:

    room says:
        Jellyfin
        item_id = abc123

Flick:

    searches its configured Jellyfin server
        ↓
    finds local playable item
        ↓
    opens it in the existing player

Do NOT assume that another participant has the same internal database ID.

If the media cannot be found locally:

    show a clear error
    keep the user connected to the room
    do not crash
    do not destroy the room

For example:

    "This item isn't available on your connected server."

The other participants should continue normally.

---

# 17. Playback authority

The Flick client must distinguish between:

    local playback
    canonical room playback

Never blindly echo every local player event back to FlickSync.

This would create feedback loops.

Example bad behavior:

    server sends pause
        ↓
    client pauses
        ↓
    player emits pause
        ↓
    client sends pause to server
        ↓
    server broadcasts pause
        ↓
    ...

Avoid this.

Use event origin/source tracking.

For example:

    LocalUser
    RemoteSync
    System

Only appropriate events should be propagated to FlickSync.

---

# 18. Playback synchronization model

The server provides canonical playback state.

Conceptually:

    position
    playback_state
    playback_rate
    reference_timestamp
    sequence

The client calculates the expected current position.

If:

    position = 100.0
    rate = 1.0
    reference time = T

and the client receives the state at T + 500ms:

    expected position ≈ 100.5

Do not simply use the position contained in the packet as the current position when the room is playing.

---

# 19. Monotonic clock

Use a monotonic clock for elapsed-time calculations.

Do not use wall-clock time such as:

    SystemTime::now()

for calculating playback elapsed time if an appropriate monotonic clock is available.

The synchronization engine should use a monotonic time source.

This protects against:

- system clock changes
- NTP adjustments
- daylight saving changes
- manual clock changes

---

# 20. Network latency

The client should estimate RTT.

Use the FlickSync ping/pong mechanism.

Maintain something like:

    estimated_rtt

Optionally smooth it over several samples.

Do not react aggressively to one bad latency measurement.

Use the estimate when determining the target playback position.

---

# 21. Initial synchronization

When a participant joins:

1. receive room state
2. resolve media
3. load media
4. wait until the player is ready
5. calculate target position
6. seek to target position
7. apply playback rate
8. play/pause according to canonical state

Do not start playback before the media is sufficiently ready.

If the player needs time to load, the client should remain locally paused until it can synchronize.

---

# 22. Drift correction

Continuously compare:

    local_position

against:

    canonical_position

Do not constantly seek.

Use a correction strategy.

Conceptually:

    tiny drift
        ↓
    ignore

    small drift
        ↓
    temporarily adjust playback rate

    significant drift
        ↓
    stronger rate correction

    large drift
        ↓
    hard seek

Example initial thresholds:

    < 100ms
        no correction

    100–500ms
        subtle rate adjustment

    500–1500ms
        stronger correction

    > 1500ms
        hard seek

These values are starting points.

Make them configurable and tune them based on actual player behavior.

---

# 23. Smooth rate correction

Do not make corrections audible or visually annoying.

For example, if local playback is:

    +300ms ahead

temporarily use:

    0.98x

instead of immediately seeking backwards.

If:

    -300ms behind

temporarily use:

    1.02x

Then smoothly return to:

    1.0x

once synchronized.

Do not permanently alter the user's selected playback rate.

Separate:

    user playback rate
    synchronization correction

For example:

    user_rate = 1.5

and:

    sync_multiplier = 0.98

effective rate:

    1.5 * 0.98

The exact implementation depends on the existing player.

---

# 24. User playback rate

The user's chosen playback rate remains authoritative as a user preference.

For example:

    1.5x

Synchronization correction should operate around it.

Do not overwrite:

    1.5x → 1.0x

simply because synchronization begins.

---

# 25. Play

When an authorized user presses play:

    local player
        ↓
    FlickSync playback_play
        ↓
    server canonical state
        ↓
    all clients

The initiating client should not wait unnecessarily for the server before providing responsive local UI.

However, the canonical server response must eventually become authoritative.

Avoid noticeable double-play events.

---

# 26. Pause

When an authorized user pauses:

    local position
        ↓
    send pause(position)
        ↓
    server canonical pause
        ↓
    all clients pause

All clients should converge to the canonical position.

---

# 27. Seek

Seeking is a room-level state change.

When the user seeks:

    user seeks to 540.25s
        ↓
    client sends playback_seek
        ↓
    server validates
        ↓
    server increments sequence
        ↓
    server broadcasts canonical state
        ↓
    clients seek

The client should not treat its local seek as final until the canonical room state is received.

The UX should still remain responsive.

---

# 28. Sequence numbers

Every room playback state update contains a monotonically increasing sequence number.

Example:

    sequence = 41
    sequence = 42
    sequence = 43

The client must track the latest accepted sequence.

If it receives:

    sequence = 42

after:

    sequence = 43

ignore it.

Never apply stale playback states.

This is essential for handling:

- latency
- packet reordering
- reconnects
- rapid seeks
- simultaneous actions

---

# 29. Remote events

Remote events should not be interpreted as local user actions.

For example:

    server -> playback_pause
        ↓
    client -> pause player
        ↓
    player emits local pause event
        ↓
    DO NOT send another pause command automatically

Use a synchronization guard or event origin system.

---

# 30. Preventing feedback loops

Implement a robust mechanism.

Possible model:

    PlaybackEventOrigin:
        User
        RemoteSync
        InternalCorrection

When the player changes because of:

    RemoteSync

do not send that change back to FlickSync.

When the player changes because of:

    User

send the appropriate room event.

When the player changes because of:

    InternalCorrection

do not treat it as a user command.

This distinction is critical.

---

# 31. Media transitions

If the host selects a different movie/episode:

    server:
        media_selected(sequence = N)

All clients:

    stop current playback
    resolve new media
    load new media
    synchronize
    wait for canonical play/pause state

Do not assume the new media is immediately available.

The UI should display a loading state.

---

# 32. TV episodes

The system must support:

- movies
- TV episodes

An episode should contain enough identity information for Flick to resolve it.

Do not model an entire series as the playback media.

The current episode is the media item.

---

# 33. Room UI

Integrate the room into Flick's existing design system.

Do not create a completely separate visual language.

The room UI should expose:

- room name/code
- host
- participants
- current media
- connection state
- chat if enabled
- leave room
- host controls

Avoid covering the entire playback UI with synchronization controls.

The feature should feel like a natural part of Flick.

---

# 34. Participants

Display:

    participant avatar
    participant name
    host indicator
    connection state

Do not expose unnecessary technical information.

Potential states:

    Watching
    Paused
    Buffering
    Connecting

These are UI states and should be derived from actual client state.

---

# 35. Chat

If chat is enabled by the server:

Provide a small integrated room chat.

Requirements:

- messages
- sender
- timestamp
- unread indicator
- send message
- rate-limit errors

Do not make chat dominate the watch experience.

The user should primarily be watching the movie.

---

# 36. Reconnection

WebSockets will disconnect.

Implement:

    connected
        ↓
    connection lost
        ↓
    reconnecting
        ↓
    reconnect
        ↓
    authenticate
        ↓
    rejoin room
        ↓
    receive canonical state
        ↓
    resynchronize

Use exponential backoff.

Example:

    1s
    2s
    4s
    8s
    16s

with a reasonable maximum.

Add jitter to avoid many clients reconnecting simultaneously.

---

# 37. Reconnection UX

Do not immediately kick the user out of the room because of a temporary network failure.

Show:

    "Reconnecting…"

If reconnection succeeds:

    continue watching

If reconnection fails permanently:

    show an actionable error.

The room itself may still exist.

---

# 38. Application restart

Do not automatically restore a room after application restart unless the existing Flick architecture makes this reliable.

If implementing restoration:

    restore session
        ↓
    reconnect
        ↓
    validate room
        ↓
    synchronize

Do not restore stale room state blindly.

---

# 39. Leaving a room

When the user chooses:

    Leave room

send the leave operation.

Then:

- disconnect WebSocket
- clear room state
- stop synchronization
- remove room-specific UI state

Do not accidentally leave the room when the user merely navigates away from the room UI.

The room session and the UI screen should be separate concepts.

---

# 40. Host leaving

If the server transfers host ownership:

The client must handle:

    host_changed

Update UI immediately.

If the current user becomes host:

    enable host controls

If they lose host status:

    disable host controls

Never rely on stale local state for permissions.

---

# 41. Server errors

Handle protocol errors gracefully.

Examples:

    NOT_HOST
    ROOM_NOT_FOUND
    ROOM_FULL
    INVALID_MEDIA
    UNAUTHORIZED
    AUTHENTICATION_FAILED
    RATE_LIMITED
    INVALID_STATE
    PROTOCOL_ERROR

Map technical errors to user-friendly UI messages.

Do not display raw Rust errors or JSON payloads to users.

---

# 42. FlickSync unavailable

If FlickSync is unavailable:

Flick itself must remain fully functional.

Users must still be able to:

- browse Jellyfin
- browse Plex
- play media
- pause
- seek
- use Flick normally

FlickSync must be an optional capability.

Never make the entire player dependent on FlickSync.

---

# 43. Feature availability

If the configured Flick Server does not provide FlickSync:

    hide or disable the feature gracefully

Do not show a broken room interface.

If FlickSync is available:

    expose the room functionality.

The exact UI should follow the existing Flick product design.

---

# 44. Local playback abstraction

If the existing player does not already expose a clean interface, create one.

Conceptually:

    PlaybackController

    get_position()
    get_state()
    get_rate()

    play()
    pause()
    seek(position)
    set_rate(rate)

    load_media(media)
    unload_media()

The actual implementation should use Flick's existing player.

Do not create a second player.

---

# 45. Synchronization service

Create a dedicated synchronization component.

Conceptually:

    SyncEngine

Responsibilities:

- receive canonical room state
- calculate expected position
- compare local position
- calculate drift
- decide correction
- apply correction
- restore user playback rate
- prevent feedback loops

It should NOT:

- manage WebSockets directly
- render UI
- know about React/Svelte/etc.
- manage room creation
- authenticate users

Keep responsibilities separate.

---

# 46. Synchronization tick

Do not run an unnecessarily expensive synchronization loop.

A reasonable model could be:

    periodic sync evaluation
        every ~250–500ms

combined with:

    immediate correction on
        play
        pause
        seek
        media change
        reconnect

Tune this based on actual player behavior.

Do not send network packets every synchronization tick unless the protocol requires it.

The client can calculate expected position locally.

---

# 47. Local state vs server state

Clearly distinguish:

    Server state
    Local playback state
    Derived synchronization state

For example:

    Server:
        canonical position = 100.0
        state = playing

    Local:
        player position = 100.25
        state = playing

    Derived:
        drift = +250ms
        correction = 0.98x

Do not mix these values into one mutable structure.

---

# 48. Buffering

Buffering is not the same as pausing.

If a participant buffers:

    local player stops temporarily

but:

    canonical room state continues

Once buffering ends:

    calculate current canonical position
    resynchronize
    continue playback

Do not send a global pause merely because one client buffers.

This is important.

---

# 49. End of media

When playback reaches the end:

Do not automatically change the room state unless the protocol explicitly requires it.

Prefer:

    client detects local end
        ↓
    reports playback ended
        ↓
    server determines room behavior

This is particularly important for TV episodes.

The server may later support:

    auto-play next episode

but do not implement this implicitly.

---

# 50. Client capabilities

During connection, the client may report capabilities.

For example:

    protocol_version
    player_features
    supported_playback_rates

This can allow the server/client protocol to evolve.

Do not make capability negotiation unnecessarily complex for V1.

---

# 51. Protocol versioning

The client must include the supported FlickSync protocol version.

If the server requires an incompatible version:

    fail gracefully

Display:

    "Your Flick version is not compatible with this FlickSync server."

Do not silently behave incorrectly.

---

# 52. Security

Never trust:

- room IDs
- media IDs
- participant names
- server URLs
- protocol messages
- playback positions
- playback rates

Validate all server data before using it.

Do not allow FlickSync messages to cause:

- arbitrary filesystem access
- arbitrary URLs to be opened
- command execution
- arbitrary server requests

Do not turn room metadata into a general command channel.

---

# 53. Privacy

FlickSync should expose as little information as possible.

Do not send to the server:

- media library contents
- Jellyfin/Plex credentials
- media server passwords
- unnecessary metadata

Only send what is required for room synchronization.

For media identity, use the minimal identifier needed to let the receiving Flick client resolve the media.

---

# 54. Persistence

Do not persist room state locally unless necessary.

Room state is server-authoritative.

Local persistent data may include:

- FlickSync server configuration
- last selected server
- user preference for enabling/disabling FlickSync

Do not persist:

- unnecessary room history
- chat history unless explicitly required
- playback history solely for synchronization

---

# 55. Testing

Add tests for the integration.

At minimum:

### Protocol

- valid messages
- invalid messages
- unknown events
- protocol version mismatch

### Room

- create
- join
- leave
- host transfer
- room closure

### Playback

- play
- pause
- seek
- rate change
- media change

### Synchronization

Test:

    0ms drift
    50ms drift
    100ms drift
    300ms drift
    500ms drift
    1000ms drift
    2000ms drift

Test both:

    client ahead
    client behind

### Reconnection

Test:

    disconnect
    reconnect
    stale state
    room destroyed
    authentication failure

### Feedback prevention

Explicitly test that:

    remote pause
        does NOT
    generate another pause command

and:

    internal drift correction
        does NOT
    generate a user playback event.

---

# 56. Testability

The synchronization engine should be deterministic where possible.

Avoid tests that depend on:

    sleep(500ms)

unless absolutely necessary.

Prefer injectable clocks/timers or deterministic time progression.

This is especially important for drift calculations.

---

# 57. Debugging tools

Provide a developer/debug mode if Flick already has one.

Useful information:

    FlickSync connection
    room ID
    participant ID
    RTT
    canonical position
    local position
    drift
    correction rate
    latest sequence
    last server event

This information should NOT appear in normal production UI.

It is extremely useful when diagnosing synchronization problems.

---

# 58. Logging

Use the existing Flick logging system.

Log important events:

- connection
- authentication
- room creation
- room join
- room leave
- reconnect
- synchronization correction
- protocol errors

Do not log:

- tokens
- passwords
- unnecessary media metadata
- chat contents by default

---

# 59. Performance

FlickSync must not negatively affect normal playback.

Synchronization work should be lightweight.

Do not:

- perform expensive work every frame
- allocate unnecessarily every synchronization tick
- perform network requests from the UI thread
- block the player thread
- make the frontend responsible for networking if the Rust backend already handles it

The playback experience remains the highest priority.

---

# 60. UI architecture

Before adding UI, inspect Flick's existing navigation and design system.

Use existing:

- buttons
- dialogs
- sheets
- avatars
- typography
- icons
- animations
- spacing
- colors

Do not introduce a new component library solely for FlickSync.

The room experience should look like it belongs to Flick.

---

# 61. Suggested UX

Potential entry points:

### Media page

    [ Play ]
    [ Add to Watch Room ]

### Sidebar / navigation

    Watch Together

### Room

    ┌──────────────────────────────┐
    │ FlickSync                    │
    │                              │
    │ Room ABC123                  │
    │                              │
    │ Antoine          Host        │
    │ Alex             Watching    │
    │ Sam              Paused      │
    │                              │
    │ Now playing                  │
    │ Movie title                  │
    │                              │
    │ [ Chat ]      [ Leave ]      │
    └──────────────────────────────┘

The exact UI should follow the existing Flick design.

Do not implement this exact layout if it conflicts with the current application.

---

# 62. Share links

If Flick supports custom URL schemes:

    flick://sync/<room_id>

Optionally support:

    https://flick.example/sync/<room_id>

The HTTP link should ultimately allow the user to open Flick if installed.

Do not build a complex web application for room joining.

---

# 63. Server configuration UX

Because FlickSync is likely a capability exposed through Flick Server, the user should ideally not need to understand that FlickSync is a separate service.

Conceptually:

    Flick Server
        ↓
    FlickSync enabled
        ↓
    Flick client sees:
        "Watch together"

If manual configuration is necessary, keep it in:

    Settings → Servers → Flick Server → FlickSync

Avoid creating:

    Settings → FlickSync → Server URL

unless required by the architecture.

---

# 64. Important product principle

FlickSync should be optional.

Without FlickSync:

    Flick works normally.

With FlickSync:

    Flick gains watch-party functionality.

This means the integration must be modular.

Avoid coupling:

    MediaPlayer ↔ FlickSync

Instead:

    MediaPlayer
         ↑
    Sync Adapter
         ↑
    FlickSync Client

---

# 65. Implementation strategy

Implement incrementally.

### Phase 1 — Understand Flick

Inspect:

- architecture
- playback
- authentication
- server configuration
- navigation
- state management

Do not modify code yet unless necessary.

### Phase 2 — FlickSync protocol client

Implement:

- protocol models
- serialization
- WebSocket connection
- authentication
- connection state

### Phase 3 — Room management

Implement:

- create
- join
- leave
- room state
- participants
- host state

### Phase 4 — Playback integration

Implement:

- media identity
- media resolution
- play
- pause
- seek
- playback rate

### Phase 5 — Synchronization engine

Implement:

- canonical position calculation
- RTT
- drift calculation
- correction
- sequence handling
- feedback-loop prevention

### Phase 6 — Reconnection

Implement:

- backoff
- session restoration
- room rejoin
- resynchronization

### Phase 7 — UI

Implement:

- room creation
- room join
- room view
- participants
- host controls
- connection state
- leave room

### Phase 8 — Chat

Implement only if the FlickSync protocol supports it.

### Phase 9 — Testing and hardening

Run:

    cargo test
    cargo fmt
    cargo clippy

and the existing Flick frontend/build/test commands.

Fix regressions before continuing.

---

# 66. Critical edge cases

Explicitly handle:

1. Host creates room but never starts playback.
2. Participant joins while playback is already running.
3. Participant joins while paused.
4. Participant joins while media is buffering.
5. Participant does not have the selected media.
6. Host seeks repeatedly.
7. Two users seek simultaneously.
8. Two users press play simultaneously.
9. Host pauses while another participant is buffering.
10. Network disconnect during playback.
11. Network disconnect during seek.
12. WebSocket reconnects with stale sequence.
13. Host disconnects.
14. Host reconnects.
15. Room is destroyed while client is connected.
16. FlickSync server becomes unavailable.
17. FlickSync authentication expires.
18. User changes media locally when they are not host.
19. User changes playback rate locally.
20. Media ends.
21. User closes the player.
22. User closes Flick.
23. User switches between Jellyfin and Plex.
24. Same media exists on multiple providers.
25. Media exists remotely but not locally.
26. Server sends malformed data.

Do not leave these as TODOs.

Define deterministic behavior.

---

# 67. Avoid overengineering

Do NOT:

- create a Redux-like state architecture if Flick does not already use one
- add Redis
- add a database
- add another authentication service
- create a second media player
- create a generic event bus unless genuinely useful
- introduce unnecessary microservices
- create a web frontend for FlickSync
- implement server-side media streaming
- implement permanent watch-party history

FlickSync is a feature, not a second product inside Flick.

---

# 68. Definition of done

The Flick Desktop integration is complete when:

- Flick can detect FlickSync availability
- user can create a room
- user can share/join a room
- participants appear
- host is clearly identified
- host can select a movie
- host can select an episode
- other clients resolve the media locally
- playback starts consistently
- pause propagates
- seek propagates
- playback rate propagates
- drift is automatically corrected
- correction is smooth
- buffering does not pause the entire room
- remote events do not create feedback loops
- stale sequence numbers are ignored
- reconnection works
- host transfer works
- leaving works
- room closure works
- Flick continues working perfectly when FlickSync is unavailable
- errors are handled gracefully
- tests cover the synchronization engine
- no credentials are leaked
- no unnecessary media data is sent to FlickSync

---

# 69. Final principle

Think of FlickSync as a synchronization layer sitting between Flick's existing player and a remote room.

The architecture should conceptually be:

                  ┌───────────────────────┐
                  │       Flick UI        │
                  └───────────┬───────────┘
                              │
                              ▼
                  ┌───────────────────────┐
                  │   FlickSync Client    │
                  │                       │
                  │ Room / WebSocket      │
                  │ Protocol              │
                  │ Reconnection          │
                  └───────────┬───────────┘
                              │
                              ▼
                  ┌───────────────────────┐
                  │    Sync Engine        │
                  │                       │
                  │ Canonical state       │
                  │ Drift calculation     │
                  │ Correction            │
                  └───────────┬───────────┘
                              │
                              ▼
                  ┌───────────────────────┐
                  │ Playback Adapter      │
                  └───────────┬───────────┘
                              │
                              ▼
                  ┌───────────────────────┐
                  │ Existing Flick Player │
                  └───────────────────────┘

The server is authoritative for the shared room state.

The client is authoritative for local media playback.

The two responsibilities must remain separate.

The user should experience none of this complexity.

It should simply feel like:

    "We're watching this together."

Before writing the implementation, inspect the repository and provide a concise technical assessment of how FlickSync should integrate with the existing Flick Desktop architecture.

Then implement the integration incrementally, testing each major component before moving to the next one.