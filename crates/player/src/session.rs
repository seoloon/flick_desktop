//! One playback session: negotiation, loading, event handling, reporting.

use std::sync::Arc;
use std::time::{Duration, Instant};

use oneshot_core::capabilities::{AudioDevice, CapabilityReport, DisplayCapabilities};
use oneshot_core::ids::ItemRef;
use oneshot_core::playback::{
    AudioOutputPlan, DeliveryKind, DeliveryRequest, ExternalSubtitle, PlaybackDecision, PlaybackReport, PlaybackState,
    ReportKind, StreamRequest, SubtitlePlan,
};
use oneshot_core::provider::MediaProvider;
use oneshot_core::settings::Settings;
use oneshot_core::{Error, Result};
use oneshot_mpv::{EndReason, Node};
use oneshot_playback::{DecisionInput, TrackRequest, client_profile, decide};
use parking_lot::Mutex;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::engine::EngineEvent;
use crate::state::{Chapter, Phase, PlayerEvent};
use crate::tracks::{self, TrackType};
use crate::{EventSink, Inner, PlayRequest, options, reconcile};

pub(crate) struct Session {
    provider: Arc<dyn MediaProvider>,
    item: ItemRef,
    decision: PlaybackDecision,
    play_session_id: Option<String>,
    delivery: DeliveryKind,
    external_subtitles: Vec<ExternalSubtitle>,
    settings: Settings,
    display: Option<DisplayCapabilities>,
    device: Option<AudioDevice>,
    source_audio_channels: u8,
    spatial_audio: bool,
    started: bool,
    tracks_applied: bool,
    video_reconciled: bool,
    hwdec_checked: bool,
    audio_fallback_done: bool,
    last_report: Instant,
}

impl Session {
    pub(crate) fn item(&self) -> &ItemRef {
        &self.item
    }

    pub(crate) fn decision(&self) -> &PlaybackDecision {
        &self.decision
    }

    /// Audio and subtitle choices in provider numbering. With the original
    /// file, mpv's selection maps back through `ff-index` (or the URL of an
    /// external subtitle), so a track picked in the player carries over.
    /// A server stream only holds the tracks that were requested.
    pub(crate) fn current_tracks(&self, tracks: &[tracks::Track]) -> (TrackRequest, TrackRequest) {
        let decided_sub = match self.decision.subtitles {
            SubtitlePlan::Local { index, .. } | SubtitlePlan::BurnIn { index } => TrackRequest::Index(index),
            SubtitlePlan::None => TrackRequest::Off,
        };
        let decided_audio = self.decision.audio_stream.map_or(TrackRequest::Auto, TrackRequest::Index);
        if self.delivery != DeliveryKind::DirectPlay || tracks.is_empty() {
            return (decided_audio, decided_sub);
        }
        let selected = |kind: TrackType| tracks.iter().find(|t| t.kind == kind && t.selected);
        let provider_index = |t: &tracks::Track| {
            t.stream_index.or_else(|| {
                let url = t.external_url.as_deref()?;
                self.external_subtitles.iter().find(|e| e.url.as_str() == url).map(|e| e.stream_index)
            })
        };
        let audio = selected(TrackType::Audio).and_then(provider_index).map_or(decided_audio, TrackRequest::Index);
        let subtitle = match selected(TrackType::Sub) {
            None => TrackRequest::Off,
            Some(t) => provider_index(t).map_or(decided_sub, TrackRequest::Index),
        };
        (audio, subtitle)
    }
}

