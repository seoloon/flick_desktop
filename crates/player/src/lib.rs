//! Native playback: libmpv engine, video presentation under the WebView,
//! decision application, runtime reconciliation and server progress reports.
//!
//! Flow of [`Player::play`]:
//! `playback_info` → [`oneshot_playback::decide`] → `stream` → mpv options →
//! `loadfile` → (file-loaded) track mapping → (reconfig) reconciliation →
//! periodic `report` until end/stop.

mod engine;
pub mod options;
pub mod presenter;
pub mod reconcile;
mod session;
pub use session::original_language;
pub mod state;
pub mod tracks;

use std::path::PathBuf;
use std::sync::Arc;

use oneshot_core::capabilities::CapabilityReport;
use oneshot_core::ids::ItemRef;
use oneshot_core::playback::PlaybackDecision;
use oneshot_core::provider::MediaProvider;
use oneshot_core::settings::{PresenterChoice, Settings};
use oneshot_core::{Error, Result};
use oneshot_mpv::{Api, Node};
use oneshot_playback::TrackRequest;
use parking_lot::Mutex;
use serde::Deserialize;

use crate::engine::Engine;
use crate::presenter::{HostWindow, UiDispatch, Viewport};
use crate::state::{LiveStats, PlayerEvent, PlayerSnapshot};
use crate::tracks::TrackType;

pub type EventSink = Arc<dyn Fn(PlayerEvent) + Send + Sync>;

pub struct PlayerConfig {
    pub libmpv_path: Option<PathBuf>,
    pub search_dirs: Vec<PathBuf>,
    /// Fonts shipped with the app (subtitle faces libass cannot find on the system).
    pub fonts_dir: Option<PathBuf>,
    pub host: HostWindow,
    pub dispatch: UiDispatch,
    pub runtime: tokio::runtime::Handle,
}

impl std::fmt::Debug for PlayerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerConfig").field("host", &self.host).field("search_dirs", &self.search_dirs).finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PlayRequest {
    pub item: Option<ItemRef>,
    /// Specific version; `None` = first offer.
    pub source_id: Option<String>,
    pub start_ms: Option<u64>,
    #[serde(default)]
    pub audio: TrackRequest,
    #[serde(default)]
    pub subtitle: TrackRequest,
    /// Prerolls: played, never reported to the server.
    #[serde(default)]
    pub silent: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "type")]
pub enum PlayerCommand {
    SetPause { paused: bool },
    TogglePause,
    SeekAbsolute { ms: u64 },
    SeekRelative { ms: i64 },
    SetVolume { volume: f64 },
    SetMute { muted: bool },
    /// `mpv_id = None` disables the track type (subtitles off).
    SelectTrack { kind: TrackType, mpv_id: Option<i64> },
    SetSpeed { speed: f64 },
    SetSubtitleDelay { ms: i64 },
    SetAudioDelay { ms: i64 },
    Stop,
}

pub(crate) struct Inner {
    pub api: Option<Arc<Api>>,
    pub engine: Option<Engine>,
    pub engine_presenter_choice: Option<PresenterChoice>,
    pub snapshot: PlayerSnapshot,
    pub session: Option<session::Session>,
    pub viewport: Option<Viewport>,
    /// Picture in Picture: the window is tiny, subtitles are enlarged to stay readable.
    pub pip: bool,
}

/// Playback timing read live from mpv, for the watch-together sync.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveTiming {
    pub position_secs: f64,
    pub speed: f64,
    pub paused: bool,
    /// Stalled waiting for data (not a user pause).
    pub buffering: bool,
}

pub struct Player {
    config: PlayerConfig,
    sink: EventSink,
    inner: Arc<Mutex<Inner>>,
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Player").field("config", &self.config).finish_non_exhaustive()
    }
}

impl Player {
    pub fn new(config: PlayerConfig, sink: EventSink) -> Self {
        let inner = Inner {
            api: None,
            engine: None,
            engine_presenter_choice: None,
            snapshot: PlayerSnapshot::default(),
            session: None,
            viewport: None,
            pip: false,
        };
        Self { config, sink, inner: Arc::new(Mutex::new(inner)) }
    }

    /// Loads libmpv (once) and reports its version. Safe to call at startup
    /// to surface a missing/old libmpv in diagnostics before any playback.
    pub fn engine_info(&self, settings: &Settings) -> Result<(String, (u32, u32))> {
        let api = self.api(settings)?;
        Ok((api.path.display().to_string(), api.client_api_version()))
    }

    fn api(&self, settings: &Settings) -> Result<Arc<Api>> {
        let mut inner = self.inner.lock();
        if let Some(api) = &inner.api {
            return Ok(Arc::clone(api));
        }
        let explicit = settings.advanced.libmpv_path.clone().map(PathBuf::from).or(self.config.libmpv_path.clone());
        let api = oneshot_mpv::load(explicit, &self.config.search_dirs).map_err(|e| {
            use oneshot_core::codes::{PLR_NOT_FOUND, PLR_VERSION};
            tracing::warn!(target: "player", "libmpv: {e}");
            match e {
                oneshot_mpv::Error::IncompatibleApi { .. } => Error::Playback(PLR_VERSION.tag("The libmpv found on this computer does not match this version of Flick. Use the one shipped with Flick.")),
                _ => Error::Playback(PLR_NOT_FOUND.tag("The video engine (libmpv) could not be loaded. Reinstall Flick; if you build it yourself, put libmpv next to the app.")),
            }
        })?;
        inner.api = Some(Arc::clone(&api));
        Ok(api)
    }

