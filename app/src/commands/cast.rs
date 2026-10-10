//! Casting commands: pick a Chromecast / AirPlay receiver, send it what is
//! playing, drive it, and keep the server's progress up to date meanwhile.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use oneshot_cast::{CastCommand, CastDevice, CastMedia, CastState, CastStatus};
use oneshot_core::ids::ItemRef;
use oneshot_core::playback::{ClientProfile, DeliveryKind, DeliveryRequest, PlaybackReport, PlaybackState, ReportKind, StreamRequest};
use oneshot_core::stream::{AudioCodec, SubtitleFormat, VideoCodec};
use oneshot_core::{Error, Result};
use oneshot_player::PlayerCommand;
use oneshot_playback::{TrackRequest, tracks};
use tauri::State;

use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

/// What every cast receiver decodes: H.264, AAC (or AC-3), stereo, up to 1080p.
fn receiver_profile(max_bitrate: u64) -> ClientProfile {
    ClientProfile {
        name: "Flick (cast)".into(),
        max_bitrate: Some(max_bitrate),
        video_codecs: vec![VideoCodec::H264],
        audio_codecs: vec![AudioCodec::Aac, AudioCodec::Ac3],
        containers: vec!["mp4".into(), "ts".into()],
        subtitle_formats: vec![SubtitleFormat::Srt, SubtitleFormat::Ass, SubtitleFormat::WebVtt],
        max_width: 1920,
        max_height: 1080,
        max_audio_channels: 2,
    }
}

/// Which cast is the current one: a reporter whose number is old stops.
static CAST_GENERATION: AtomicU64 = AtomicU64::new(0);

/// 12 Mb/s: a good 1080p picture that home Wi-Fi and the receivers carry.
const CAST_BITRATE: u64 = 12_000_000;

#[tauri::command(async)]
pub fn cast_devices(state: St<'_>) -> Vec<CastDevice> {
    state.cast.devices()
}

/// Sends `item` to the receiver, from `start_ms`. The local player stops.
#[tauri::command]
pub async fn cast_start(state: St<'_>, device: String, item: ItemRef, start_ms: u64) -> Result<()> {
    let provider = state.catalog.provider(item.server)?;
    let settings = state.settings();
    let max_bitrate = settings.playback.max_bitrate.map_or(CAST_BITRATE, |b| b.min(CAST_BITRATE));
    let info = provider.playback_info(&item, &receiver_profile(max_bitrate)).await?;
    let source = info.offers.first().map(|o| &o.source).ok_or_else(|| Error::Playback(oneshot_core::codes::PLAY_NO_VERSION.tag("The server returned no playable version of this title.")))?;

    // The same language and subtitle choices as locally; subtitles are burnt
    // into the picture, as a receiver has no way to load them.
    let original = oneshot_player::original_language(&*provider, &item).await;
    let audio = tracks::select_audio(source, TrackRequest::Auto, &settings.playback.preferred_audio_languages, original.as_deref());
    let subtitle = tracks::select_subtitle(source, TrackRequest::Auto, &settings.subtitles, audio);
    let delivery = DeliveryRequest::Transcode {
        video: Some(VideoCodec::H264),
        audio: Some(AudioCodec::Aac),
        max_bitrate: Some(max_bitrate),
        max_width: Some(1920),
        audio_channels: Some(2),
        burn_subtitle: subtitle.map(|s| s.index),
    };
    // The stream starts at 0 and the receiver seeks: both servers then keep
    // one timeline, the file's.
    let target = provider
        .stream(&StreamRequest {
            item: item.clone(),
            source_id: source.id.clone(),
            play_session_id: info.play_session_id.clone(),
            delivery,
            audio_index: audio.map(|a| a.index),
            subtitle_index: subtitle.map(|s| s.index),
            start_ms: 0,
        })
        .await?;

    let title = state.catalog.item(&item).await.map(|i| i.episode.as_ref().and_then(|e| e.series_title.clone()).map_or(i.title.clone(), |s| format!("{s} · {}", i.title))).unwrap_or_default();
    let content_type = if target.url.path().ends_with(".m3u8") { "application/x-mpegURL" } else { "video/mp4" };
    let media = CastMedia { url: target.url, headers: target.headers, content_type: content_type.into(), title, start_ms, duration_ms: source.duration_ms };

    // The receiver takes over: nothing plays here any more.
    let _ = state.player.command(PlayerCommand::Stop);
    state.cast.start(&device, media, state.http()).await?;
    let generation = CAST_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

    if settings.privacy.report_progress {
        let report = Reporter { provider, item, source_id: source.id.clone(), session: info.play_session_id, audio: audio.map(|a| a.index), subtitle: subtitle.map(|s| s.index), generation, interval: Duration::from_secs(u64::from(settings.playback.report_interval_secs.max(5))) };
        tauri::async_runtime::spawn(report.run(Arc::clone(&state)));
    }
    Ok(())
}

#[tauri::command]
pub async fn cast_command(state: St<'_>, command: CastCommand) -> Result<()> {
    state.cast.command(command).await
}

#[tauri::command]
pub async fn cast_status(state: St<'_>) -> Result<CastStatus> {
    Ok(state.cast.status().await)
}

/// Ends the cast; the position it reached, to carry on locally.
#[tauri::command]
pub async fn cast_stop(state: St<'_>) -> Result<Option<u64>> {
    Ok(state.cast.stop().await)
}

/// Tells the server how the cast title is going (resume point, "now
/// playing"), as the local player does, until the cast ends.
struct Reporter {
    provider: Arc<dyn oneshot_core::provider::MediaProvider>,
    item: ItemRef,
    source_id: String,
    session: Option<String>,
    audio: Option<u32>,
    subtitle: Option<u32>,
    generation: u64,
    interval: Duration,
}

impl Reporter {
    async fn send(&self, kind: ReportKind, s: &CastStatus) {
        let state = match (kind, s.state) {
            (ReportKind::Stop, _) => PlaybackState::Stopped,
            (_, CastState::Paused) => PlaybackState::Paused,
            (_, CastState::Buffering | CastState::Loading) => PlaybackState::Buffering,
            _ => PlaybackState::Playing,
        };
        let report = PlaybackReport {
            kind,
            item: self.item.clone(),
            source_id: self.source_id.clone(),
            play_session_id: self.session.clone(),
            state,
            position_ms: s.position_ms,
            duration_ms: s.duration_ms,
            delivery: DeliveryKind::Transcode,
            audio_index: self.audio,
            subtitle_index: self.subtitle,
            volume: s.volume.map_or(100, |v| (v * 100.0).round() as u8),
            muted: false,
        };
        if let Err(e) = self.provider.report(&report).await {
            tracing::warn!(target: "cast", "progress report failed: {e}");
        }
    }

    async fn run(self, app: Arc<AppState>) {
        let mut last = app.cast.status().await;
        self.send(ReportKind::Start, &last).await;
        loop {
            tokio::time::sleep(self.interval).await;
            let now = app.cast.status().await;
            let mine = now.device.is_some() && CAST_GENERATION.load(Ordering::SeqCst) == self.generation;
            if !mine || matches!(now.state, CastState::Ended | CastState::Error | CastState::Idle) {
                // Over, or replaced by another cast: close with the last known position.
                let end = if mine { &now } else { &last };
                if CAST_GENERATION.load(Ordering::SeqCst) != self.generation {
                    // A newer cast owns the receiver now; this title's last word.
                    self.send(ReportKind::Stop, &last).await;
                    return;
                }
                self.send(ReportKind::Stop, end).await;
                return;
            }
            self.send(ReportKind::Progress, &now).await;
            last = now;
        }
    }
}
