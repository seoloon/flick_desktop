//! FlickSync in the app: the seam between the room client and the existing
//! player/catalogue. There is no second player and no second login here.
//!
//! * [`AppController`] implements the client's `PlaybackController` on top of
//!   `oneshot-player`, and resolves a shared `MediaRef` on the user's own servers.
//! * [`Hub`] builds the client from settings, routes `play` and the player
//!   controls while a room is active, and forwards events to the UI.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use oneshot_core::ids::ItemRef;
use oneshot_core::media::ItemKind;
use oneshot_core::server::ProviderKind;
use oneshot_core::{Error, Result};
use oneshot_flicksync::auth::{
    EndpointTokenProvider, Identity, LocalKeyTokenProvider, SigningKey, TokenProvider, discover, sanitize_user_id,
};
use oneshot_flicksync::clock::MonotonicClock;
use oneshot_flicksync::protocol::{MediaRef, MediaType, Provider};
use oneshot_flicksync::sync::{LocalPlayback, SyncConfig};
use oneshot_flicksync::{ClientEvent, FlickSyncClient, LoadError, PlaybackController, UserMessage};
use oneshot_player::PlayerCommand;
use oneshot_player::state::Phase;
use oneshot_storage::secrets;
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;
use url::Url;

use crate::state::AppState;

/// Keychain entry for a self-hosted FlickSync signing key (`kid:server_id:secret`).
pub const KEY_ENTRY: &str = "flicksync-key";
const EVENT: &str = "flicksync";
/// How long a joining player waits for the UI to open the player screen.
const UI_OPEN_TIMEOUT: Duration = Duration::from_secs(20);

fn into_error(e: oneshot_flicksync::Error) -> Error {
    // The user reads a friendly sentence, never the protocol or Rust error.
    tracing::debug!(target: "flicksync", "{e}");
    Error::Other(e.user_message().text().to_owned())
}

/// A player screen the UI is about to open for a shared item.
struct Gate {
    item: ItemRef,
    done: oneshot::Sender<std::result::Result<(), String>>,
}

pub struct AppController {
    app: AppHandle,
    /// The item the room is watching, once resolved on our servers.
    current: Mutex<Option<ItemRef>>,
    gate: Mutex<Option<Gate>>,
    /// The host started this item itself (it announced it to the room too).
    host_started: Mutex<Option<ItemRef>>,
}

impl std::fmt::Debug for AppController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppController").finish_non_exhaustive()
    }
}

/// What the `play` command must do while a room may be active.
#[derive(Debug)]
pub enum PlayRoute {
    Normal,
    /// The host plays and tells the room.
    Host,
    /// The room asked for this item: playing it answers the pending load.
    Joiner(PendingLoad),
}

#[derive(Debug)]
pub struct PendingLoad(oneshot::Sender<std::result::Result<(), String>>);

impl PendingLoad {
    pub fn finish<T>(self, r: &Result<T>) {
        let _ = self.0.send(r.as_ref().map(|_| ()).map_err(|e| e.to_string()));
    }
}

impl AppController {
    fn new(app: AppHandle) -> Self {
        Self { app, current: Mutex::new(None), gate: Mutex::new(None), host_started: Mutex::new(None) }
    }

    fn state(&self) -> tauri::State<'_, Arc<AppState>> {
        self.app.state::<Arc<AppState>>()
    }

    fn reset(&self) {
        *self.current.lock() = None;
        *self.host_started.lock() = None;
        if let Some(g) = self.gate.lock().take() {
            let _ = g.done.send(Err("left the room".into()));
        }
    }

    fn command(&self, c: PlayerCommand) {
        if let Err(e) = self.state().player.command(c) {
            tracing::debug!(target: "flicksync", "player refused a sync action: {e}");
        }
    }

    fn take_gate(&self, item: &ItemRef) -> Option<PendingLoad> {
        let mut g = self.gate.lock();
        if g.as_ref().is_some_and(|g| &g.item == item) { g.take().map(|g| PendingLoad(g.done)) } else { None }
    }
}

