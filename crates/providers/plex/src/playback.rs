//! Plex playback: direct part URLs, universal transcoder, timeline reports.

use oneshot_core::ids::ItemRef;
use oneshot_core::playback::{
    ClientProfile, DeliveryRequest, ExternalSubtitle, PlaybackInfo, PlaybackReport, PlaybackState, ReportKind,
    ServerPolicy, SourceOffer, StreamRequest, StreamTarget,
};
use oneshot_core::stream::{AudioCodec, VideoCodec};
use oneshot_core::{Error, Result};
use oneshot_net::reqwest::Method;
use serde::Deserialize;

use crate::dto::{Envelope, Media, Metadata};
use crate::map;
use crate::provider::PlexProvider;

/// `X-Plex-Client-Profile-Extra` declaring what we direct-play. Plex's
/// "Generic" base profile is conservative; without this the server would
/// transcode HEVC/TrueHD/DTS for no reason.
pub fn profile_extra(p: &ClientProfile) -> String {
    let video: Vec<&str> = p
        .video_codecs
        .iter()
        .filter_map(|c| match c {
            VideoCodec::H264 => Some("h264"),
            VideoCodec::Hevc => Some("hevc"),
            VideoCodec::Av1 => Some("av1"),
            VideoCodec::Vp9 => Some("vp9"),
            VideoCodec::Vp8 => Some("vp8"),
            VideoCodec::Mpeg2 => Some("mpeg2video"),
            VideoCodec::Mpeg4 => Some("mpeg4"),
            VideoCodec::Vc1 => Some("vc1"),
            VideoCodec::Other(_) => None,
        })
        .collect();
    let audio: Vec<&str> = p
        .audio_codecs
        .iter()
        .flat_map(|c| match c {
            AudioCodec::Aac => &["aac"][..],
            AudioCodec::Ac3 => &["ac3"],
            AudioCodec::Eac3 => &["eac3"],
            AudioCodec::Dts | AudioCodec::DtsHd => &["dca", "dca-ma"],
            AudioCodec::TrueHd => &["truehd"],
            AudioCodec::Flac => &["flac"],
            AudioCodec::Alac => &["alac"],
            AudioCodec::Opus => &["opus"],
            AudioCodec::Vorbis => &["vorbis"],
            AudioCodec::Mp3 => &["mp3"],
            AudioCodec::Pcm => &["pcm"],
            AudioCodec::Other(_) => &[],
        })
        .copied()
        .collect();
    format!(
        "add-direct-play-profile(type=videoProfile&container={}&videoCodec={}&audioCodec={}&subtitleCodec=*)\
         +add-transcode-target(type=videoProfile&context=streaming&protocol=hls&container=mpegts&videoCodec=hevc,h264&audioCodec=aac,ac3,eac3)\
         +add-limitation(scope=videoAudioCodec&scopeName=*&type=upperBound&name=audio.channels&value={})",
        p.containers.join(","),
        video.join(","),
        audio.join(","),
        p.max_audio_channels
    )
}

/// Decision payload. PMS ≥ 1.40 answers through its "MDE" (media decision
/// engine: `mdeDecisionCode` + per-part `decision`), older servers through
/// `directPlayDecisionCode`; both are read.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Decision {
    general_decision_code: Option<u32>,
    general_decision_text: Option<String>,
    direct_play_decision_code: Option<u32>,
    direct_play_decision_text: Option<String>,
    transcode_decision_text: Option<String>,
    mde_decision_code: Option<u32>,
    mde_decision_text: Option<String>,
    #[serde(rename = "Metadata", default)]
    metadata: Vec<DecisionMetadata>,
}

#[derive(Debug, Default, Deserialize)]
struct DecisionMetadata {
    #[serde(rename = "Media", default)]
    media: Vec<DecisionMedia>,
}

#[derive(Debug, Default, Deserialize)]
struct DecisionMedia {
    #[serde(rename = "Part", default)]
    parts: Vec<DecisionPart>,
}

#[derive(Debug, Default, Deserialize)]
struct DecisionPart {
    decision: Option<String>,
}

impl Decision {
    fn part_decision(&self) -> Option<&str> {
        self.metadata.first()?.media.first()?.parts.first()?.decision.as_deref()
    }

