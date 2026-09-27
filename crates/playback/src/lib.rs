//! Playback decision engine.
//!
//! [`decide`] is a pure function: same inputs → same decision. It never
//! touches the OS, the network or mpv, which is what makes the Direct Play
//! guarantees testable. Every branch records a [`DecisionReason`] so the
//! player's "Advanced" overlay and the Debug panel can explain *why*.

mod profile;
mod reasons;
mod tracks;

use oneshot_core::capabilities::{AudioDevice, CapabilityReport, DisplayCapabilities, HdrState};
use oneshot_core::playback::{
    AudioOutputPlan, DecisionReason, DeliveryRequest, PlaybackDecision, ReasonSeverity, SourceOffer, StrategyLabel,
    SubtitlePlan, VideoOutputPlan,
};
use oneshot_core::settings::{HardwareDecoding, HdrMode, Settings};
use oneshot_core::stream::{
    AudioCodec, AudioStream, BitstreamFormat, DolbyVisionCompat, DynamicRange, SubtitleStream, VideoCodec, VideoStream,
};

pub use profile::client_profile;
pub use reasons::codes;
pub use tracks::TrackRequest;

/// Everything the engine looks at.
#[derive(Debug, Clone, Copy)]
pub struct DecisionInput<'a> {
    pub offer: &'a SourceOffer,
    pub caps: &'a CapabilityReport,
    /// OS id of the display the player window is on; `None` = primary.
    pub display_id: Option<&'a str>,
    pub settings: &'a Settings,
    pub audio: TrackRequest,
    pub subtitle: TrackRequest,
}

/// The source cannot be played with the current settings/server policy.
#[derive(Debug, Clone, thiserror::Error)]
#[error("this media cannot be played: {}", .reasons.iter().filter(|r| r.severity == ReasonSeverity::Blocking).map(|r| r.message.as_str()).collect::<Vec<_>>().join("; "))]
pub struct Unplayable {
    pub reasons: Vec<DecisionReason>,
}

#[derive(Debug, Default)]
struct Log(Vec<DecisionReason>);