#[async_trait]
impl PlaybackController for AppController {
    fn local(&self) -> LocalPlayback {
        let st = self.state();
        let (item, phase) = st.player.now_playing();
        let current = self.current.lock().clone();
        let on_room_item = current.is_some() && item == current;
        let usable = matches!(phase, Phase::Playing | Phase::Paused | Phase::Buffering);
        match st.player.live_timing() {
            Some(t) if on_room_item && usable => LocalPlayback {
                ready: true,
                position: t.position_secs,
                paused: t.paused,
                buffering: t.buffering || phase == Phase::Buffering,
                rate: t.speed,
            },
            _ => LocalPlayback { ready: false, position: 0.0, paused: true, buffering: false, rate: 1.0 },
        }
    }

    fn play(&self) {
        self.command(PlayerCommand::SetPause { paused: false });
    }

    fn pause(&self) {
        self.command(PlayerCommand::SetPause { paused: true });
    }

    fn seek(&self, secs: f64) {
        self.command(PlayerCommand::SeekAbsolute { ms: (secs.max(0.0) * 1000.0).round() as u64 });
    }

    fn set_rate(&self, rate: f64) {
        self.command(PlayerCommand::SetSpeed { speed: rate });
    }

    async fn load_media(&self, media: &MediaRef) -> std::result::Result<(), LoadError> {
        let (item, already) = {
            let st = self.state();
            let item = resolve(&st, media).await.ok_or(LoadError::Unavailable)?;
            let (playing, phase) = st.player.now_playing();
            let playing_it = playing.as_ref() == Some(&item) && !matches!(phase, Phase::Idle | Phase::Ended | Phase::Error);
            (item, playing_it)
        };
        *self.current.lock() = Some(item.clone());
        if already || self.host_started.lock().as_ref() == Some(&item) {
            return Ok(());
        }
        // Not playing it yet: have the UI open the player, whose `play` call answers us.
        let (tx, rx) = oneshot::channel();
        *self.gate.lock() = Some(Gate { item: item.clone(), done: tx });
        let _ = self.app.emit(EVENT, serde_json::json!({ "type": "openPlayer", "item": item.to_string() }));
        match tokio::time::timeout(UI_OPEN_TIMEOUT, rx).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(why))) => Err(LoadError::Failed(why)),
            Ok(Err(_)) => Err(LoadError::Failed("cancelled".into())),
            Err(_) => {
                self.gate.lock().take();
                Err(LoadError::Failed("the player did not open".into()))
            }
        }
    }

    fn unload(&self) {
        let was = self.current.lock().take();
        let st = self.state();
        if was.is_some() && st.player.now_playing().0 == was {
            self.command(PlayerCommand::Stop);
        }
    }
}

/// Finds the shared title on one of *our* servers: same provider, same server
/// id. Ids are never assumed interchangeable between servers or providers.
async fn resolve(st: &AppState, media: &MediaRef) -> Option<ItemRef> {
    let kind = match media.provider {
        Provider::Jellyfin => ProviderKind::Jellyfin,
        Provider::Plex => ProviderKind::Plex,
    };
    let candidates: Vec<_> =
        st.servers.read().iter().filter(|s| s.kind == kind && s.remote_id == media.server_id && !s.disabled).map(|s| s.id).collect();
    for id in candidates {
        // A connection of another profile is not loaded; skip it.
        if st.catalog.provider(id).is_err() {
            continue;
        }
        let item = ItemRef::new(id, &media.media_id);
        if st.catalog.item(&item).await.is_ok() {
            return Some(item);
        }
    }
    None
}

/// What we announce to the room about a title: its identity, nothing else.
async fn media_ref_for(st: &AppState, item: &ItemRef) -> Result<MediaRef> {
    let desc = st.servers.read().iter().find(|s| s.id == item.server).cloned().ok_or_else(|| Error::NotFound("server".into()))?;
    let mi = st.catalog.item(item).await?;
    let (media_type, season_id, episode_id, title) = match mi.kind {
        ItemKind::Movie => (MediaType::Movie, None, None, mi.title.clone()),
        ItemKind::Episode => {
            let e = mi.episode.as_ref();
            let title = match e.and_then(|e| e.series_title.as_deref()) {
                Some(series) => format!("{series} \u{b7} {}", mi.title),
                None => mi.title.clone(),
            };
            (MediaType::Episode, e.and_then(|e| e.season.as_ref()).map(|s| s.key.clone()), Some(item.key.clone()), title)
        }
        _ => return Err(Error::Invalid("Only movies and episodes can be watched together.".into())),
    };
    let media = MediaRef {
        provider: match desc.kind {
            ProviderKind::Jellyfin => Provider::Jellyfin,
            ProviderKind::Plex => Provider::Plex,
        },
        server_id: desc.remote_id,
        media_id: item.key.clone(),
        media_type,
        season_id,
        episode_id,
        title: Some(title.chars().take(120).collect()),
        duration_secs: mi.runtime_ms.map(|ms| ms as f64 / 1000.0),
    };
    media.validate().map_err(into_error)?;
    Ok(media)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// Something is configured and reachable: the UI may offer rooms.
    pub available: bool,
    pub configured: bool,
    pub in_room: bool,
    pub message: Option<UserMessage>,
}

