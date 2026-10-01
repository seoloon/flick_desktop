//! The FlickSync client: owns the room session (REST + WebSocket +
//! reconnection), the canonical [`RoomState`] and the [`SyncEngine`], and
//! drives local playback through a [`PlaybackController`].
//!
//! ```text
//! UI ──actions──▶ FlickSyncClient ──▶ session task ──▶ SyncEngine ──▶ PlaybackController
//!    ◀──events───        (room state, WebSocket)       (pure maths)      (existing player)
//! ```
//!
//! The UI never touches the socket, and the player never knows FlickSync
//! exists: the controller is the only seam between them.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use oneshot_net::reqwest::Client;
use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use crate::auth::TokenProvider;
use crate::clock::Clock;
use crate::connection::{self, ConnectError, GIVE_UP_AFTER, JoinedRoom, Socket};
use crate::errors::{Error, Result, UserMessage, user_message_for_code};
use crate::protocol::{ClientMessage, ControlMode, MediaRef, ServerMessage, parse_server_message};
use crate::room::{Change, ConnectionState, RoomState};
use crate::sync::{Applied, Evaluation, LocalIntent, LocalPlayback, PlaybackOrigin, PlayerAction, SyncConfig, SyncEngine};

#[derive(Debug)]
pub enum LoadError {
    /// The media is not on one of the user's own servers.
    Unavailable,
    Failed(String),
}

/// The seam to the existing player. Implemented by the app on top of
/// `oneshot-player`; there is deliberately no second player in this crate.
#[async_trait]
pub trait PlaybackController: Send + Sync {
    /// What the player does right now. `ready` is false when the shared media
    /// is not the one loaded (or still loading).
    fn local(&self) -> LocalPlayback;
    fn play(&self);
    fn pause(&self);
    fn seek(&self, secs: f64);
    /// Effective speed (room rate × correction).
    fn set_rate(&self, rate: f64);
    /// Resolves the shared media on the user's own servers and starts loading it.
    async fn load_media(&self, media: &MediaRef) -> std::result::Result<(), LoadError>;
    fn unload(&self);
}

/// What the UI layer receives.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "type")]
pub enum ClientEvent {
    State {
        room: Box<RoomState>,
    },
    Notice {
        message: UserMessage,
    },
    MediaUnavailable {
        title: Option<String>,
    },
    /// The session ended: back to the lobby.
    Left {
        reason: Option<String>,
    },
}

pub type EventSink = Arc<dyn Fn(ClientEvent) + Send + Sync>;

#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DebugInfo {
    pub connection: ConnectionState,
    pub room_id: String,
    pub participant_id: String,
    pub rtt_ms: Option<f64>,
    pub canonical_position: Option<f64>,
    pub local_position: Option<f64>,
    pub drift_ms: f64,
    pub correction_rate: f64,
    pub sequence: u64,
    pub last_server_event: String,
}

enum Cmd {
    Send(ClientMessage),
    /// Load the room's media again (the user closed the player).
    Reload,
    Leave,
}

enum Internal {
    Loaded { generation: u64, result: std::result::Result<(), LoadError> },
}

struct Shared {
    http: Client,
    base_url: Url,
    tokens: Arc<dyn TokenProvider>,
    controller: Arc<dyn PlaybackController>,
    clock: Arc<dyn Clock>,
    sink: EventSink,
    state: Mutex<RoomState>,
    engine: Mutex<SyncEngine>,
    debug: Mutex<DebugInfo>,
    session: Mutex<Option<SessionHandle>>,
}

struct SessionHandle {
    tx: mpsc::UnboundedSender<Cmd>,
    task: JoinHandle<()>,
}

#[derive(Clone)]
pub struct FlickSyncClient {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for FlickSyncClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlickSyncClient").field("base_url", &self.shared.base_url.as_str()).finish_non_exhaustive()
    }
}