impl Log {
    fn push(&mut self, code: &str, severity: ReasonSeverity, message: impl Into<String>) {
        self.0.push(DecisionReason { code: code.to_owned(), severity, message: message.into() });
    }
    fn info(&mut self, code: &str, m: impl Into<String>) {
        self.push(code, ReasonSeverity::Info, m);
    }
    fn degraded(&mut self, code: &str, m: impl Into<String>) {
        self.push(code, ReasonSeverity::Degraded, m);
    }
    fn blocking(&mut self, code: &str, m: impl Into<String>) {
        self.push(code, ReasonSeverity::Blocking, m);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VideoDecode {
    Hardware,
    Software,
    /// Hardware support could not be determined.
    Unknown,
    Unsupported,
}

pub fn decide(input: &DecisionInput<'_>) -> Result<PlaybackDecision, Unplayable> {
    let DecisionInput { offer, caps, settings, .. } = *input;
    let source = &offer.source;
    let mut log = Log::default();

    let video = source.primary_video();
    let audio = tracks::select_audio(source, input.audio, &settings.playback.preferred_audio_languages);
    let subtitle = tracks::select_subtitle(source, input.subtitle, &settings.subtitles, audio);

    // --- 1. Local decodability -------------------------------------------
    let decode = video.map(|v| assess_video(v, caps, settings, &mut log));
    let audio_decodable = audio.is_none_or(|a| !matches!(a.codec, AudioCodec::Other(_)));
    if !audio_decodable {
        log.degraded(codes::AUDIO_CODEC_UNSUPPORTED, "Audio codec unknown to the local engine");
    }

    // --- 2. Delivery -----------------------------------------------------
    let mut transcode_video: Option<VideoCodec> = None;
    let mut transcode_audio: Option<AudioCodec> = None;
    let mut max_bitrate: Option<u64> = None;

    if let (Some(v), Some(d)) = (video, decode) {
        if d == VideoDecode::Unsupported {
            transcode_video = Some(transcode_target(caps));
        } else if d == VideoDecode::Software
            && settings.playback.transcode_without_hwdec_min_height.is_some_and(|h| v.height >= h)
        {
            log.info(
                codes::TRANSCODE_NO_HWDEC_POLICY,
                format!("{}p {} without GPU decoding: server transcode preferred by settings", v.height, v.codec.label()),
            );
            transcode_video = Some(transcode_target(caps));
        }
    }
    if !audio_decodable {
        transcode_audio = Some(AudioCodec::Aac);
    }
    if let (Some(limit), Some(rate)) = (settings.playback.max_bitrate, source.bitrate)
        && rate > limit
    {
        log.info(
            codes::BITRATE_LIMIT,
            format!("Source bitrate {:.1} Mb/s exceeds the {:.1} Mb/s limit", mbps(rate), mbps(limit)),
        );
        max_bitrate = Some(limit);
        transcode_video.get_or_insert_with(|| video.map_or(VideoCodec::H264, |v| v.codec.clone()));
    }

    let needs_transcode = transcode_video.is_some() || transcode_audio.is_some() || max_bitrate.is_some();
    let policy = &offer.policy;
    for r in &policy.server_reasons {
        log.info(codes::SERVER_REASON, format!("Server: {r}"));
    }

    let delivery = if !needs_transcode && policy.direct_play_allowed {
        log.info(codes::DIRECT_PLAY, "Original file streamed untouched");
        DeliveryRequest::Direct
    } else if !needs_transcode && policy.direct_stream_allowed && settings.playback.allow_direct_stream {
        log.info(codes::DIRECT_STREAM, "Server refuses direct play; remuxing without re-encoding");
        DeliveryRequest::Remux { container: "mkv".into() }
    } else if policy.transcode_allowed && settings.playback.allow_transcode {
        if !policy.direct_play_allowed && !needs_transcode {
            log.degraded(codes::SERVER_FORCED_TRANSCODE, "Server policy forces a transcode");
            transcode_video = video.map(|_| transcode_target(caps));
        }
        DeliveryRequest::Transcode {
            video: transcode_video.clone(),
            audio: transcode_audio.clone(),
            max_bitrate,
            max_width: None,
            audio_channels: None,
            burn_subtitle: None,
        }
    } else {
        log.blocking(
            codes::NO_DELIVERY,
            if settings.playback.allow_transcode {
                "The server does not allow direct play, remux or transcoding for this user"
            } else {
                "Transcoding is disabled in settings and the media cannot be played directly"
            },
        );
        return Err(Unplayable { reasons: log.0 });
    };
    let transcoding_video = matches!(&delivery, DeliveryRequest::Transcode { video: Some(_), .. });
    let transcoding_audio = matches!(&delivery, DeliveryRequest::Transcode { audio: Some(_), .. });

    // --- 3. Local output paths --------------------------------------------
    let display = input.display_id.and_then(|id| caps.display(id)).or_else(|| caps.primary_display());
    let video_plan = match video {
        None => VideoOutputPlan::Sdr,
        Some(_) if transcoding_video => VideoOutputPlan::ServerDetermined,
        Some(v) => video_output(v, display, settings, &mut log),
    };
    let device = settings
        .audio
        .device
        .as_deref()
        .and_then(|id| caps.audio.device(id))
        .or_else(|| caps.audio.default_output());
    let audio_plan = match audio {
        None => AudioOutputPlan::None,
        Some(_) if transcoding_audio => AudioOutputPlan::ServerDetermined,
        Some(a) => audio_output(a, device, settings, &mut log),
    };
    let subtitle_plan = subtitle_plan(subtitle, &delivery, &mut log);
    let delivery = match (delivery, &subtitle_plan) {
        (DeliveryRequest::Transcode { video, audio, max_bitrate, max_width, audio_channels, .. }, SubtitlePlan::BurnIn { index }) => {
            DeliveryRequest::Transcode { video, audio, max_bitrate, max_width, audio_channels, burn_subtitle: Some(*index) }
        }
        (d, _) => d,
    };

    let label = match &delivery {
        DeliveryRequest::Transcode { .. } => StrategyLabel::ServerTranscode,
        DeliveryRequest::Remux { .. } => StrategyLabel::DirectStream,
        DeliveryRequest::Direct if is_local_conversion(&video_plan, &audio_plan) => StrategyLabel::LocalDecode,
        DeliveryRequest::Direct => StrategyLabel::DirectPlay,
    };

    Ok(PlaybackDecision {
        label,
        source_id: source.id.clone(),
        delivery,
        video_stream: video.map(|v| v.index),
        audio_stream: audio.map(|a| a.index),
        video: video_plan,
        audio: audio_plan,
        subtitles: subtitle_plan,
        hardware_decode: match decode {
            Some(VideoDecode::Hardware) => Some(true),
            Some(VideoDecode::Software) => Some(false),
            _ => None,
        },
        reasons: log.0,
    })
}

fn mbps(bps: u64) -> f64 {
    bps as f64 / 1_000_000.0
}

fn is_local_conversion(video: &VideoOutputPlan, audio: &AudioOutputPlan) -> bool {
    matches!(video, VideoOutputPlan::ToneMapToSdr { .. } | VideoOutputPlan::DolbyVisionReshape { .. })
        || matches!(
            audio,
            AudioOutputPlan::Pcm { downmix: true, .. }
                | AudioOutputPlan::Pcm { spatial_lost: true, .. }
                | AudioOutputPlan::Bitstream { reencoded: true, .. }
        )
}

fn bit_depth(v: &VideoStream) -> u8 {
    v.bit_depth.unwrap_or(if v.range.is_hdr() { 10 } else { 8 })
}

fn assess_video(v: &VideoStream, caps: &CapabilityReport, settings: &Settings, log: &mut Log) -> VideoDecode {
    let label = format!("{} {}-bit", v.codec.label(), bit_depth(v));
    if matches!(v.codec, VideoCodec::Other(_)) {
        log.degraded(codes::VIDEO_CODEC_UNSUPPORTED, format!("Video codec {} unknown to the local engine", v.codec.label()));
        return VideoDecode::Unsupported;
    }
    if settings.video.hardware_decoding == HardwareDecoding::Off {
        log.info(codes::HWDEC_DISABLED, "Hardware decoding disabled in settings: CPU decoding");
        return VideoDecode::Software;
    }
    if !caps.video.hardware_probe_ok {
        log.info(codes::HWDEC_UNKNOWN, format!("{label}: GPU decoder support unknown on this platform; mpv will try and fall back to CPU"));
        return VideoDecode::Unknown;
    }
    match caps.video.hw_decoder(&v.codec, bit_depth(v)) {
        Some(d) => {
            log.info(codes::HWDEC, format!("{label} decoded by the GPU ({}, {})", d.api, d.profile));
            VideoDecode::Hardware
        }
        None => {
            log.degraded(codes::SWDEC, format!("No GPU decoder for {label}: CPU decoding (higher power use, may drop frames at 4K)"));
            VideoDecode::Software
        }
    }
}

/// Codec to request when the server must re-encode: the best one we can
/// hardware-decode, falling back to universally supported H.264.
fn transcode_target(caps: &CapabilityReport) -> VideoCodec {
    if caps.video.hw_decoder(&VideoCodec::Hevc, 10).is_some() { VideoCodec::Hevc } else { VideoCodec::H264 }
}

fn video_output(
    v: &VideoStream,
    display: Option<&DisplayCapabilities>,
    settings: &Settings,
    log: &mut Log,
) -> VideoOutputPlan {
    if !v.range.is_hdr() {
        return VideoOutputPlan::Sdr;
    }
    let hdr_state = display.map(|d| &d.hdr);
    let display_name = display.map_or("the display", |d| d.name.as_str());
    let passthrough = settings.video.hdr == HdrMode::Auto && hdr_state.is_some_and(HdrState::is_active);

    let why_sdr = || -> String {
        if settings.video.hdr == HdrMode::ForceSdr {
            return "HDR disabled in settings".into();
        }
        match hdr_state {
            Some(HdrState::SupportedButOff) => format!("{display_name} supports HDR but HDR is off in the OS"),
            Some(HdrState::Unsupported) => format!("{display_name} is not HDR capable"),
            Some(HdrState::Unknown { reason }) => format!("HDR state of {display_name} unknown ({reason})"),
            None => "no display information".into(),
            Some(HdrState::Active { .. }) => unreachable!("active HDR handled by passthrough"),
        }
    };

    if let DynamicRange::DolbyVision { profile, compat, enhancement_layer } = &v.range {
        if *enhancement_layer {
            log.degraded(codes::DV_FEL_IGNORED, "Dolby Vision enhancement layer (profile 7 FEL) is not applied; base layer + RPU used");
        }
        let output = if passthrough { "HDR10 (PQ)" } else { "SDR" };
        let profile = profile.map_or_else(String::new, |p| format!(" profile {p}"));
        let detail = match compat {
            DolbyVisionCompat::None => "RPU reshaping is required (no fallback layer)",
            DolbyVisionCompat::Hdr10 => "HDR10 base layer with Dolby Vision metadata",
            DolbyVisionCompat::Sdr => "SDR base layer with Dolby Vision metadata",
            DolbyVisionCompat::Hlg => "HLG base layer with Dolby Vision metadata",
        };
        log.degraded(
            codes::DV_RESHAPE,
            format!("Dolby Vision{profile}: {detail}; output as {output}. The display does not receive a Dolby Vision signal"),
        );
        if !passthrough {
            log.info(codes::HDR_TONEMAP, why_sdr());
        }
        return VideoOutputPlan::DolbyVisionReshape { output: output.into() };
    }

    if passthrough {
        log.info(codes::HDR_PASSTHROUGH, format!("{} signalled to {display_name} (OS HDR active)", v.range.label()));
        VideoOutputPlan::HdrPassthrough { format: v.range.label().into() }
    } else {
        let reason = why_sdr();
        log.degraded(codes::HDR_TONEMAP, format!("{} tone-mapped to SDR: {reason}", v.range.label()));
        VideoOutputPlan::ToneMapToSdr { reason }
    }
}

fn audio_output(a: &AudioStream, device: Option<&AudioDevice>, settings: &Settings, log: &mut Log) -> AudioOutputPlan {
    let prefs = &settings.audio;
    let Some(device) = device else {
        log.degraded(codes::AUDIO_DEVICE_UNKNOWN, "No audio device information: decoding to PCM with OS defaults");
        return AudioOutputPlan::Pcm {
            source_channels: a.channels,
            output_channels: a.channels.min(2),
            downmix: a.channels > 2,
            spatial_lost: a.spatial.is_some(),
        };
    };

    if let Some(format) = a.codec.bitstream_format() {
        if prefs.passthrough {
            let allowed = |f: BitstreamFormat| prefs.passthrough_formats.contains(&f) && device.passthrough.supports(f);
            if allowed(format) {
                log.info(codes::BITSTREAM, format!("{} bitstreamed to {}", a.codec.label(), device.name));
                return AudioOutputPlan::Bitstream { format, device: device.name.clone(), reencoded: false };
            }
            if format == BitstreamFormat::DtsHd && allowed(BitstreamFormat::Dts) {
                log.degraded(
                    codes::BITSTREAM_DTS_CORE,
                    format!("{} does not accept DTS-HD: bitstreaming the lossy DTS core", device.name),
                );
                return AudioOutputPlan::Bitstream { format: BitstreamFormat::Dts, device: device.name.clone(), reencoded: false };
            }
            let why = if !prefs.passthrough_formats.contains(&format) {
                format!("{} passthrough is disabled in settings", a.codec.label())
            } else {
                format!("{} rejected {} passthrough during the device probe", device.name, a.codec.label())
            };
            log.degraded(codes::BITSTREAM_UNAVAILABLE, format!("{why}: decoding to PCM"));
        } else if a.spatial.is_some() {
            log.degraded(codes::SPATIAL_NEEDS_BITSTREAM, "Object audio (Atmos/DTS:X) needs passthrough, which is disabled");
        }
    }

    if prefs.ac3_reencode && a.channels > 2 && device.passthrough.supports(BitstreamFormat::Ac3) {
        log.degraded(
            codes::AC3_REENCODE,
            format!("{} decoded and re-encoded to Dolby Digital 5.1 for {} (lossy)", a.codec.label(), device.name),
        );
        return AudioOutputPlan::Bitstream { format: BitstreamFormat::Ac3, device: device.name.clone(), reencoded: true };
    }

    let output_channels = prefs.channels.channels().unwrap_or(device.channels).max(1);
    let downmix = a.channels > output_channels;
    let spatial_lost = a.spatial.is_some();
    if downmix {
        log.degraded(
            codes::DOWNMIX,
            format!("{} downmixed to {} channels for {}", a.channels_label(), output_channels, device.name),
        );
    } else {
        log.info(codes::PCM, format!("{} decoded to {} PCM channels", a.channels_label(), a.channels));
    }
    if spatial_lost {
        log.degraded(codes::SPATIAL_LOST, "Atmos/DTS:X objects are not reproduced; the channel bed is played");
    }
    AudioOutputPlan::Pcm { source_channels: a.channels, output_channels, downmix, spatial_lost }
}

fn subtitle_plan(subtitle: Option<&SubtitleStream>, delivery: &DeliveryRequest, log: &mut Log) -> SubtitlePlan {
    let Some(s) = subtitle else { return SubtitlePlan::None };
    let transcoding_video = matches!(delivery, DeliveryRequest::Transcode { video: Some(_), .. });
    if transcoding_video && s.format.is_bitmap() && !s.external {
        log.degraded(
            codes::SUBTITLE_BURN_IN,
            "Bitmap subtitle embedded in a transcoded stream: the server must burn it into the video",
        );
        return SubtitlePlan::BurnIn { index: s.index };
    }
    let external = s.external || transcoding_video;
    log.info(codes::SUBTITLE_LOCAL, "Subtitles rendered locally over the video");
    SubtitlePlan::Local { index: s.index, external }
}

#[cfg(test)]
mod tests;