pub struct Hub {
    app: AppHandle,
    controller: Arc<AppController>,
    /// Current client with the config it was built for.
    client: tokio::sync::Mutex<Option<(String, FlickSyncClient)>>,
    /// Sync view of the same client, for the synchronous player commands.
    active: RwLock<Option<FlickSyncClient>>,
}

impl std::fmt::Debug for Hub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hub").finish_non_exhaustive()
    }
}

impl Hub {
    pub fn new(app: AppHandle) -> Self {
        Self {
            controller: Arc::new(AppController::new(app.clone())),
            app,
            client: tokio::sync::Mutex::new(None),
            active: RwLock::new(None),
        }
    }

    fn identity(st: &AppState) -> Identity {
        let s = st.settings();
        let name = s
            .flicksync
            .display_name
            .clone()
            .filter(|n| !n.trim().is_empty())
            .or_else(|| st.active_profile_name())
            .or_else(|| st.servers.read().iter().find(|d| !d.disabled).map(|d| d.user.name.clone()))
            .unwrap_or_else(|| "Flick".into());
        Identity { user_id: sanitize_user_id(&st.identity.device_id), display_name: name.trim().chars().take(64).collect() }
    }

    /// `(fingerprint, base url, token provider)` from the settings, or why not.
    async fn configuration(&self, st: &AppState) -> Result<(String, Url, Arc<dyn TokenProvider>)> {
        let cfg = st.settings().flicksync;
        let not_configured = || Error::Other(UserMessage::NotConfigured.text().to_owned());
        if !cfg.enabled {
            return Err(not_configured());
        }
        let who = Self::identity(st);
        let http = st.http();
        // The Flick Server comes first, but when it does not answer (or does not offer
        // FlickSync) a manually configured address + key still works.
        let mut failure = None;
        if let Some(server) = cfg.flick_server_url.as_deref().filter(|s| !s.trim().is_empty()) {
            match Url::parse(server.trim()) {
                Ok(server) => match discover(&http, &server).await {
                    Ok(Some((sync_url, token_url))) => {
                        let fp = format!("server|{sync_url}|{token_url}|{}", who.display_name);
                        return Ok((fp, sync_url, Arc::new(EndpointTokenProvider::new(http, token_url, who))));
                    }
                    Ok(None) => failure = Some(Error::Other(UserMessage::Unavailable.text().to_owned())),
                    Err(e) => failure = Some(into_error(e)),
                },
                Err(_) => failure = Some(not_configured()),
            }
        }
        if let Some(url) = cfg.sync_url.as_deref().filter(|s| !s.trim().is_empty()) {
            let base = Url::parse(url.trim())
                .ok()
                .filter(|u| matches!(u.scheme(), "http" | "https" | "ws" | "wss"))
                .ok_or_else(|| Error::Other("The FlickSync address isn't valid (use http://host:port or https://…).".into()))?;
            let key = secrets::load_secret(KEY_ENTRY)?.ok_or_else(not_configured)?;
            let key = SigningKey::parse(&key).map_err(|_| not_configured())?;
            let fp = format!("key|{base}|{}|{}|{}", key.kid, key.server_id, who.display_name);
            return Ok((fp, base, Arc::new(LocalKeyTokenProvider::new(key, who))));
        }
        Err(failure.unwrap_or_else(not_configured))
    }