    fn direct_play(&self) -> bool {
        self.part_decision() == Some("directplay")
            || self.direct_play_decision_code == Some(1000)
            || (self.part_decision().is_none() && self.mde_decision_code == Some(1000))
    }
}

async fn decision(p: &PlexProvider, item: &str, media_index: usize, extra: &str) -> Result<Decision> {
    let q = [
        ("path", format!("/library/metadata/{item}")),
        ("mediaIndex", media_index.to_string()),
        ("partIndex", "0".into()),
        ("protocol", "http".into()),
        ("directPlay", "1".into()),
        ("directStream", "1".into()),
        ("hasMDE", "1".into()),
        ("X-Plex-Client-Profile-Extra", extra.to_owned()),
    ];
    Ok(p.get::<Envelope<Decision>>("video/:/transcode/universal/decision", &q).await?.container)
}

pub async fn playback_info(p: &PlexProvider, id: &ItemRef, profile: &ClientProfile) -> Result<PlaybackInfo> {
    let m = p.metadata(&id.key).await?;
    let extra = profile_extra(profile);
    let mut offers = Vec::new();
    for (i, media) in m.media.iter().enumerate() {
        let policy = match decision(p, &id.key, i, &extra).await {
            // 1000 = direct play OK. Anything else carries a server reason
            // (e.g. remote bitrate limits, Plex Pass requirement for remote).
            Ok(d) => {
                let direct = d.direct_play();
                let reasons: Vec<String> = [d.general_decision_text, d.direct_play_decision_text, d.transcode_decision_text, d.mde_decision_text]
                    .into_iter()
                    .flatten()
                    .filter(|t| !direct || !t.contains("Direct play OK"))
                    .collect();
                let blocked = d.general_decision_code.is_some_and(|c| (2000..3000).contains(&c) || c >= 4000);
                ServerPolicy {
                    direct_play_allowed: direct,
                    direct_stream_allowed: !blocked,
                    transcode_allowed: !blocked,
                    server_reasons: reasons,
                }
            }
            Err(e @ (Error::Unauthorized | Error::Forbidden(_))) => return Err(e),
            Err(e) => {
                tracing::warn!(target: "playback", "plex decision endpoint failed ({e}); assuming direct play is allowed");
                let mut policy = ServerPolicy::permissive();
                policy.server_reasons.push(format!("decision endpoint unavailable: {e}"));
                policy
            }
        };
        offers.push(SourceOffer { source: map::source(media), policy });
    }
    tracing::info!(target: "playback", item = %id, offers = offers.len(),
        direct_play = ?offers.iter().map(|o| o.policy.direct_play_allowed).collect::<Vec<_>>(), "plex decision");
    Ok(PlaybackInfo { item: id.clone(), offers, play_session_id: Some(uuid::Uuid::new_v4().to_string()) })
}

fn find_media<'a>(m: &'a Metadata, source_id: &str) -> Result<(usize, &'a Media)> {
    m.media
        .iter()
        .enumerate()
        .find(|(_, media)| media.id.to_string() == source_id)
        .ok_or_else(|| Error::NotFound(format!("media version {source_id}")))
}

/// Plex selects streams by its own stream *id*; we address them by index.
fn stream_id(media: &Media, index: Option<u32>, stream_type: u8) -> Option<i64> {
    let index = index?;
    media.parts.first()?.streams.iter().find(|s| s.stream_type == stream_type && s.index == Some(index)).map(|s| s.id)
}