    /// (Re)creates the mpv core when needed. Presenter options are init-only,
    /// so changing the presenter recreates the core.
    fn ensure_engine(&self, settings: &Settings) -> Result<()> {
        let api = self.api(settings)?;
        let mut inner = self.inner.lock();
        let choice = settings.advanced.presenter;
        if inner.engine.is_some() && inner.engine_presenter_choice == Some(choice) {
            return Ok(());
        }
        inner.engine = None;
        // `d3d11-output-mode=composition` exists since mpv 0.41 (client API 2.5).
        let composition_supported = api.client_api_version() >= (2, 5);
        let presenter =
            presenter::choose(choice, self.config.host, Arc::clone(&self.config.dispatch), composition_supported);
        tracing::info!(target: "player", kind = ?presenter.kind(), "starting mpv engine");
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        // Before the user's own options, so an explicit `sub-fonts-dir` wins.
        let mut extra: Vec<(String, String)> =
            self.config.fonts_dir.iter().map(|d| ("sub-fonts-dir".to_owned(), d.display().to_string())).collect();
        extra.extend(settings.advanced.extra_mpv_options.iter().cloned());
        let engine = Engine::start(api, presenter, &extra, tx)
            .map_err(|e| Error::Playback(oneshot_core::codes::PLR_INIT.tag(format!("The video engine failed to start ({e}). Restart Flick."))))?;
        for (name, value) in options::base_properties(settings) {
            if let Err(e) = engine.mpv.set_property(name, value) {
                tracing::warn!(target: "player", "base option {name}: {e}");
            }
        }
        if let Some(vp) = inner.viewport {
            engine.presenter.set_viewport(&engine.mpv, vp);
        }
        inner.snapshot.presenter = Some(engine.presenter.kind());
        inner.engine = Some(engine);
        inner.engine_presenter_choice = Some(choice);
        drop(inner);
        session::spawn_event_loop(Arc::clone(&self.inner), Arc::clone(&self.sink), self.config.runtime.clone(), rx);
        Ok(())
    }

    /// Starts playback of an item. Returns the decision (also emitted).
    pub async fn play(
        &self,
        provider: Arc<dyn MediaProvider>,
        caps: Arc<CapabilityReport>,
        settings: Settings,
        display_id: Option<String>,
        request: PlayRequest,
    ) -> Result<PlaybackDecision> {
        let item = request.item.clone().ok_or_else(|| Error::Invalid(oneshot_core::codes::PLAY_NOTHING.tag("There is nothing to play.")))?;
        self.ensure_engine(&settings)?;
        session::stop_current(&self.inner, &self.config.runtime);
        session::start(&self.inner, &self.sink, provider, caps, settings, display_id, item, request).await
    }

    pub fn command(&self, cmd: PlayerCommand) -> Result<()> {
        let inner = self.inner.lock();
        let Some(engine) = &inner.engine else { return Err(Error::Playback(oneshot_core::codes::PLR_NOT_RUNNING.tag("The video engine is not running. Restart Flick."))) };
        let mpv = &engine.mpv;
        let r = match cmd {
            PlayerCommand::SetPause { paused } => mpv.set_property("pause", paused),
            PlayerCommand::TogglePause => mpv.command(&["cycle", "pause"]),
            PlayerCommand::SeekAbsolute { ms } => mpv.command(&["seek", &format!("{:.3}", ms as f64 / 1000.0), "absolute"]),
            PlayerCommand::SeekRelative { ms } => mpv.command(&["seek", &format!("{:.3}", ms as f64 / 1000.0), "relative"]),
            PlayerCommand::SetVolume { volume } => mpv.set_property("volume", volume.clamp(0.0, 150.0)),
            PlayerCommand::SetMute { muted } => mpv.set_property("mute", muted),
            PlayerCommand::SelectTrack { kind, mpv_id } => {
                mpv.set_property(kind.property(), mpv_id.map_or(Node::from("no"), Node::Int64))
            }
            PlayerCommand::SetSpeed { speed } => mpv.set_property("speed", speed.clamp(0.25, 4.0)),
            PlayerCommand::SetSubtitleDelay { ms } => mpv.set_property("sub-delay", ms as f64 / 1000.0),
            PlayerCommand::SetAudioDelay { ms } => mpv.set_property("audio-delay", ms as f64 / 1000.0),
            PlayerCommand::Stop => {
                drop(inner);
                session::stop_current(&self.inner, &self.config.runtime);
                return Ok(());
            }
        };
        r.map_err(|e| Error::Playback(oneshot_core::codes::PLR_COMMAND.tag(format!("The video engine rejected the command ({e})."))))
    }