impl FlickSyncClient {
    pub fn new(
        http: Client,
        base_url: Url,
        tokens: Arc<dyn TokenProvider>,
        controller: Arc<dyn PlaybackController>,
        clock: Arc<dyn Clock>,
        cfg: SyncConfig,
        sink: EventSink,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                http,
                base_url,
                tokens,
                controller,
                clock,
                sink,
                state: Mutex::new(RoomState::default()),
                engine: Mutex::new(SyncEngine::new(cfg)),
                debug: Mutex::new(DebugInfo::default()),
                session: Mutex::new(None),
            }),
        }
    }

    pub fn state(&self) -> RoomState {
        self.shared.state.lock().clone()
    }

    pub fn in_room(&self) -> bool {
        self.shared.session.lock().is_some()
    }

    pub fn debug(&self) -> DebugInfo {
        let mut d = self.shared.debug.lock().clone();
        let s = self.shared.state.lock();
        d.connection = s.connection;
        d.room_id.clone_from(&s.room_id);
        d.participant_id.clone_from(&s.you);
        let e = self.shared.engine.lock();
        d.rtt_ms = e.clock.rtt_ms();
        d.sequence = e.last_sequence();
        d
    }

    /// Whether the FlickSync service answers (used to show or hide the feature).
    pub async fn available(&self) -> bool {
        connection::health(&self.shared.http, &self.shared.base_url).await
    }

    pub async fn create_room(&self, control_mode: ControlMode) -> Result<RoomState> {
        self.ensure_idle()?;
        let token = self.shared.tokens.token().await?;
        let joined = connection::create_room(&self.shared.http, &self.shared.base_url, &token, control_mode, true).await?;
        tracing::info!(target: "flicksync", "room created");
        Ok(self.start_session(joined))
    }

    pub async fn join_room(&self, code: &str) -> Result<RoomState> {
        self.ensure_idle()?;
        let token = self.shared.tokens.token().await?;
        let joined = connection::join_room(&self.shared.http, &self.shared.base_url, &token, code).await?;
        tracing::info!(target: "flicksync", "room joined");
        Ok(self.start_session(joined))
    }

    fn ensure_idle(&self) -> Result<()> {
        if self.in_room() { Err(Error::Server { code: "ALREADY_IN_ROOM".into() }) } else { Ok(()) }
    }

    fn start_session(&self, joined: JoinedRoom) -> RoomState {
        {
            let mut state = self.shared.state.lock();
            *state = RoomState::new(&joined.room_id, &joined.share_code, &joined.participant_id);
            state.connection = ConnectionState::Connecting;
        }
        self.shared.engine.lock().reset();
        let (tx, rx) = mpsc::unbounded_channel();
        let shared = Arc::clone(&self.shared);
        let task = tokio::spawn(async move {
            Session::new(Arc::clone(&shared), joined, rx).run().await;
            // The session is over for any reason: forget it so the user can start another.
            shared.session.lock().take();
        });
        *self.shared.session.lock() = Some(SessionHandle { tx, task });
        let snapshot = self.state();
        emit_state(&self.shared);
        snapshot
    }

    fn send_cmd(&self, cmd: Cmd) -> Result<()> {
        let guard = self.shared.session.lock();
        let handle = guard.as_ref().ok_or(Error::NoRoom)?;
        handle.tx.send(cmd).map_err(|_| Error::NoRoom)
    }

    fn require_connected(&self) -> Result<()> {
        match self.shared.state.lock().connection {
            ConnectionState::Connected => Ok(()),
            _ => Err(Error::Network("not connected".into())),
        }
    }

    /// Host only (UI-level check; the server is the authority).
    pub fn select_media(&self, media: MediaRef) -> Result<()> {
        media.validate()?;
        if !self.shared.state.lock().is_host() {
            return Err(Error::Server { code: "NOT_HOST".into() });
        }
        self.require_connected()?;
        self.send_cmd(Cmd::Send(ClientMessage::SelectMedia(media)))
    }

    /// A user action on the player. It becomes a room command and is *not*
    /// applied locally: the canonical broadcast comes back to us too.
    pub fn user_intent(&self, intent: LocalIntent) -> Result<()> {
        if !self.shared.state.lock().can_control_playback() {
            return Err(Error::Server { code: "CONTROL_DENIED".into() });
        }
        self.require_connected()?;
        let msg = self.shared.engine.lock().outbound(PlaybackOrigin::User, intent);
        match msg {
            Some(m) => self.send_cmd(Cmd::Send(m)),
            None => Ok(()),
        }
    }

    pub fn user_play(&self) -> Result<()> {
        self.user_intent(LocalIntent::Play)
    }

    pub fn user_pause(&self) -> Result<()> {
        let position = self.shared.controller.local().position;
        self.user_intent(LocalIntent::Pause { position })
    }

    pub fn user_seek(&self, position: f64) -> Result<()> {
        self.user_intent(LocalIntent::Seek { position: position.max(0.0) })
    }

    pub fn user_rate(&self, rate: f64) -> Result<()> {
        self.user_intent(LocalIntent::Rate { rate })
    }

    /// Re-opens the room's media (after the player was closed) and resynchronizes.
    pub fn resync_media(&self) -> Result<()> {
        self.send_cmd(Cmd::Reload)
    }

    pub fn send_chat(&self, text: &str) -> Result<()> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(());
        }
        self.require_connected()?;
        self.send_cmd(Cmd::Send(ClientMessage::Chat { text: text.chars().take(500).collect() }))
    }

    pub fn update_room(&self, control_mode: Option<ControlMode>, chat_enabled: Option<bool>) -> Result<()> {
        if !self.shared.state.lock().is_host() {
            return Err(Error::Server { code: "NOT_HOST".into() });
        }
        self.require_connected()?;
        self.send_cmd(Cmd::Send(ClientMessage::UpdateRoom { control_mode, chat_enabled }))
    }

    pub fn close_room(&self) -> Result<()> {
        if !self.shared.state.lock().is_host() {
            return Err(Error::Server { code: "NOT_HOST".into() });
        }
        self.send_cmd(Cmd::Send(ClientMessage::CloseRoom))
    }

    /// Leaves for good. Navigating away from the room screen must *not* call this.
    pub async fn leave(&self) {
        let handle = self.shared.session.lock().take();
        let Some(handle) = handle else { return };
        let _ = handle.tx.send(Cmd::Leave);
        // Give the session a moment to say goodbye, then make sure it is gone.
        let mut task = handle.task;
        if tokio::time::timeout(Duration::from_secs(2), &mut task).await.is_err() {
            task.abort();
        }
        finish(&self.shared, None);
    }
}