fn delivery_kind(d: &DeliveryRequest) -> DeliveryKind {
    match d {
        DeliveryRequest::Direct => DeliveryKind::DirectPlay,
        DeliveryRequest::Remux { .. } => DeliveryKind::DirectStream,
        DeliveryRequest::Transcode { .. } => DeliveryKind::Transcode,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn start(
    inner: &Arc<Mutex<Inner>>,
    sink: &EventSink,
    provider: Arc<dyn MediaProvider>,
    caps: Arc<CapabilityReport>,
    settings: Settings,
    display_id: Option<String>,
    item: ItemRef,
    request: PlayRequest,
) -> Result<PlaybackDecision> {
    {
        let mut i = inner.lock();
        i.snapshot.phase = Phase::Loading;
        i.snapshot.item = Some(item.clone());
        i.snapshot.error = None;
    }
    sink(PlayerEvent::State { phase: Phase::Loading, position_ms: 0, duration_ms: None, buffered_ms: None, volume: f64::from(settings.audio.volume), muted: false });

    let result = negotiate_and_load(inner, sink, provider, &caps, settings, display_id, item, request).await;
    if let Err(e) = &result {
        let mut i = inner.lock();
        i.snapshot.phase = Phase::Error;
        i.snapshot.error = Some(e.to_string());
        drop(i);
        sink(PlayerEvent::Error { message: e.to_string() });
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn negotiate_and_load(
    inner: &Arc<Mutex<Inner>>,
    sink: &EventSink,
    provider: Arc<dyn MediaProvider>,
    caps: &CapabilityReport,
    settings: Settings,
    display_id: Option<String>,
    item: ItemRef,
    request: PlayRequest,
) -> Result<PlaybackDecision> {
    let profile = client_profile(caps, &settings);
    let info = provider.playback_info(&item, &profile).await?;
    let offer = match &request.source_id {
        Some(id) => info.offers.iter().find(|o| &o.source.id == id),
        None => info.offers.first(),
    }
    .ok_or_else(|| Error::Playback("the server returned no playable version".into()))?;

    let decision = decide(&DecisionInput {
        offer,
        caps,
        display_id: display_id.as_deref(),
        settings: &settings,
        audio: request.audio,
        subtitle: request.subtitle,
    })
    .map_err(|u| Error::Playback(u.to_string()))?;
    tracing::info!(target: "playback", item = %item, label = ?decision.label, video = ?decision.video, audio = ?decision.audio,
        reasons = ?decision.reasons.iter().map(|r| r.code.as_str()).collect::<Vec<_>>(), "decision");

    let subtitle_index = match decision.subtitles {
        SubtitlePlan::Local { index, .. } | SubtitlePlan::BurnIn { index } => Some(index),
        SubtitlePlan::None => None,
    };
    let start_ms = request.start_ms.unwrap_or(0);
    let target = provider
        .stream(&StreamRequest {
            item: item.clone(),
            source_id: decision.source_id.clone(),
            play_session_id: info.play_session_id.clone(),
            delivery: decision.delivery.clone(),
            audio_index: decision.audio_stream,
            subtitle_index,
            start_ms,
        })
        .await?;

    let display = display_id.as_deref().and_then(|id| caps.display(id)).or_else(|| caps.primary_display()).cloned();
    let device: Option<AudioDevice> = settings
        .audio
        .device
        .as_deref()
        .and_then(|id| caps.audio.device(id))
        .or_else(|| caps.audio.default_output())
        .cloned();
    let mut props = options::decision_properties(&decision, &settings, display.as_ref(), device.as_ref());
    let source_audio_channels = decision
        .audio_stream
        .and_then(|i| offer.source.audio.iter().find(|a| a.index == i))
        .map_or(2, |a| a.channels);
    let spatial = decision
        .audio_stream
        .and_then(|i| offer.source.audio.iter().find(|a| a.index == i))
        .is_some_and(|a| a.spatial.is_some());
    // Auth travels as HTTP headers (never in the URL). A node array avoids
    // mpv's comma-splitting of string lists (Jellyfin's header has commas).
    let headers: Vec<Node> = target.headers.iter().map(|(k, v)| Node::String(format!("{k}: {v}"))).collect();
    props.push(("http-header-fields", Node::Array(headers)));

    let applied: Vec<(String, String)>;
    {
        let mut i = inner.lock();
        let engine = i.engine.as_ref().ok_or_else(|| Error::Playback("engine not running".into()))?;
        for (name, value) in &props {
            if let Err(e) = engine.mpv.set_property(name, value.clone()) {
                tracing::warn!(target: "player", "option {name}: {e}");
            }
        }
        // `pause` is a global mpv option: it survives `stop` and `loadfile`.
        // Without this, a title left paused makes the next one open paused
        // while the UI believes it plays.
        if let Err(e) = engine.mpv.set_property("pause", false) {
            tracing::warn!(target: "player", "unpause before load: {e}");
        }
        // Server streams (Jellyfin and Plex HLS alike) list the whole title
        // from 0 and encode from whichever segment is fetched first: the
        // start offset sent to the server only warms it up, the seek is ours.
        let file_opts = format!("start={:.3}", start_ms as f64 / 1000.0);
        engine
            .mpv
            .command(&["loadfile", target.url.as_str(), "replace", "-1", &file_opts])
            .map_err(|e| Error::Playback(e.to_string()))?;
        engine.presenter.set_visible(true);

        applied = props
            .iter()
            .map(|(k, v)| {
                let shown = if *k == "http-header-fields" { "<redacted>".to_owned() } else { display_node(v) };
                ((*k).to_owned(), shown)
            })
            .collect();
        i.snapshot.applied_options = applied;
        i.snapshot.decision = Some(decision.clone());
        i.snapshot.tracks.clear();
        i.session = Some(Session {
            provider,
            item,
            decision: decision.clone(),
            play_session_id: info.play_session_id,
            delivery: delivery_kind(&decision.delivery),
            external_subtitles: target.external_subtitles,
            settings,
            display,
            device,
            source_audio_channels,
            spatial_audio: spatial,
            started: false,
            tracks_applied: false,
            video_reconciled: false,
            hwdec_checked: false,
            audio_fallback_done: false,
            last_report: Instant::now(),
        });
    }
    sink(PlayerEvent::Decision { decision: decision.clone() });
    Ok(decision)
}

/// Human-readable value for the Debug panel.
fn display_node(n: &Node) -> String {
    match n {
        Node::String(s) if s.is_empty() => "(empty)".into(),
        Node::String(s) => s.clone(),
        Node::Flag(b) => if *b { "yes" } else { "no" }.into(),
        Node::Int64(i) => i.to_string(),
        Node::Double(d) => d.to_string(),
        Node::Array(a) => a.iter().map(display_node).collect::<Vec<_>>().join(", "),
        other => format!("{other:?}"),
    }
}

/// Sends the Stop report for the active session and stops mpv.
pub(crate) fn stop_current(inner: &Arc<Mutex<Inner>>, rt: &tokio::runtime::Handle) {
    let mut i = inner.lock();
    let Some(session) = i.session.take() else { return };
    if let Some(engine) = &i.engine {
        let _ = engine.mpv.command(&["stop"]);
        engine.presenter.set_visible(false);
    }
    let position = i.snapshot.position_ms;
    let duration = i.snapshot.duration_ms;
    i.snapshot.phase = Phase::Idle;
    drop(i);
    if session.started {
        send_report(rt, &session, ReportKind::Stop, PlaybackState::Stopped, position, duration, None);
    }
}

fn send_report(
    rt: &tokio::runtime::Handle,
    s: &Session,
    kind: ReportKind,
    state: PlaybackState,
    position_ms: u64,
    duration_ms: Option<u64>,
    selection: Option<(Option<u32>, Option<u32>, u8, bool)>,
) {
    if !s.settings.privacy.report_progress {
        return;
    }
    let (audio_index, subtitle_index, volume, muted) = selection.unwrap_or((s.decision.audio_stream, None, s.settings.audio.volume, false));
    let report = PlaybackReport {
        kind,
        item: s.item.clone(),
        source_id: s.decision.source_id.clone(),
        play_session_id: s.play_session_id.clone(),
        state,
        position_ms,
        duration_ms,
        delivery: s.delivery,
        audio_index,
        subtitle_index,
        volume,
        muted,
    };
    let provider = Arc::clone(&s.provider);
    rt.spawn(async move {
        if let Err(e) = provider.report(&report).await {
            tracing::warn!(target: "playback", kind = ?report.kind, "progress report failed: {e}");
        } else {
            tracing::debug!(target: "playback", kind = ?report.kind, position_ms = report.position_ms, "progress reported");
        }
    });
}

pub(crate) fn spawn_event_loop(
    inner: Arc<Mutex<Inner>>,
    sink: EventSink,
    rt: tokio::runtime::Handle,
    mut rx: UnboundedReceiver<EngineEvent>,
) {
    let rt2 = rt.clone();
    rt.spawn(async move {
        while let Some(ev) = rx.recv().await {
            if matches!(ev, EngineEvent::Shutdown) {
                break;
            }
            handle(&inner, &sink, &rt2, ev);
        }
    });
}

fn ms(n: &Node) -> Option<u64> {
    n.as_f64().map(|s| (s.max(0.0) * 1000.0) as u64)
}

fn emit_state(inner: &Inner, sink: &EventSink) {
    let s = &inner.snapshot;
    sink(PlayerEvent::State {
        phase: s.phase,
        position_ms: s.position_ms,
        duration_ms: s.duration_ms,
        buffered_ms: s.buffered_ms,
        volume: s.volume,
        muted: s.muted,
    });
}

fn handle(inner: &Arc<Mutex<Inner>>, sink: &EventSink, rt: &tokio::runtime::Handle, ev: EngineEvent) {
    let mut guard = inner.lock();
    let i = &mut *guard;
    match ev {
        EngineEvent::Property { name, value } => match name.as_str() {
            "time-pos" => {
                if let Some(p) = ms(&value) {
                    i.snapshot.position_ms = p;
                }
                emit_state(i, sink);
                if let Some(s) = &mut i.session
                    && s.started
                    && i.snapshot.phase == Phase::Playing
                    && s.last_report.elapsed() >= Duration::from_secs(u64::from(s.settings.playback.report_interval_secs.max(5)))
                {
                    s.last_report = Instant::now();
                    send_report(rt, s, ReportKind::Progress, PlaybackState::Playing, i.snapshot.position_ms, i.snapshot.duration_ms, None);
                }
            }
            "duration" => {
                i.snapshot.duration_ms = ms(&value);
                emit_state(i, sink);
            }
            "demuxer-cache-time" => i.snapshot.buffered_ms = ms(&value),
            "volume" => {
                i.snapshot.volume = value.as_f64().unwrap_or(i.snapshot.volume);
                emit_state(i, sink);
            }
            "mute" => {
                i.snapshot.muted = value.as_bool().unwrap_or(false);
                emit_state(i, sink);
            }
            "pause" | "paused-for-cache" => {
                let paused = value.as_bool().unwrap_or(false);
                let prev = i.snapshot.phase;
                i.snapshot.phase = match (name.as_str(), paused) {
                    ("pause", true) => Phase::Paused,
                    ("paused-for-cache", true) => Phase::Buffering,
                    _ if i.session.as_ref().is_some_and(|s| s.started) => Phase::Playing,
                    _ => prev,
                };
                emit_state(i, sink);
                if name == "pause"
                    && prev != i.snapshot.phase
                    && let Some(s) = &mut i.session
                    && s.started
                {
                    s.last_report = Instant::now();
                    let state = if paused { PlaybackState::Paused } else { PlaybackState::Playing };
                    send_report(rt, s, ReportKind::Progress, state, i.snapshot.position_ms, i.snapshot.duration_ms, None);
                }
            }
            "eof-reached" if value.as_bool() == Some(true) => {
                i.snapshot.phase = Phase::Ended;
                emit_state(i, sink);
                let item = i.snapshot.item.clone();
                if let Some(s) = i.session.take() {
                    let end = i.snapshot.duration_ms.unwrap_or(i.snapshot.position_ms);
                    send_report(rt, &s, ReportKind::Stop, PlaybackState::Stopped, end, i.snapshot.duration_ms, None);
                }
                sink(PlayerEvent::Ended { item, natural: true });
            }
            "track-list" => {
                i.snapshot.tracks = tracks::parse_track_list(&value);
                sink(PlayerEvent::Tracks { tracks: i.snapshot.tracks.clone() });
            }
            "chapter-list" => {
                i.snapshot.chapters = value
                    .as_array()
                    .unwrap_or_default()
                    .iter()
                    .map(|c| Chapter {
                        title: c.get("title").and_then(Node::as_str).map(str::to_owned),
                        start_ms: c.get("time").and_then(ms).unwrap_or(0),
                    })
                    .collect();
                sink(PlayerEvent::Chapters { chapters: i.snapshot.chapters.clone() });
            }
            "hwdec-current" => {
                if let Some(s) = &mut i.session
                    && !s.hwdec_checked
                    && let Some(h) = value.as_str()
                {
                    s.hwdec_checked = true;
                    if let Some(r) = reconcile::hwdec(&s.decision, h) {
                        s.decision.reasons.push(r);
                        s.decision.hardware_decode = Some(false);
                        i.snapshot.decision = Some(s.decision.clone());
                        sink(PlayerEvent::Decision { decision: s.decision.clone() });
                    }
                }
            }
            _ => {}
        },
        EngineEvent::FileLoaded => {
            if let (Some(s), Some(engine)) = (&mut i.session, &i.engine) {
                for sub in &s.external_subtitles {
                    let title = sub.title.clone().unwrap_or_default();
                    let lang = sub.language.clone().unwrap_or_default();
                    if let Err(e) = engine.mpv.command(&["sub-add", sub.url.as_str(), "auto", &title, &lang]) {
                        tracing::warn!(target: "player", "sub-add failed: {e}");
                    }
                }
                apply_track_selection(s, engine, &mut i.snapshot.tracks);
            }
        }
        EngineEvent::PlaybackRestart => {
            if let Some(s) = &mut i.session
                && !s.started
            {
                s.started = true;
                s.last_report = Instant::now();
                // Report what mpv actually does, never an assumed state.
                let paused = i.engine.as_ref().and_then(|e| e.mpv.try_get_property("pause").ok().flatten()).and_then(|n| n.as_bool()).unwrap_or(false);
                i.snapshot.phase = if paused { Phase::Paused } else { Phase::Playing };
                send_report(rt, s, ReportKind::Start, PlaybackState::Playing, i.snapshot.position_ms, i.snapshot.duration_ms, None);
                emit_state(i, sink);
            }
        }
        EngineEvent::VideoReconfig => {
            if let (Some(s), Some(engine)) = (&mut i.session, &i.engine)
                && !s.video_reconciled
                && let Ok(Some(params)) = engine.mpv.try_get_property("video-params")
                && let Some(gamma) = params.get("gamma").and_then(Node::as_str)
            {
                s.video_reconciled = true;
                if let Some((plan, why)) = reconcile::video(&s.decision, gamma, s.display.as_ref(), &s.settings) {
                    tracing::warn!(target: "playback", "{}", why.message);
                    for (name, value) in options::video_target(&plan, s.display.as_ref()) {
                        let _ = engine.mpv.set_property(name, value);
                    }
                    s.decision.video = plan;
                    s.decision.reasons.push(why);
                    i.snapshot.decision = Some(s.decision.clone());
                    sink(PlayerEvent::Decision { decision: s.decision.clone() });
                }
            }
        }
        EngineEvent::AudioOutputFailed(msg) => {
            if let (Some(s), Some(engine)) = (&mut i.session, &i.engine)
                && !s.audio_fallback_done
                && let Some(why) = reconcile::bitstream_failed(&s.decision)
            {
                s.audio_fallback_done = true;
                tracing::warn!(target: "playback", "{msg}; falling back to PCM");
                let _ = engine.mpv.set_property("audio-spdif", "");
                let _ = engine.mpv.set_property("audio-exclusive", s.settings.audio.exclusive);
                let _ = engine.mpv.command(&["ao-reload"]);
                let output_channels = s.device.as_ref().map_or(2, |d| d.channels);
                s.decision.audio = AudioOutputPlan::Pcm {
                    source_channels: s.source_audio_channels,
                    output_channels,
                    downmix: s.source_audio_channels > output_channels,
                    spatial_lost: s.spatial_audio,
                };
                s.decision.reasons.push(why);
                i.snapshot.decision = Some(s.decision.clone());
                sink(PlayerEvent::Decision { decision: s.decision.clone() });
            }
        }
        EngineEvent::AudioReconfig => {}
        EngineEvent::EndFile { reason, error } => {
            if reason == EndReason::Error {
                let message = error.unwrap_or_else(|| "playback failed".into());
                i.snapshot.phase = Phase::Error;
                i.snapshot.error = Some(message.clone());
                sink(PlayerEvent::Error { message });
            }
        }
        EngineEvent::Shutdown => {}
    }
}

/// Selects the decided audio/subtitle/video streams by mapping provider
/// stream indexes onto mpv track ids.
fn apply_track_selection(s: &mut Session, engine: &crate::engine::Engine, tracks_cache: &mut Vec<tracks::Track>) {
    if s.tracks_applied {
        return;
    }
    let Ok(Some(list)) = engine.mpv.try_get_property("track-list") else { return };
    let list = tracks::parse_track_list(&list);
    let select = |kind: TrackType, id: Option<i64>| {
        if let Some(id) = id {
            let _ = engine.mpv.set_property(kind.property(), id);
        }
    };
    select(TrackType::Video, tracks::mpv_id(&list, TrackType::Video, s.decision.video_stream, None));
    select(TrackType::Audio, tracks::mpv_id(&list, TrackType::Audio, s.decision.audio_stream, None));
    match &s.decision.subtitles {
        SubtitlePlan::Local { index, .. } => {
            let external = s.external_subtitles.iter().find(|e| e.stream_index == *index).map(|e| e.url.as_str().to_owned());
            let id = tracks::mpv_id(&list, TrackType::Sub, Some(*index), external.as_deref())
                .or_else(|| external.as_deref().and_then(|u| tracks::mpv_id(&list, TrackType::Sub, None, Some(u))));
            match id {
                Some(id) => select(TrackType::Sub, Some(id)),
                None => tracing::warn!(target: "player", index, "decided subtitle not found in mpv track list"),
            }
        }
        // Burned-in or none: make sure mpv does not auto-select one.
        _ => {
            let _ = engine.mpv.set_property("sid", "no");
        }
    }
    s.tracks_applied = true;
    *tracks_cache = list;
}