    /// The client for the current settings (rebuilt when they changed and no room is open).
    pub async fn client(&self, st: &AppState) -> Result<FlickSyncClient> {
        let (fp, base, tokens) = self.configuration(st).await?;
        let mut slot = self.client.lock().await;
        if let Some((old_fp, c)) = slot.as_ref()
            && (*old_fp == fp || c.in_room())
        {
            return Ok(c.clone());
        }
        let app = self.app.clone();
        let controller = Arc::clone(&self.controller);
        let sink: oneshot_flicksync::EventSink = Arc::new(move |e: ClientEvent| {
            if matches!(e, ClientEvent::Left { .. }) {
                controller.reset();
            }
            let _ = app.emit(EVENT, &e);
        });
        let client = FlickSyncClient::new(
            st.http(),
            base,
            tokens,
            Arc::clone(&self.controller) as Arc<dyn PlaybackController>,
            Arc::new(MonotonicClock::new()),
            SyncConfig::default(),
            sink,
        );
        *self.active.write() = Some(client.clone());
        *slot = Some((fp, client.clone()));
        Ok(client)
    }

    /// The client when a room is open (synchronous: used by player commands).
    pub fn room_client(&self) -> Option<FlickSyncClient> {
        self.active.read().as_ref().filter(|c| c.in_room()).cloned()
    }

    pub async fn status(&self, st: &AppState) -> Status {
        let in_room = self.room_client().is_some();
        match self.client(st).await {
            Ok(c) => {
                let available = in_room || c.available().await;
                Status { available, configured: true, in_room, message: (!available).then_some(UserMessage::Unavailable) }
            }
            Err(_) => {
                // Something filled in but not working is "unreachable", not "not set up".
                let cfg = st.settings().flicksync;
                let filled = |v: &Option<String>| v.as_deref().is_some_and(|s| !s.trim().is_empty());
                let configured = cfg.enabled && (filled(&cfg.flick_server_url) || filled(&cfg.sync_url));
                Status { available: false, configured, in_room, message: configured.then_some(UserMessage::Unavailable) }
            }
        }
    }

    pub async fn select_media(&self, st: &AppState, item: &ItemRef) -> Result<()> {
        let client = self.room_client().ok_or_else(|| Error::Invalid("not in a room".into()))?;
        let media = media_ref_for(st, item).await?;
        client.select_media(media).map_err(into_error)
    }

    /// Decides how `play` proceeds for `item`.
    pub async fn route_play(&self, st: &AppState, item: &ItemRef) -> Result<PlayRoute> {
        let Some(client) = self.room_client() else { return Ok(PlayRoute::Normal) };
        // The room is waiting for this very item: this play call answers it.
        if let Some(pending) = self.controller.take_gate(item) {
            return Ok(PlayRoute::Joiner(pending));
        }
        if !client.state().is_host() {
            return Err(Error::Other(UserMessage::OnlyHostCanChooseMedia.text().to_owned()));
        }
        *self.controller.host_started.lock() = Some(item.clone());
        if let Err(e) = self.select_media(st, item).await {
            *self.controller.host_started.lock() = None;
            return Err(e);
        }
        Ok(PlayRoute::Host)
    }

    /// While a room is active the player's own controls become room commands
    /// (not applied locally: the canonical broadcast comes back to us).
    /// `Some` = handled here.
    pub fn intercept(&self, cmd: &PlayerCommand) -> Option<Result<()>> {
        let client = self.room_client()?;
        let local = self.controller.local();
        if !local.ready {
            return None; // The player is not on the room's media: ordinary local control.
        }
        let sent = match cmd {
            PlayerCommand::SetPause { paused: true } => client.user_pause(),
            PlayerCommand::SetPause { paused: false } => client.user_play(),
            PlayerCommand::TogglePause if local.paused => client.user_play(),
            PlayerCommand::TogglePause => client.user_pause(),
            PlayerCommand::SeekAbsolute { ms } => client.user_seek(*ms as f64 / 1000.0),
            PlayerCommand::SeekRelative { ms } => client.user_seek(local.position + *ms as f64 / 1000.0),
            PlayerCommand::SetSpeed { speed } => client.user_rate(*speed),
            _ => return None,
        };
        if let Err(e) = sent {
            let message = e.user_message();
            let _ = self.app.emit(EVENT, &ClientEvent::Notice { message });
        }
        Some(Ok(()))
    }

    pub fn host_play_done(&self, item: &ItemRef) {
        let mut h = self.controller.host_started.lock();
        if h.as_ref() == Some(item) {
            *h = None;
        }
    }

    pub fn current_item(&self) -> Option<ItemRef> {
        self.controller.current.lock().clone()
    }
}

pub fn into_core_error(e: oneshot_flicksync::Error) -> Error {
    into_error(e)
}