pub async fn stream(p: &PlexProvider, req: &StreamRequest) -> Result<StreamTarget> {
    let m = p.metadata(&req.item.key).await?;
    let (media_index, media) = find_media(&m, &req.source_id)?;
    let part = media.parts.first().ok_or_else(|| Error::Playback("media has no playable part".into()))?;
    let headers = p.identity.headers(&p.token);
    match &req.delivery {
        DeliveryRequest::Direct => {
            let external_subtitles = part
                .streams
                .iter()
                .filter(|s| s.stream_type == 3)
                .filter_map(|s| {
                    let key = s.key.as_ref()?;
                    Some(ExternalSubtitle {
                        stream_index: s.index.unwrap_or(10_000 + s.id as u32),
                        url: p.url(key).ok()?,
                        title: s.title.clone().or(s.display_title.clone()),
                        language: s.language_tag.clone().or(s.language_code.clone()),
                    })
                })
                .collect();
            Ok(StreamTarget { url: p.url(&part.key)?, headers, external_subtitles })
        }
        DeliveryRequest::Remux { .. } | DeliveryRequest::Transcode { .. } => {
            // Stream selection for the transcoder is persisted on the part.
            let audio = stream_id(media, req.audio_index, 2);
            let subtitle = stream_id(media, req.subtitle_index, 3);
            let mut sel: Vec<(&str, String)> = Vec::new();
            if let Some(a) = audio {
                sel.push(("audioStreamID", a.to_string()));
            }
            sel.push(("subtitleStreamID", subtitle.map_or_else(|| "0".into(), |s| s.to_string())));
            p.send_empty(Method::PUT, &format!("library/parts/{}", part.id), &sel).await?;

            let (burn, max_kbps, max_width, copy_video) = match &req.delivery {
                DeliveryRequest::Transcode { burn_subtitle, max_bitrate, max_width, video, .. } => {
                    (burn_subtitle.is_some(), max_bitrate.map(|b| b / 1000), *max_width, video.is_none())
                }
                _ => (false, None, None, true),
            };
            let session = req.play_session_id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let mut url = p.url("video/:/transcode/universal/start.m3u8")?;
            {
                let mut q = url.query_pairs_mut();
                q.append_pair("path", &format!("/library/metadata/{}", req.item.key))
                    .append_pair("mediaIndex", &media_index.to_string())
                    .append_pair("partIndex", "0")
                    .append_pair("protocol", "hls")
                    .append_pair("fastSeek", "1")
                    .append_pair("directPlay", "0")
                    .append_pair("directStream", if copy_video { "1" } else { "0" })
                    .append_pair("directStreamAudio", "1")
                    .append_pair("videoQuality", "100")
                    .append_pair("subtitles", if burn { "burn" } else { "auto" })
                    .append_pair("offset", &(req.start_ms / 1000).to_string())
                    .append_pair("session", &session)
                    .append_pair("X-Plex-Session-Identifier", &session);
                if let Some(k) = max_kbps {
                    q.append_pair("maxVideoBitrate", &k.to_string());
                }
                // Without a target resolution Plex keeps the source size and
                // spends the capped bitrate on a starved 4K picture.
                if let Some(w) = max_width {
                    q.append_pair("videoResolution", &format!("{w}x{}", w * 9 / 16));
                }
            }
            Ok(StreamTarget { url, headers, external_subtitles: Vec::new() })
        }
    }
}

pub async fn report(p: &PlexProvider, r: &PlaybackReport) -> Result<()> {
    let state = match (r.kind, r.state) {
        (ReportKind::Stop, _) | (_, PlaybackState::Stopped) => "stopped",
        (_, PlaybackState::Paused) => "paused",
        (_, PlaybackState::Buffering) => "buffering",
        _ => "playing",
    };
    let mut q = vec![
        ("ratingKey", r.item.key.clone()),
        ("key", format!("/library/metadata/{}", r.item.key)),
        ("state", state.to_owned()),
        ("time", r.position_ms.to_string()),
        ("hasMDE", "1".into()),
    ];
    if let Some(d) = r.duration_ms {
        q.push(("duration", d.to_string()));
    }
    if let Some(s) = &r.play_session_id {
        q.push(("X-Plex-Session-Identifier", s.clone()));
    }
    p.send_empty(Method::GET, ":/timeline", &q).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use oneshot_core::stream::SubtitleFormat;

    #[test]
    fn profile_extra_declares_hd_audio_and_channels() {
        let p = ClientProfile {
            name: "t".into(),
            max_bitrate: None,
            video_codecs: vec![VideoCodec::Hevc, VideoCodec::Av1],
            audio_codecs: vec![AudioCodec::TrueHd, AudioCodec::DtsHd],
            containers: vec!["mkv".into()],
            subtitle_formats: vec![SubtitleFormat::Pgs],
            max_width: 7680,
            max_height: 4320,
            max_audio_channels: 8,
        };
        let e = profile_extra(&p);
        assert!(e.contains("videoCodec=hevc,av1"));
        assert!(e.contains("truehd") && e.contains("dca-ma"));
        assert!(e.contains("name=audio.channels&value=8"));
    }
}
