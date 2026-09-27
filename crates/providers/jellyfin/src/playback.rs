//! PlaybackInfo negotiation, stream URLs and progress reporting.

use oneshot_core::ids::ItemRef;
use oneshot_core::playback::{
    ClientProfile, DeliveryKind, DeliveryRequest, ExternalSubtitle, PlaybackInfo, PlaybackReport, PlaybackState,
    ReportKind, ServerPolicy, SourceOffer, StreamRequest, StreamTarget,
};
use oneshot_core::{Error, Result};
use oneshot_core::provider::MediaProvider;
use oneshot_net::reqwest::Method;

use crate::dto::{MediaSourceInfo, PlaybackInfoRequest, PlaybackInfoResponse, PlaybackProgressInfo};
use crate::map::{self, TICKS_PER_MS};
use crate::profile::device_profile;
use crate::provider::JellyfinProvider;

async fn negotiate(
    p: &JellyfinProvider,
    item: &str,
    body: &PlaybackInfoRequest,
) -> Result<PlaybackInfoResponse> {
    let resp = p
        .request(Method::POST, &format!("Items/{item}/PlaybackInfo"), &[("userId", p.user_id().to_owned())])?
        .json(body)
        .send()
        .await
        .map_err(oneshot_net::map_err)?;
    let info: PlaybackInfoResponse = oneshot_net::json(resp).await?;
    if let Some(code) = &info.error_code {
        // e.g. NotAllowed (parental control), NoCompatibleStream, RateLimitExceeded
        return Err(match code.as_str() {
            "NotAllowed" => Error::Forbidden("playback not allowed for this user".into()),
            other => Error::Playback(format!("server refused playback: {other}")),
        });
    }
    Ok(info)
}

fn request_body(p: &JellyfinProvider, profile: &serde_json::Value, max_bitrate: Option<u64>) -> PlaybackInfoRequest {
    PlaybackInfoRequest {
        user_id: p.user_id().to_owned(),
        max_streaming_bitrate: max_bitrate,
        start_time_ticks: None,
        audio_stream_index: None,
        subtitle_stream_index: None,
        media_source_id: None,
        device_profile: profile.clone(),
        enable_direct_play: true,
        enable_direct_stream: true,
        enable_transcoding: true,
        allow_video_stream_copy: true,
        allow_audio_stream_copy: true,
        auto_open_live_stream: true,
    }
}

fn policy(src: &MediaSourceInfo) -> ServerPolicy {
    ServerPolicy {
        direct_play_allowed: src.supports_direct_play,
        direct_stream_allowed: src.supports_direct_stream,
        transcode_allowed: src.supports_transcoding,
        server_reasons: src.transcode_reasons.clone(),
    }
}

pub async fn playback_info(p: &JellyfinProvider, id: &ItemRef, profile: &ClientProfile) -> Result<PlaybackInfo> {
    let body = request_body(p, &device_profile(profile), profile.max_bitrate);
    let info = negotiate(p, &id.key, &body).await?;
    tracing::info!(
        target: "playback",
        item = %id,
        sources = info.media_sources.len(),
        direct_play = ?info.media_sources.iter().map(|s| s.supports_direct_play).collect::<Vec<_>>(),
        reasons = ?info.media_sources.iter().flat_map(|s| s.transcode_reasons.iter()).collect::<Vec<_>>(),
        "jellyfin PlaybackInfo"
    );
    Ok(PlaybackInfo {
        item: id.clone(),
        offers: info.media_sources.iter().map(|s| SourceOffer { source: map::source(s), policy: policy(s) }).collect(),
        play_session_id: info.play_session_id,
    })
}

