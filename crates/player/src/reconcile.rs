//! Runtime reconciliation: the decision is made from *server metadata*, which
//! can be wrong (validated: Jellyfin 12.1 labels a PQ/BT.2020 HEVC file as
//! SDR). Once mpv has opened the stream, what it reports is authoritative.
//! These pure functions compare both and produce corrections + reasons.

use oneshot_core::capabilities::{DisplayCapabilities, HdrState};
use oneshot_core::playback::{AudioOutputPlan, DecisionReason, PlaybackDecision, ReasonSeverity, VideoOutputPlan};
use oneshot_core::settings::{HdrMode, Settings};

fn reason(code: &str, severity: ReasonSeverity, message: String) -> DecisionReason {
    DecisionReason { code: code.to_owned(), severity, message }
}

/// Transfer characteristic reported by mpv (`video-params/gamma`).
pub fn is_hdr_transfer(gamma: &str) -> bool {
    matches!(gamma, "pq" | "hlg")
}

/// Returns a corrected video plan when the stream's real transfer function
/// contradicts the plan built from metadata.
pub fn video(
    decision: &PlaybackDecision,
    observed_gamma: &str,
    display: Option<&DisplayCapabilities>,
    settings: &Settings,
) -> Option<(VideoOutputPlan, DecisionReason)> {
    let actual_hdr = is_hdr_transfer(observed_gamma);
    let planned_hdr = matches!(
        decision.video,
        VideoOutputPlan::HdrPassthrough { .. } | VideoOutputPlan::ToneMapToSdr { .. } | VideoOutputPlan::DolbyVisionReshape { .. }
    );
    if actual_hdr == planned_hdr || matches!(decision.video, VideoOutputPlan::ServerDetermined) {
        return None;
    }
    let label = if observed_gamma == "hlg" { "HLG" } else { "HDR10" };
    if !actual_hdr {
        return Some((
            VideoOutputPlan::Sdr,
            reason("metadata-mismatch", ReasonSeverity::Info, format!("Server metadata announced HDR but the stream is SDR ({observed_gamma})")),
        ));
    }
    let display_hdr = display.is_some_and(|d| d.hdr.is_active());
    let plan = if display_hdr && settings.video.hdr == HdrMode::Auto {
        VideoOutputPlan::HdrPassthrough { format: label.into() }
    } else {
        let why = match display.map(|d| &d.hdr) {
            Some(HdrState::SupportedButOff) => "display HDR is off in the OS",
            Some(HdrState::Unsupported) => "display is not HDR capable",
            _ if settings.video.hdr == HdrMode::ForceSdr => "HDR disabled in settings",
            _ => "display HDR state unknown",
        };
        VideoOutputPlan::ToneMapToSdr { reason: why.into() }
    };
    Some((
        plan,
        reason(
            "metadata-mismatch",
            ReasonSeverity::Degraded,
            format!("Server metadata announced SDR but the stream signals {label} ({observed_gamma}); output corrected"),
        ),
    ))
}

/// Hardware decoding expected but mpv fell back to software.
pub fn hwdec(decision: &PlaybackDecision, hwdec_current: &str) -> Option<DecisionReason> {
    (decision.hardware_decode == Some(true) && hwdec_current == "no").then(|| {
        reason(
            "hwdec-fallback",
            ReasonSeverity::Degraded,
            "The GPU decoder rejected this stream (profile/level/resolution); decoding on the CPU".into(),
        )
    })
}

/// The receiver refused the bitstream after the driver accepted it (EDID
/// mismatch, AVR off, HDMI switch in between): fall back to PCM.
pub fn bitstream_failed(decision: &PlaybackDecision) -> Option<DecisionReason> {
    match &decision.audio {
        AudioOutputPlan::Bitstream { format, device, .. } => Some(reason(
            "bitstream-runtime-failure",
            ReasonSeverity::Degraded,
            format!("{device} did not accept the {} bitstream when opened; switched to PCM", format.mpv_name()),
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use oneshot_core::capabilities::Rect;
    use oneshot_core::playback::{DeliveryRequest, StrategyLabel, SubtitlePlan};
    use oneshot_core::stream::BitstreamFormat;

    use super::*;

    fn decision(video: VideoOutputPlan) -> PlaybackDecision {
        PlaybackDecision {
            label: StrategyLabel::DirectPlay,
            source_id: "s".into(),
            delivery: DeliveryRequest::Direct,
            video_stream: Some(0),
            audio_stream: None,
            video,
            audio: AudioOutputPlan::Bitstream { format: BitstreamFormat::TrueHd, device: "AVR".into(), reencoded: false },
            subtitles: SubtitlePlan::None,
            hardware_decode: Some(true),
            reasons: vec![],
        }
    }

    fn display(hdr: HdrState) -> DisplayCapabilities {
        DisplayCapabilities {
            id: "d".into(),
            name: "TV".into(),
            is_primary: true,
            width: 3840,
            height: 2160,
            refresh_hz: None,
            bits_per_color: None,
            hdr,
            bounds: Rect::default(),
        }
    }

    #[test]
    fn sdr_metadata_but_pq_stream_is_corrected_to_tonemap() {
        // Exactly the Jellyfin 12.1 case observed with the HDR10 corpus file.
        let (plan, why) = video(&decision(VideoOutputPlan::Sdr), "pq", Some(&display(HdrState::SupportedButOff)), &Settings::default()).unwrap();
        assert!(matches!(plan, VideoOutputPlan::ToneMapToSdr { .. }));
        assert_eq!(why.code, "metadata-mismatch");
    }

    #[test]
    fn sdr_metadata_but_pq_stream_on_hdr_display_becomes_passthrough() {
        let active = display(HdrState::Active { max_luminance: Some(1000.0), min_luminance: None, max_full_frame_luminance: None });
        let (plan, _) = video(&decision(VideoOutputPlan::Sdr), "pq", Some(&active), &Settings::default()).unwrap();
        assert_eq!(plan, VideoOutputPlan::HdrPassthrough { format: "HDR10".into() });
    }

    #[test]
    fn consistent_metadata_needs_no_correction() {
        assert!(video(&decision(VideoOutputPlan::Sdr), "bt.1886", None, &Settings::default()).is_none());
        let tm = VideoOutputPlan::ToneMapToSdr { reason: "x".into() };
        assert!(video(&decision(tm), "pq", None, &Settings::default()).is_none());
    }

    #[test]
    fn hwdec_and_bitstream_fallbacks_are_reported() {
        let d = decision(VideoOutputPlan::Sdr);
        assert!(hwdec(&d, "no").is_some());
        assert!(hwdec(&d, "d3d11va").is_none());
        assert!(bitstream_failed(&d).unwrap().message.contains("truehd"));
    }
}