fn emit_state(shared: &Shared) {
    let room = Box::new(shared.state.lock().clone());
    (shared.sink)(ClientEvent::State { room });
}

fn notice(shared: &Shared, message: UserMessage) {
    (shared.sink)(ClientEvent::Notice { message });
}

fn set_connection(shared: &Shared, c: ConnectionState) {
    {
        let mut s = shared.state.lock();
        if s.connection == c {
            return;
        }
        s.connection = c;
    }
    emit_state(shared);
}

/// Clears everything room-specific and tells the UI to go back to the lobby.
fn finish(shared: &Shared, reason: Option<String>) {
    shared.engine.lock().reset();
    *shared.state.lock() = RoomState::default();
    *shared.debug.lock() = DebugInfo::default();
    // Whatever speed the room imposed, hand the player back at normal speed.
    shared.controller.set_rate(1.0);
    (shared.sink)(ClientEvent::Left { reason });
}

enum Ended {
    Left,
    Closed(Option<String>),
    Replaced,
    Refused,
    Dropped,
}

struct Session {
    shared: Arc<Shared>,
    joined: JoinedRoom,
    rx: mpsc::UnboundedReceiver<Cmd>,
    itx: mpsc::UnboundedSender<Internal>,
    irx: mpsc::UnboundedReceiver<Internal>,
    media: Option<MediaRef>,
    loaded: bool,
    generation: u64,
    next_ping_at: f64,
    pings_sent: u32,
    next_report_at: f64,
}

impl Session {
    fn new(shared: Arc<Shared>, joined: JoinedRoom, rx: mpsc::UnboundedReceiver<Cmd>) -> Self {
        let (itx, irx) = mpsc::unbounded_channel();
        Self {
            shared,
            joined,
            rx,
            itx,
            irx,
            media: None,
            loaded: false,
            generation: 0,
            next_ping_at: 0.0,
            pings_sent: 0,
            next_report_at: 0.0,
        }
    }