    /// Places the video under the UI (physical pixels in the window).
    pub fn set_viewport(&self, viewport: Viewport) {
        let mut inner = self.inner.lock();
        // Unchanged: nothing to move (on Windows each call resizes mpv's swapchain).
        if inner.viewport == Some(viewport) {
            return;
        }
        inner.viewport = Some(viewport);
        if let Some(engine) = &inner.engine {
            engine.presenter.set_viewport(&engine.mpv, viewport);
        }
    }

    pub fn set_video_visible(&self, visible: bool) {
        if let Some(engine) = &self.inner.lock().engine {
            engine.presenter.set_visible(visible);
        }
    }

    /// Picture in Picture changes: subtitles are sized for the window, which
    /// is a fraction of the screen, so they are scaled up to remain readable.
    pub fn set_pip(&self, on: bool, settings: &Settings) {
        self.inner.lock().pip = on;
        self.apply_settings(settings);
    }

    /// Applies settings that can change at runtime (styles, hwdec, volume…).
    /// While a title plays, its volume and audio filters stay the session's
    /// (see [`options::live_properties`]).
    pub fn apply_settings(&self, settings: &Settings) {
        let inner = self.inner.lock();
        let Some(engine) = &inner.engine else { return };
        let props = match &inner.session {
            Some(s) => options::live_properties(settings, &s.decision().audio),
            None => options::base_properties(settings),
        };
        for (name, value) in props {
            let value = match (name, &value) {
                ("sub-scale", Node::Double(v)) if inner.pip => Node::Double(v * options::PIP_SUBTITLE_SCALE),
                _ => value,
            };
            if let Err(e) = engine.mpv.set_property(name, value) {
                tracing::warn!(target: "player", "apply {name}: {e}");
            }
        }
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        self.inner.lock().snapshot.clone()
    }

    /// The request that restarts the current title where it is, on the same
    /// version and tracks (after a quality change). `None` when idle.
    pub fn resume_request(&self) -> Option<PlayRequest> {
        let inner = self.inner.lock();
        let s = inner.session.as_ref()?;
        let (audio, subtitle) = s.current_tracks(&inner.snapshot.tracks);
        Some(PlayRequest {
            item: Some(s.item().clone()),
            source_id: Some(s.decision().source_id.clone()),
            start_ms: Some(inner.snapshot.position_ms),
            audio,
            subtitle,
            silent: false,
        })
    }

    /// Live technical stats straight from mpv (for the info overlay).
    pub fn stats(&self) -> LiveStats {
        let inner = self.inner.lock();
        let Some(engine) = &inner.engine else { return LiveStats::default() };
        let get = |p: &str| engine.mpv.try_get_property(p).ok().flatten();
        let text = |p: &str| get(p).and_then(|n| n.as_str().map(str::to_owned));
        let num = |p: &str| get(p).and_then(|n| n.as_f64());
        LiveStats {
            hwdec: text("hwdec-current"),
            video_params: get("video-params"),
            video_target: get("video-target-params"),
            audio_params: get("audio-params"),
            audio_out: get("audio-out-params"),
            current_ao: text("current-ao"),
            container_fps: num("container-fps"),
            display_fps: num("estimated-display-fps").or_else(|| num("display-fps")),
            dropped_frames: get("frame-drop-count").and_then(|n| n.as_i64()),
            decoder_dropped_frames: get("decoder-frame-drop-count").and_then(|n| n.as_i64()),
            avsync: num("avsync"),
            video_bitrate: num("video-bitrate"),
            audio_bitrate: num("audio-bitrate"),
        }
    }

    /// Which item is loaded and in what phase, without cloning the whole snapshot
    /// (tracks, decision, options): cheap enough to poll several times a second.
    pub fn now_playing(&self) -> (Option<ItemRef>, state::Phase) {
        let inner = self.inner.lock();
        (inner.snapshot.item.clone(), inner.snapshot.phase)
    }

    /// Unthrottled timing straight from mpv (the snapshot position is 4 Hz,
    /// too coarse to measure sub-100 ms drift). `None` without a loaded file.
    pub fn live_timing(&self) -> Option<LiveTiming> {
        let inner = self.inner.lock();
        let engine = inner.engine.as_ref()?;
        inner.session.as_ref()?;
        let get = |p: &str| engine.mpv.try_get_property(p).ok().flatten();
        Some(LiveTiming {
            position_secs: get("time-pos").and_then(|n| n.as_f64())?,
            speed: get("speed").and_then(|n| n.as_f64()).unwrap_or(1.0),
            paused: get("pause").and_then(|n| n.as_bool()).unwrap_or(false),
            buffering: get("paused-for-cache").and_then(|n| n.as_bool()).unwrap_or(false),
        })
    }

    /// Stops playback, sends the final report and releases the core.
    pub fn shutdown(&self) {
        session::stop_current(&self.inner, &self.config.runtime);
        self.inner.lock().engine = None;
    }
}
