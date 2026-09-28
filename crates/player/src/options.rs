//! Translation of a [`PlaybackDecision`] + settings into mpv properties.
//!
//! Pure and deterministic: the session applies the returned list with
//! `set_property` right before `loadfile`, and the Debug panel shows it.

use oneshot_core::capabilities::{AudioDevice, DisplayCapabilities, HdrState};
use oneshot_core::playback::{AudioOutputPlan, PlaybackDecision, VideoOutputPlan};
use oneshot_core::settings::{Deinterlace, FrameSync, HardwareDecoding, Normalization, Settings};
use oneshot_mpv::Node;

/// Ordered `(property, value)` pairs.
pub type PropertyList = Vec<(&'static str, Node)>;

fn s(v: impl Into<String>) -> Node {
    Node::String(v.into())
}

/// Properties that depend only on settings (applied at engine start and
/// whenever settings change).
pub fn base_properties(settings: &Settings) -> PropertyList {
    let v = &settings.video;
    let sub = &settings.subtitles;
    let mut p: PropertyList = vec![
        ("hwdec", s(if v.hardware_decoding == HardwareDecoding::Auto { "auto-safe" } else { "no" })),
        ("tone-mapping", s(v.tone_mapping.mpv_name())),
        ("hdr-compute-peak", s(if v.hdr_peak_detection { "auto" } else { "no" })),
        (
            "deinterlace",
            s(match v.deinterlace {
                Deinterlace::Auto => "auto",
                Deinterlace::On => "yes",
                Deinterlace::Off => "no",
            }),
        ),
        ("video-sync", s(if v.frame_sync == FrameSync::DisplayResample { "display-resample" } else { "audio" })),
        ("interpolation", Node::Flag(v.interpolation && v.frame_sync == FrameSync::DisplayResample)),
        ("demuxer-max-bytes", s(format!("{}MiB", settings.network.buffer_mib))),
        ("demuxer-max-back-bytes", s(format!("{}MiB", (settings.network.buffer_mib / 3).max(16)))),
        ("volume", Node::Double(f64::from(settings.audio.volume))),
        // Streaming-service look for plain-text subtitles: a bold sans, a
        // thin crisp black edge, and a faint drop shadow
        // instead of a heavy outline. Sizes are relative to a 720p frame.
        ("sub-font", s(subtitle_font(&sub.font_family))),
        ("sub-bold", Node::Flag(sub.bold)),
        ("sub-font-size", Node::Double(40.0)),
        ("sub-scale", Node::Double(f64::from(sub.scale))),
        ("sub-color", s(sub.color.clone())),
        ("sub-border-color", s("#000000")),
        ("sub-border-size", Node::Double(f64::from(sub.outline))),
        ("sub-blur", Node::Double(0.0)),
        ("sub-shadow-offset", Node::Double(1.2)),
        ("sub-shadow-color", s("#8C000000")),
        ("sub-pos", Node::Int64(i64::from(sub.position.min(150)))),
        ("sub-ass-override", s(if sub.override_ass { "force" } else { "scale" })),
    ];
    if sub.background_opacity > 0.0 {
        let alpha = (sub.background_opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        let rgb = sub.background.trim_start_matches('#');
        p.push(("sub-back-color", s(format!("#{alpha:02X}{rgb}"))));
        p.push(("sub-border-style", s("background-box")));
    } else {
        p.push(("sub-border-style", s("outline-and-shadow")));
    }
    p.push(("af", s(audio_filters(settings, false))));
    p
}

/// The platform's closest match to streaming services' subtitle fonts, for
/// an empty setting. libass resolves families through DirectWrite (Windows),
/// CoreText (macOS) or fontconfig (Linux, which understands `sans-serif`).
fn subtitle_font(family: &str) -> String {
    if !family.trim().is_empty() {
        return family.trim().to_owned();
    }
    if cfg!(windows) {
        "Arial".into()
    } else if cfg!(target_os = "macos") {
        "Helvetica Neue".into()
    } else {
        "sans-serif".into()
    }
}

/// Audio filter chain: optional normalisation, optional boost, then optional
/// AC3 encoder for S/PDIF receivers (must be last: it outputs the compressed
/// stream).
fn audio_filters(settings: &Settings, ac3_encode: bool) -> String {
    let mut chain: Vec<String> = Vec::new();
    match settings.audio.normalization {
        Normalization::Off => {}
        Normalization::NightMode => chain.push("lavfi=[acompressor=threshold=0.08:ratio=4:attack=20:release=250]".into()),
        Normalization::Loudness => chain.push("lavfi=[loudnorm=I=-16:TP=-1.5:LRA=11]".into()),
    }
    if let Some(boost) = volume_boost(settings) {
        chain.push(boost);
    }
    if ac3_encode {
        chain.push("lavcac3enc".into());
    }
    chain.join(",")
}

/// Gain followed by a limiter, so loud scenes are held just under full scale
/// instead of clipping. mpv's own volume is applied after the filters, so the
/// limiter has to sit in the same graph as the gain.
fn volume_boost(settings: &Settings) -> Option<String> {
    let a = &settings.audio;
    if !a.volume_boost || a.volume_boost_percent <= 100 {
        return None;
    }
    let gain_db = 20.0 * (f64::from(a.volume_boost_percent.min(300)) / 100.0).log10();
    Some(format!("lavfi=[volume={gain_db:.2}dB,alimiter=limit=0.97:level=0]"))
}

/// Per-file properties derived from the decision.
pub fn decision_properties(
    decision: &PlaybackDecision,
    settings: &Settings,
    display: Option<&DisplayCapabilities>,
    device: Option<&AudioDevice>,
) -> PropertyList {
    let mut p: PropertyList = Vec::new();

    // ---- audio device + passthrough --------------------------------------
    if let Some(name) = device.and_then(|d| d.mpv_name.clone()) {
        p.push(("audio-device", s(name)));
    } else {
        p.push(("audio-device", s("auto")));
    }
    match &decision.audio {
        AudioOutputPlan::Bitstream { format, reencoded, .. } => {
            // Only the chosen format: never let mpv try a format the probe
            // did not validate (mpv does not fall back to PCM on refusal).
            p.push(("audio-spdif", s(format.mpv_name())));
            p.push(("audio-exclusive", Node::Flag(true)));
            // An untouched bitstream cannot be filtered (no boost, no levelling).
            p.push(("af", s(if *reencoded { audio_filters(settings, true) } else { String::new() })));
        }
        AudioOutputPlan::Pcm { output_channels, .. } => {
            p.push(("audio-spdif", s("")));
            p.push(("audio-exclusive", Node::Flag(settings.audio.exclusive)));
            p.push(("audio-channels", s(channel_layout(*output_channels))));
            p.push(("af", s(audio_filters(settings, false))));
        }
        AudioOutputPlan::ServerDetermined | AudioOutputPlan::None => {
            p.push(("audio-spdif", s("")));
            p.push(("audio-channels", s("auto-safe")));
        }
    }

    // ---- colour pipeline --------------------------------------------------
    p.extend(video_target(&decision.video, display));
    p
}

/// Swapchain colourspace. In composition mode mpv cannot query the output,
/// so HDR passthrough is signalled explicitly from our display probe.
pub fn video_target(plan: &VideoOutputPlan, display: Option<&DisplayCapabilities>) -> PropertyList {
    let hdr_display = display.and_then(|d| match &d.hdr {
        HdrState::Active { max_luminance, .. } => Some(max_luminance.unwrap_or(1000.0)),
        _ => None,
    });
    let passthrough = matches!(plan, VideoOutputPlan::HdrPassthrough { .. })
        || matches!(plan, VideoOutputPlan::DolbyVisionReshape { output } if output.contains("HDR"));
    match (passthrough, hdr_display) {
        (true, Some(peak)) => vec![
            ("target-colorspace-hint", s("yes")),
            ("target-prim", s("bt.2020")),
            ("target-trc", s("pq")),
            ("target-peak", Node::Double(f64::from(peak))),
        ],
        // SDR output (tone-mapped if needed). "no" keeps an SDR swapchain even
        // on a display in HDR mode, where Windows composes it at SDR white.
        _ => vec![
            ("target-colorspace-hint", s("no")),
            ("target-prim", s("auto")),
            ("target-trc", s("auto")),
            ("target-peak", s("auto")),
        ],
    }
}

fn channel_layout(ch: u8) -> &'static str {
    match ch {
        1 => "mono",
        2 => "stereo",
        3 => "2.1",
        4 => "quad",
        5 => "5.0",
        6 => "5.1",
        7 => "6.1",
        8 => "7.1",
        _ => "auto-safe",
    }
}

#[cfg(test)]
mod tests {
    use oneshot_core::capabilities::{AudioConnection, PassthroughProbe, Rect};
    use oneshot_core::playback::{DeliveryRequest, StrategyLabel, SubtitlePlan};
    use oneshot_core::stream::BitstreamFormat;

    use super::*;

    fn decision(video: VideoOutputPlan, audio: AudioOutputPlan) -> PlaybackDecision {
        PlaybackDecision {
            label: StrategyLabel::DirectPlay,
            source_id: "s".into(),
            delivery: DeliveryRequest::Direct,
            video_stream: Some(0),
            audio_stream: Some(1),
            video,
            audio,
            subtitles: SubtitlePlan::None,
            hardware_decode: Some(true),
            reasons: vec![],
        }
    }

    fn display(hdr: HdrState) -> DisplayCapabilities {
        DisplayCapabilities {
            id: "d".into(),
            name: "d".into(),
            is_primary: true,
            width: 3840,
            height: 2160,
            refresh_hz: None,
            bits_per_color: None,
            hdr,
            bounds: Rect::default(),
        }
    }

    fn device() -> AudioDevice {
        AudioDevice {
            id: "x".into(),
            mpv_name: Some("wasapi/{x}".into()),
            name: "AVR".into(),
            connection: AudioConnection::Hdmi,
            channels: 8,
            channel_layout: None,
            sample_rate: None,
            passthrough: PassthroughProbe::Probed { formats: vec![BitstreamFormat::TrueHd] },
        }
    }

    fn get<'a>(p: &'a PropertyList, k: &str) -> Option<&'a Node> {
        p.iter().rev().find(|(n, _)| *n == k).map(|(_, v)| v)
    }

    #[test]
    fn bitstream_sets_exactly_one_spdif_format_and_device() {
        let d = decision(VideoOutputPlan::Sdr, AudioOutputPlan::Bitstream { format: BitstreamFormat::TrueHd, device: "AVR".into(), reencoded: false });
        let p = decision_properties(&d, &Settings::default(), None, Some(&device()));
        assert_eq!(get(&p, "audio-spdif"), Some(&Node::from("truehd")));
        assert_eq!(get(&p, "audio-device"), Some(&Node::from("wasapi/{x}")));
        assert_eq!(get(&p, "audio-exclusive"), Some(&Node::Flag(true)));
    }

    #[test]
    fn pcm_clears_spdif_and_pins_layout() {
        let d = decision(
            VideoOutputPlan::Sdr,
            AudioOutputPlan::Pcm { source_channels: 8, output_channels: 6, downmix: true, spatial_lost: false },
        );
        let p = decision_properties(&d, &Settings::default(), None, Some(&device()));
        assert_eq!(get(&p, "audio-spdif"), Some(&Node::from("")));
        assert_eq!(get(&p, "audio-channels"), Some(&Node::from("5.1")));
    }

    #[test]
    fn hdr_passthrough_signals_pq_with_display_peak() {
        let active = display(HdrState::Active { max_luminance: Some(800.0), min_luminance: None, max_full_frame_luminance: None });
        let p = video_target(&VideoOutputPlan::HdrPassthrough { format: "HDR10".into() }, Some(&active));
        assert_eq!(get(&p, "target-trc"), Some(&Node::from("pq")));
        assert_eq!(get(&p, "target-peak"), Some(&Node::Double(800.0)));
    }

    #[test]
    fn tonemap_never_signals_hdr_even_on_hdr_display() {
        let active = display(HdrState::Active { max_luminance: None, min_luminance: None, max_full_frame_luminance: None });
        let p = video_target(&VideoOutputPlan::ToneMapToSdr { reason: "x".into() }, Some(&active));
        assert_eq!(get(&p, "target-colorspace-hint"), Some(&Node::from("no")));
    }

    #[test]
    fn subtitle_background_uses_alpha_prefixed_colour() {
        let mut s = Settings::default();
        s.subtitles.background_opacity = 0.5;
        s.subtitles.background = "#101010".into();
        let p = base_properties(&s);
        assert_eq!(get(&p, "sub-back-color"), Some(&Node::from("#80101010")));
    }

    #[test]
    fn empty_subtitle_font_uses_platform_default() {
        let p = base_properties(&Settings::default());
        let Some(Node::String(font)) = get(&p, "sub-font") else { panic!("sub-font missing") };
        assert!(!font.is_empty());
        assert_eq!(get(&p, "sub-bold"), Some(&Node::Flag(true)));
    }

    #[test]
    fn volume_boost_adds_gain_and_limiter_only_when_enabled() {
        let mut s = Settings::default();
        assert_eq!(get(&base_properties(&s), "af"), Some(&Node::from("")));
        s.audio.volume_boost = true;
        s.audio.volume_boost_percent = 200;
        assert_eq!(get(&base_properties(&s), "af"), Some(&Node::from("lavfi=[volume=6.02dB,alimiter=limit=0.97:level=0]")));
    }

    #[test]
    fn untouched_bitstream_is_never_filtered() {
        let mut s = Settings::default();
        s.audio.volume_boost = true;
        let d = decision(VideoOutputPlan::Sdr, AudioOutputPlan::Bitstream { format: BitstreamFormat::TrueHd, device: "AVR".into(), reencoded: false });
        let p = decision_properties(&d, &s, None, Some(&device()));
        assert_eq!(get(&p, "af"), Some(&Node::from("")));
    }
}