    async fn run(mut self) {
        let url = match connection::ws_url(&self.shared.base_url, &self.joined.ws_path) {
            Ok(u) => u,
            Err(e) => {
                tracing::warn!(target: "flicksync", "{e}");
                set_connection(&self.shared, ConnectionState::Error);
                notice(&self.shared, UserMessage::Generic);
                return finish(&self.shared, None);
            }
        };
        let mut attempt = 0u32;
        let mut down_since = std::time::Instant::now();
        let mut refreshed_after_401 = false;
        let mut first = true;
        loop {
            set_connection(&self.shared, if first { ConnectionState::Connecting } else { ConnectionState::Reconnecting });
            let outcome = match self.shared.tokens.token().await {
                // A fresh token before every (re)connection: they are short-lived.
                Ok(token) => connection::connect(&url, &token).await,
                Err(Error::Unauthenticated) => Err(ConnectError::Unauthenticated),
                Err(e) => Err(ConnectError::Retry(e.to_string())),
            };
            match outcome {
                Ok(socket) => {
                    attempt = 0;
                    refreshed_after_401 = false;
                    first = false;
                    set_connection(&self.shared, ConnectionState::Connected);
                    tracing::info!(target: "flicksync", "connected");
                    match self.run_socket(socket).await {
                        Ended::Left => return finish(&self.shared, None),
                        Ended::Closed(reason) => return finish(&self.shared, reason),
                        Ended::Replaced => {
                            notice(&self.shared, UserMessage::Generic);
                            return finish(&self.shared, Some("replaced".into()));
                        }
                        Ended::Refused => {
                            set_connection(&self.shared, ConnectionState::Error);
                            return finish(&self.shared, Some("refused".into()));
                        }
                        Ended::Dropped => {
                            tracing::info!(target: "flicksync", "connection lost");
                            down_since = std::time::Instant::now();
                        }
                    }
                }
                Err(ConnectError::Unauthenticated) if !refreshed_after_401 => {
                    refreshed_after_401 = true;
                    continue;
                }
                Err(ConnectError::Unauthenticated) => {
                    set_connection(&self.shared, ConnectionState::AuthenticationFailed);
                    notice(&self.shared, UserMessage::SessionExpired);
                    return finish(&self.shared, Some("authentication_failed".into()));
                }
                Err(ConnectError::RoomGone) => {
                    notice(&self.shared, UserMessage::RoomNotFound);
                    return finish(&self.shared, Some("not_found".into()));
                }
                Err(ConnectError::Retry(why)) => {
                    tracing::debug!(target: "flicksync", attempt, "connect failed: {why}");
                    if first && attempt >= 2 {
                        set_connection(&self.shared, ConnectionState::Unavailable);
                        notice(&self.shared, UserMessage::Unavailable);
                        return finish(&self.shared, Some("unavailable".into()));
                    }
                }
            }
            if down_since.elapsed() > GIVE_UP_AFTER {
                set_connection(&self.shared, ConnectionState::Unavailable);
                notice(&self.shared, UserMessage::Unavailable);
                return finish(&self.shared, Some("unavailable".into()));
            }
            let wait = connection::backoff_delay(attempt, connection::jitter());
            attempt = attempt.saturating_add(1);
            // A command during the wait is only honoured if it is a leave.
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                cmd = self.rx.recv() => match cmd {
                    Some(Cmd::Leave) | None => return finish(&self.shared, None),
                    Some(_) => {}
                },
            }
        }
    }

    fn local(&self) -> LocalPlayback {
        let mut l = self.shared.controller.local();
        l.ready &= self.loaded;
        l
    }

    async fn send(sink: &mut futures::stream::SplitSink<Socket, Message>, msg: &ClientMessage) -> bool {
        sink.send(Message::text(msg.to_json())).await.is_ok()
    }

    async fn run_socket(&mut self, socket: Socket) -> Ended {
        let (mut sink, mut stream) = socket.split();
        let cfg = *self.shared.engine.lock().config();
        let mut tick = tokio::time::interval(Duration::from_millis(cfg.tick_ms as u64));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let now = self.shared.clock.now_ms();
        self.next_ping_at = now;
        self.pings_sent = 0;
        self.next_report_at = now + cfg.report_interval_ms;
        loop {
            tokio::select! {
                frame = stream.next() => {
                    let Some(Ok(frame)) = frame else { return Ended::Dropped };
                    match frame {
                        Message::Text(text) => {
                            if let Some(ended) = self.on_text(&text, &mut sink).await {
                                return ended;
                            }
                        }
                        Message::Close(frame) => {
                            let code = frame.map_or(1006, |f| u16::from(f.code));
                            return match code {
                                4001 => Ended::Replaced,
                                4002 => Ended::Closed(Some("removed".into())),
                                4003 => Ended::Closed(self.shared.state.lock().closed_reason.clone().or(Some("closed".into()))),
                                1008 => Ended::Refused,
                                _ => Ended::Dropped,
                            };
                        }
                        // Pings are answered by the library; binary frames are not part of the protocol.
                        _ => {}
                    }
                }
                cmd = self.rx.recv() => match cmd {
                    Some(Cmd::Send(m)) => {
                        if !Self::send(&mut sink, &m).await { return Ended::Dropped; }
                    }
                    Some(Cmd::Reload) => {
                        if let Some(media) = self.media.clone() { self.begin_load(media); }
                    }
                    Some(Cmd::Leave) | None => {
                        let _ = Self::send(&mut sink, &ClientMessage::Leave).await;
                        let _ = sink.close().await;
                        return Ended::Left;
                    }
                },
                Some(Internal::Loaded { generation, result }) = self.irx.recv() => self.on_loaded(generation, result),
                _ = tick.tick() => {
                    let now = self.shared.clock.now_ms();
                    if now >= self.next_ping_at {
                        let rtt_ms = self.shared.engine.lock().clock.rtt_ms();
                        if !Self::send(&mut sink, &ClientMessage::Ping { client_time: now, rtt_ms }).await { return Ended::Dropped; }
                        self.pings_sent += 1;
                        // A burst at the start to find a good sample, then every 30 s.
                        self.next_ping_at = now + if self.pings_sent < 4 { 1000.0 } else { 30_000.0 };
                    }
                    self.drive();
                    if now >= self.next_report_at {
                        self.next_report_at = now + cfg.report_interval_ms;
                        let report = self.shared.engine.lock().report(&self.local());
                        if let Some(r) = report && !Self::send(&mut sink, &r).await { return Ended::Dropped; }
                    }
                }
            }
        }
    }

    async fn on_text(&mut self, text: &str, sink: &mut futures::stream::SplitSink<Socket, Message>) -> Option<Ended> {
        let msg = match parse_server_message(text) {
            Ok(m) => m,
            Err(Error::IncompatibleVersion) => {
                notice(&self.shared, UserMessage::IncompatibleVersion);
                return Some(Ended::Refused);
            }
            Err(e) => {
                // Malformed data never terminates the connection, and its content is not logged.
                tracing::warn!(target: "flicksync", "dropped a malformed message ({e})");
                return None;
            }
        };
        self.shared.debug.lock().last_server_event = event_name(&msg);
        match &msg {
            ServerMessage::Pong { client_time, server_time } => {
                let now = self.shared.clock.now_ms();
                self.shared.engine.lock().clock.record(*client_time, now, *server_time);
                self.drive();
                return None;
            }
            ServerMessage::Error { code, .. } => {
                tracing::info!(target: "flicksync", code, "server refused a request");
                notice(&self.shared, user_message_for_code(code));
                if matches!(code.as_str(), "CONTROL_DENIED" | "INVALID_SEQUENCE" | "NO_MEDIA") {
                    // Our idea of the room is off: ask for the canonical state.
                    let _ = Self::send(sink, &ClientMessage::SyncRequest).await;
                }
                return None;
            }
            ServerMessage::Unknown(_) => return None,
            _ => {}
        }
        if let Some(ended) = self.apply(&msg) {
            return Some(ended);
        }
        self.drive();
        None
    }

    /// Reduces a message into room state and sync engine; starts media loads.
    fn apply(&mut self, msg: &ServerMessage) -> Option<Ended> {
        let shared = Arc::clone(&self.shared);
        match msg {
            ServerMessage::RoomState { room, .. } => {
                shared.state.lock().apply(msg);
                let applied = {
                    let mut e = shared.engine.lock();
                    let applied = e.apply_snapshot(room.playback, false);
                    // Joining or reconnecting: always land on the current canonical position.
                    e.resync_now();
                    applied
                };
                if applied == Applied::Stale {
                    tracing::debug!(target: "flicksync", "room_state older than what we applied");
                }
                match &room.media {
                    Some(m) if self.media.as_ref() != Some(m) || !self.loaded => self.begin_load(m.clone()),
                    None if self.media.is_some() => {
                        self.media = None;
                        self.loaded = false;
                        shared.controller.unload();
                    }
                    _ => {}
                }
            }
            ServerMessage::MediaSelected { media, playback, .. } => {
                shared.state.lock().apply(msg);
                let accepted = shared.engine.lock().apply_snapshot(*playback, false) == Applied::Accepted;
                if accepted {
                    shared.state.lock().set_playback(playback);
                }
                self.begin_load(media.clone());
            }
            ServerMessage::Playback { snapshot, .. } => self.accept_snapshot(snapshot, false),
            ServerMessage::SyncState { snapshot } => self.accept_snapshot(snapshot, true),
            ServerMessage::SyncCorrection(c) => {
                let now = shared.clock.now_ms();
                if shared.engine.lock().apply_correction(c, now) == Applied::Accepted
                    && let crate::protocol::Correction::Seek { position, .. } = c
                {
                    shared.controller.seek(*position);
                }
            }
            ServerMessage::RoomClosed { reason } => {
                shared.state.lock().apply(msg);
                notice(&shared, UserMessage::RoomClosed);
                return Some(Ended::Closed(Some(reason.clone())));
            }
            ServerMessage::RoomUpdated { .. } => {
                let change = shared.state.lock().apply(msg);
                if let Change::Host { became_host, lost_host } = change {
                    tracing::info!(target: "flicksync", became_host, lost_host, "host changed");
                }
            }
            _ => {
                shared.state.lock().apply(msg);
            }
        }
        emit_state(&shared);
        None
    }

    fn accept_snapshot(&mut self, snap: &crate::protocol::PlaybackSnapshot, heartbeat: bool) {
        let accepted = self.shared.engine.lock().apply_snapshot(*snap, heartbeat) == Applied::Accepted;
        if accepted {
            self.shared.state.lock().set_playback(snap);
        } else {
            tracing::debug!(target: "flicksync", "ignored a stale playback state");
        }
    }

    fn begin_load(&mut self, media: MediaRef) {
        self.loaded = false;
        self.generation += 1;
        {
            let mut e = self.shared.engine.lock();
            e.media_changed();
            e.set_duration(media.duration_secs);
        }
        self.media = Some(media.clone());
        let (controller, tx, generation) = (Arc::clone(&self.shared.controller), self.itx.clone(), self.generation);
        tokio::spawn(async move {
            let result = controller.load_media(&media).await;
            let _ = tx.send(Internal::Loaded { generation, result });
        });
    }

    fn on_loaded(&mut self, generation: u64, result: std::result::Result<(), LoadError>) {
        if generation != self.generation {
            return; // The host moved on to something else while this was loading.
        }
        match result {
            Ok(()) => {
                self.loaded = true;
                self.shared.engine.lock().resync_now();
                self.drive();
            }
            Err(LoadError::Unavailable) => {
                // Stay in the room: the others carry on and chat still works.
                let title = self.media.as_ref().and_then(|m| m.title.clone());
                (self.shared.sink)(ClientEvent::MediaUnavailable { title });
            }
            Err(LoadError::Failed(why)) => {
                tracing::warn!(target: "flicksync", "could not load the shared media: {why}");
                notice(&self.shared, UserMessage::Generic);
            }
        }
    }

    /// One synchronization step: evaluate against the local player and apply.
    /// These changes are RemoteSync/InternalCorrection by construction: they
    /// never become room commands (see [`SyncEngine::outbound`]).
    fn drive(&mut self) {
        let now = self.shared.clock.now_ms();
        let local = self.local();
        let ev: Evaluation = self.shared.engine.lock().evaluate(&local, now);
        for action in &ev.actions {
            match *action {
                PlayerAction::Seek(p) => self.shared.controller.seek(p),
                PlayerAction::SetRate(r) => self.shared.controller.set_rate(r),
                PlayerAction::Play => self.shared.controller.play(),
                PlayerAction::Pause => self.shared.controller.pause(),
            }
        }
        let mut d = self.shared.debug.lock();
        d.canonical_position = Some(ev.canonical_position).filter(|_| self.shared.engine.lock().snapshot().is_some());
        d.local_position = Some(local.position).filter(|_| local.ready);
        d.drift_ms = ev.drift_ms;
        d.correction_rate = ev.multiplier;
    }
}

fn event_name(m: &ServerMessage) -> String {
    match m {
        ServerMessage::RoomState { .. } => "room_state",
        ServerMessage::ParticipantJoined(_) => "participant_joined",
        ServerMessage::ParticipantLeft { .. } => "participant_left",
        ServerMessage::PresenceChanged { .. } => "presence_changed",
        ServerMessage::MediaSelected { .. } => "media_selected",
        ServerMessage::Playback { .. } => "playback",
        ServerMessage::SyncState { .. } => "sync_state",
        ServerMessage::SyncCorrection(_) => "sync_correction",
        ServerMessage::RoomUpdated { .. } => "room_updated",
        ServerMessage::RoomClosed { .. } => "room_closed",
        ServerMessage::Chat(_) => "chat_message",
        ServerMessage::ChatHistory(_) => "chat_history",
        ServerMessage::Pong { .. } => "pong",
        ServerMessage::Error { .. } => "error",
        ServerMessage::Unknown(k) => k,
    }
    .to_owned()
}