pub async fn stream(p: &JellyfinProvider, req: &StreamRequest) -> Result<StreamTarget> {
    let headers = vec![("Authorization".to_owned(), p.identity.header(Some(&p.token)))];
    match &req.delivery {
        DeliveryRequest::Direct => {
            let mut url = p.url(&format!("Videos/{}/stream", req.item.key))?;
            {
                let mut q = url.query_pairs_mut();
                q.append_pair("static", "true").append_pair("mediaSourceId", &req.source_id);
                if let Some(ps) = &req.play_session_id {
                    q.append_pair("PlaySessionId", ps);
                }
            }
            // Sidecar subtitle files are not in the container: side-load them.
            let item = p.item(&req.item).await.ok();
            let externals = item
                .as_ref()
                .and_then(|i| i.sources.iter().find(|s| s.id == req.source_id))
                .map(|s| s.subtitles.iter().filter(|s| s.external).cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            let external_subtitles = externals
                .iter()
                .filter_map(|s| {
                    let path = s.delivery_path.clone().unwrap_or_else(|| {
                        format!("Videos/{}/{}/Subtitles/{}/0/Stream.srt", req.item.key, req.source_id, s.index)
                    });
                    Some(ExternalSubtitle {
                        stream_index: s.index,
                        url: p.url(&path).ok()?,
                        title: s.title.clone(),
                        language: s.language.clone(),
                    })
                })
                .collect();
            Ok(StreamTarget { url, headers, external_subtitles })
        }
        DeliveryRequest::Remux { .. } | DeliveryRequest::Transcode { .. } => {
            let (video_copy, audio_copy, max_bitrate) = match &req.delivery {
                DeliveryRequest::Transcode { video, audio, max_bitrate, .. } => (video.is_none(), audio.is_none(), *max_bitrate),
                _ => (true, true, None),
            };
            let mut body = request_body(p, &device_profile(&transcode_profile(max_bitrate)), max_bitrate);
            body.enable_direct_play = false;
            body.enable_direct_stream = matches!(req.delivery, DeliveryRequest::Remux { .. });
            body.allow_video_stream_copy = video_copy;
            body.allow_audio_stream_copy = audio_copy;
            body.media_source_id = Some(req.source_id.clone());
            body.audio_stream_index = req.audio_index;
            body.subtitle_stream_index = req.subtitle_index.map(i64::from);
            body.start_time_ticks = Some(req.start_ms as i64 * TICKS_PER_MS);
            let info = negotiate(p, &req.item.key, &body).await?;
            let src = info
                .media_sources
                .iter()
                .find(|s| s.id == req.source_id)
                .or(info.media_sources.first())
                .ok_or_else(|| Error::Playback("server returned no media source".into()))?;
            let path = src
                .transcoding_url
                .as_deref()
                .ok_or_else(|| Error::Playback(format!("server offered no stream URL ({:?})", src.transcode_reasons)))?;
            tracing::info!(target: "playback", reasons = ?src.transcode_reasons, "jellyfin transcode/remux url obtained");
            Ok(StreamTarget { url: p.url(path)?, headers, external_subtitles: Vec::new() })
        }
    }
}

/// Profile for transcode requests: the transcoding profile is what matters;
/// direct play entries are irrelevant because direct play is disabled.
fn transcode_profile(max_bitrate: Option<u64>) -> ClientProfile {
    use oneshot_core::stream::{AudioCodec, SubtitleFormat, VideoCodec};
    ClientProfile {
        name: "Flick (transcode)".into(),
        max_bitrate,
        video_codecs: vec![VideoCodec::Hevc, VideoCodec::H264],
        audio_codecs: vec![AudioCodec::Aac, AudioCodec::Ac3, AudioCodec::Eac3],
        containers: vec!["ts".into()],
        subtitle_formats: vec![SubtitleFormat::Srt, SubtitleFormat::Ass, SubtitleFormat::WebVtt],
        max_width: 7680,
        max_height: 4320,
        max_audio_channels: 8,
    }
}

pub async fn report(p: &JellyfinProvider, r: &PlaybackReport) -> Result<()> {
    let body = PlaybackProgressInfo {
        item_id: r.item.key.clone(),
        media_source_id: r.source_id.clone(),
        play_session_id: r.play_session_id.clone(),
        position_ticks: r.position_ms as i64 * TICKS_PER_MS,
        is_paused: r.state == PlaybackState::Paused,
        is_muted: r.muted,
        volume_level: r.volume,
        play_method: match r.delivery {
            DeliveryKind::DirectPlay => "DirectPlay",
            DeliveryKind::DirectStream => "DirectStream",
            DeliveryKind::Transcode => "Transcode",
        },
        audio_stream_index: r.audio_index,
        subtitle_stream_index: Some(r.subtitle_index.map_or(-1, i64::from)),
        can_seek: true,
        event_name: match (r.kind, r.state) {
            (ReportKind::Progress, PlaybackState::Paused) => Some("pause"),
            (ReportKind::Progress, _) => Some("timeupdate"),
            _ => None,
        },
    };
    let path = match r.kind {
        ReportKind::Start => "Sessions/Playing",
        ReportKind::Progress => "Sessions/Playing/Progress",
        ReportKind::Stop => "Sessions/Playing/Stopped",
    };
    let resp = p.request(Method::POST, path, &[])?.json(&body).send().await.map_err(oneshot_net::map_err)?;
    oneshot_net::ensure_ok(resp).await.map(drop)
}
