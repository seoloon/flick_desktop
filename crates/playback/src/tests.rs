//! Scenario tests for the decision engine. Fixtures mirror real hardware
//! observed during validation (S/PDIF accepting AC3+DTS only, HDMI monitor
//! accepting nothing, HDR-capable display with HDR off).

use chrono::Utc;
use oneshot_core::capabilities::*;
use oneshot_core::playback::*;
use oneshot_core::settings::{ChannelOverride, HdrMode, Settings, SubtitleMode};
use oneshot_core::stream::*;

use super::*;

// ----------------------------------------------------------------- fixtures

fn display(hdr: HdrState) -> DisplayCapabilities {
    DisplayCapabilities {
        id: "\\\\.\\DISPLAY1".into(),
        name: "Test TV".into(),
        is_primary: true,
        width: 3840,
        height: 2160,
        refresh_hz: Some(60.0),
        bits_per_color: Some(10),
        hdr,
        bounds: Rect { x: 0, y: 0, width: 3840, height: 2160 },
    }
}

fn device(id: &str, channels: u8, formats: &[BitstreamFormat]) -> AudioDevice {
    AudioDevice {
        id: id.into(),
        mpv_name: Some(format!("wasapi/{id}")),
        name: id.into(),
        connection: AudioConnection::Hdmi,
        channels,
        channel_layout: None,
        sample_rate: Some(48_000),
        passthrough: PassthroughProbe::Probed { formats: formats.to_vec() },
    }
}

fn hw(codec: VideoCodec, depth: u8) -> HardwareDecoder {
    HardwareDecoder { codec, profile: "test".into(), max_bit_depth: depth, api: "d3d11va".into(), max_width: None, max_height: None }
}

fn caps(hdr: HdrState, devices: Vec<AudioDevice>) -> CapabilityReport {
    let default_device = devices.first().map(|d| d.id.clone());
    CapabilityReport {
        system: SystemCapabilities {
            os: OsKind::Windows,
            os_version: "test".into(),
            arch: "x86_64".into(),
            cpu_model: None,
            cpu_threads: 8,
            memory_bytes: None,
            gpus: vec![],
        },
        displays: vec![display(hdr)],
        audio: AudioCapabilities { devices, default_device },
        video: VideoCapabilities {
            hardware_decoders: vec![hw(VideoCodec::H264, 8), hw(VideoCodec::Hevc, 10), hw(VideoCodec::Av1, 10)],
            hardware_probe_ok: true,
            software_codecs: vec![VideoCodec::H264, VideoCodec::Hevc, VideoCodec::Av1, VideoCodec::Vp9],
        },
        probed_at: Utc::now(),
        notes: vec![],
    }
}

fn hdr_on() -> HdrState {
    HdrState::Active { max_luminance: Some(1000.0), min_luminance: Some(0.05), max_full_frame_luminance: Some(600.0) }
}

fn video(codec: VideoCodec, w: u32, h: u32, depth: u8, range: DynamicRange) -> VideoStream {
    VideoStream {
        index: 0,
        codec,
        profile: None,
        level: None,
        width: w,
        height: h,
        bit_depth: Some(depth),
        frame_rate: Some(23.976),
        bitrate: None,
        range,
        interlaced: false,
        title: None,
        is_default: true,
    }
}

fn audio(index: u32, codec: AudioCodec, channels: u8, lang: &str, spatial: Option<SpatialAudio>) -> AudioStream {
    AudioStream {
        index,
        codec,
        profile: None,
        channels,
        channel_layout: None,
        sample_rate: Some(48_000),
        bitrate: None,
        spatial,
        language: Some(lang.into()),
        title: None,
        is_default: index == 1,
        is_commentary: false,
    }
}

fn subtitle(index: u32, format: SubtitleFormat, lang: &str, forced: bool) -> SubtitleStream {
    SubtitleStream {
        index,
        format,
        language: Some(lang.into()),
        title: None,
        forced,
        hearing_impaired: false,
        is_default: false,
        external: false,
        delivery_path: None,
    }
}

fn offer(video: VideoStream, audio: Vec<AudioStream>, subtitles: Vec<SubtitleStream>) -> SourceOffer {
    SourceOffer {
        source: MediaSource {
            id: "src".into(),
            name: None,
            container: Some("mkv".into()),
            size_bytes: None,
            bitrate: Some(60_000_000),
            duration_ms: Some(7_200_000),
            video: vec![video],
            audio,
            subtitles,
        },
        policy: ServerPolicy::permissive(),
    }
}

fn run(offer: &SourceOffer, caps: &CapabilityReport, settings: &Settings) -> PlaybackDecision {
    decide(&DecisionInput {
        offer,
        caps,
        display_id: None,
        settings,
        audio: TrackRequest::Auto,
        subtitle: TrackRequest::Auto,
        original_language: None,
    })
    .expect("playable")
}

#[test]
fn original_audio_follows_the_language_the_title_was_made_in() {
    let c = caps(HdrState::Unsupported, vec![device("spk", 2, &[])]);
    let o = offer(
        video(VideoCodec::H264, 1920, 1080, 8, DynamicRange::Sdr),
        vec![audio(1, AudioCodec::Aac, 2, "fre", None), audio(2, AudioCodec::Aac, 2, "jpn", None)],
        vec![],
    );
    let s = Settings::default();
    let pick = |original| {
        decide(&DecisionInput { offer: &o, caps: &c, display_id: None, settings: &s, audio: TrackRequest::Auto, subtitle: TrackRequest::Auto, original_language: original })
            .expect("playable")
            .audio_stream
    };
    assert_eq!(pick(Some("ja")), Some(2));
    // Unknown, or not among the tracks: the file's own default.
    assert_eq!(pick(None), Some(1));
    assert_eq!(pick(Some("ko")), Some(1));
}

fn has(d: &PlaybackDecision, code: &str) -> bool {
    d.reasons.iter().any(|r| r.code == code)
}

fn passthrough_on() -> Settings {
    let mut s = Settings::default();
    s.audio.passthrough = true;
    s
}

// -------------------------------------------------------------------- tests

#[test]
fn h264_stereo_is_plain_direct_play_on_gpu() {
    let c = caps(HdrState::Unsupported, vec![device("spk", 2, &[])]);
    let o = offer(video(VideoCodec::H264, 1920, 1080, 8, DynamicRange::Sdr), vec![audio(1, AudioCodec::Aac, 2, "en", None)], vec![]);
    let d = run(&o, &c, &Settings::default());
    assert_eq!(d.label, StrategyLabel::DirectPlay);
    assert_eq!(d.delivery, DeliveryRequest::Direct);
    assert_eq!(d.hardware_decode, Some(true));
    assert_eq!(d.video, VideoOutputPlan::Sdr);
    assert!(matches!(d.audio, AudioOutputPlan::Pcm { output_channels: 2, downmix: false, .. }));
}

#[test]
fn hdr10_on_hdr_display_is_passthrough() {
    let c = caps(hdr_on(), vec![device("avr", 8, &[])]);
    let o = offer(video(VideoCodec::Hevc, 3840, 2160, 10, DynamicRange::Hdr10), vec![audio(1, AudioCodec::Eac3, 6, "en", None)], vec![]);
    let d = run(&o, &c, &Settings::default());
    assert_eq!(d.video, VideoOutputPlan::HdrPassthrough { format: "HDR10".into() });
    assert_eq!(d.label, StrategyLabel::DirectPlay);
}

#[test]
fn hdr10_with_os_hdr_off_is_tonemapped_and_says_why() {
    let c = caps(HdrState::SupportedButOff, vec![device("spk", 2, &[])]);
    let o = offer(video(VideoCodec::Hevc, 3840, 2160, 10, DynamicRange::Hdr10), vec![audio(1, AudioCodec::Aac, 2, "en", None)], vec![]);
    let d = run(&o, &c, &Settings::default());
    let VideoOutputPlan::ToneMapToSdr { reason } = &d.video else { panic!("expected tone mapping, got {:?}", d.video) };
    assert!(reason.contains("HDR is off"), "{reason}");
    assert_eq!(d.label, StrategyLabel::LocalDecode);
    // Tone mapping is local: the server still sends the original file.
    assert_eq!(d.delivery, DeliveryRequest::Direct);
}

#[test]
fn force_sdr_setting_wins_over_hdr_display() {
    let c = caps(hdr_on(), vec![device("spk", 2, &[])]);
    let mut s = Settings::default();
    s.video.hdr = HdrMode::ForceSdr;
    let o = offer(video(VideoCodec::Hevc, 3840, 2160, 10, DynamicRange::Hdr10), vec![], vec![]);
    assert!(matches!(run(&o, &c, &s).video, VideoOutputPlan::ToneMapToSdr { .. }));
}

#[test]
fn dolby_vision_is_never_claimed_as_dolby_vision_output() {
    let c = caps(hdr_on(), vec![device("spk", 2, &[])]);
    let dv = DynamicRange::DolbyVision { profile: Some(5), compat: DolbyVisionCompat::None, enhancement_layer: false };
    let o = offer(video(VideoCodec::Hevc, 3840, 2160, 10, dv), vec![], vec![]);
    let d = run(&o, &c, &Settings::default());
    assert_eq!(d.video, VideoOutputPlan::DolbyVisionReshape { output: "HDR10 (PQ)".into() });
    assert!(d.reasons.iter().any(|r| r.message.contains("does not receive a Dolby Vision signal")));
    assert_eq!(d.label, StrategyLabel::LocalDecode);
}

#[test]
fn dolby_vision_fel_is_reported_as_ignored() {
    let c = caps(HdrState::Unsupported, vec![device("spk", 2, &[])]);
    let dv = DynamicRange::DolbyVision { profile: Some(7), compat: DolbyVisionCompat::Hdr10, enhancement_layer: true };
    let d = run(&offer(video(VideoCodec::Hevc, 3840, 2160, 10, dv), vec![], vec![]), &c, &Settings::default());
    assert!(has(&d, codes::DV_FEL_IGNORED));
    assert_eq!(d.video, VideoOutputPlan::DolbyVisionReshape { output: "SDR".into() });
}

#[test]
fn truehd_atmos_bitstreams_when_the_receiver_accepts_it() {
    let c = caps(hdr_on(), vec![device("avr", 8, &BitstreamFormat::ALL)]);
    let o = offer(
        video(VideoCodec::Hevc, 3840, 2160, 10, DynamicRange::Hdr10),
        vec![audio(1, AudioCodec::TrueHd, 8, "en", Some(SpatialAudio::DolbyAtmos))],
        vec![],
    );
    let d = run(&o, &c, &passthrough_on());
    assert_eq!(d.audio, AudioOutputPlan::Bitstream { format: BitstreamFormat::TrueHd, device: "avr".into(), reencoded: false });
    assert_eq!(d.label, StrategyLabel::DirectPlay);
}

#[test]
fn truehd_atmos_on_spdif_decodes_and_declares_atmos_lost() {
    // Mirrors the validated Realtek S/PDIF output: AC3 + DTS only.
    let c = caps(HdrState::Unsupported, vec![device("spdif", 2, &[BitstreamFormat::Ac3, BitstreamFormat::Dts])]);
    let o = offer(
        video(VideoCodec::H264, 1920, 1080, 8, DynamicRange::Sdr),
        vec![audio(1, AudioCodec::TrueHd, 8, "en", Some(SpatialAudio::DolbyAtmos))],
        vec![],
    );
    let d = run(&o, &c, &passthrough_on());
    assert_eq!(
        d.audio,
        AudioOutputPlan::Pcm { source_channels: 8, output_channels: 2, downmix: true, spatial_lost: true }
    );
    assert!(has(&d, codes::BITSTREAM_UNAVAILABLE));
    assert!(has(&d, codes::SPATIAL_LOST));
    assert_eq!(d.label, StrategyLabel::LocalDecode);
}

#[test]
fn eac3_is_not_forced_onto_spdif() {
    // The regression this engine exists for: mpv fails (no PCM fallback) when
    // a refused format is forced. E-AC3 must be decoded, not bitstreamed.
    let c = caps(HdrState::Unsupported, vec![device("spdif", 2, &[BitstreamFormat::Ac3, BitstreamFormat::Dts])]);
    let o = offer(video(VideoCodec::H264, 1920, 1080, 8, DynamicRange::Sdr), vec![audio(1, AudioCodec::Eac3, 6, "en", None)], vec![]);
    let d = run(&o, &c, &passthrough_on());
    assert!(matches!(d.audio, AudioOutputPlan::Pcm { .. }), "{:?}", d.audio);
}

#[test]
fn dts_hd_falls_back_to_dts_core_when_only_dts_is_accepted() {
    let c = caps(HdrState::Unsupported, vec![device("spdif", 2, &[BitstreamFormat::Ac3, BitstreamFormat::Dts])]);
    let o = offer(video(VideoCodec::H264, 1920, 1080, 8, DynamicRange::Sdr), vec![audio(1, AudioCodec::DtsHd, 8, "en", None)], vec![]);
    let d = run(&o, &c, &passthrough_on());
    assert_eq!(d.audio, AudioOutputPlan::Bitstream { format: BitstreamFormat::Dts, device: "spdif".into(), reencoded: false });
    assert!(has(&d, codes::BITSTREAM_DTS_CORE));
}

#[test]
fn ac3_reencode_carries_surround_over_spdif_and_says_it_is_lossy() {
    let c = caps(HdrState::Unsupported, vec![device("spdif", 2, &[BitstreamFormat::Ac3, BitstreamFormat::Dts])]);
    let mut s = passthrough_on();
    s.audio.ac3_reencode = true;
    let o = offer(video(VideoCodec::H264, 1920, 1080, 8, DynamicRange::Sdr), vec![audio(1, AudioCodec::Flac, 6, "en", None)], vec![]);
    let d = run(&o, &c, &s);
    assert_eq!(d.audio, AudioOutputPlan::Bitstream { format: BitstreamFormat::Ac3, device: "spdif".into(), reencoded: true });
    assert!(has(&d, codes::AC3_REENCODE));
}

#[test]
fn passthrough_respects_user_format_selection() {
    let c = caps(HdrState::Unsupported, vec![device("avr", 8, &BitstreamFormat::ALL)]);
    let mut s = passthrough_on();
    s.audio.passthrough_formats = vec![BitstreamFormat::Ac3];
    let o = offer(video(VideoCodec::H264, 1920, 1080, 8, DynamicRange::Sdr), vec![audio(1, AudioCodec::Dts, 6, "en", None)], vec![]);
    let d = run(&o, &c, &s);
    assert!(matches!(d.audio, AudioOutputPlan::Pcm { output_channels: 8, downmix: false, .. }));
}

#[test]
fn surround_is_kept_on_surround_devices_and_downmixed_on_stereo() {
    let o = offer(video(VideoCodec::H264, 1920, 1080, 8, DynamicRange::Sdr), vec![audio(1, AudioCodec::Ac3, 6, "en", None)], vec![]);
    let d51 = run(&o, &caps(HdrState::Unsupported, vec![device("hts", 6, &[])]), &Settings::default());
    assert_eq!(d51.audio, AudioOutputPlan::Pcm { source_channels: 6, output_channels: 6, downmix: false, spatial_lost: false });
    assert_eq!(d51.label, StrategyLabel::DirectPlay);

    let d71 = run(&o, &caps(HdrState::Unsupported, vec![device("avr", 8, &[])]), &Settings::default());
    assert_eq!(d71.audio, AudioOutputPlan::Pcm { source_channels: 6, output_channels: 8, downmix: false, spatial_lost: false });

    let d20 = run(&o, &caps(HdrState::Unsupported, vec![device("hp", 2, &[])]), &Settings::default());
    assert!(matches!(d20.audio, AudioOutputPlan::Pcm { output_channels: 2, downmix: true, .. }));
    assert_eq!(d20.label, StrategyLabel::LocalDecode);
}

#[test]
fn channel_override_beats_os_layout() {
    let mut s = Settings::default();
    s.audio.channels = ChannelOverride::Stereo;
    let o = offer(video(VideoCodec::H264, 1920, 1080, 8, DynamicRange::Sdr), vec![audio(1, AudioCodec::Ac3, 6, "en", None)], vec![]);
    let d = run(&o, &caps(HdrState::Unsupported, vec![device("avr", 8, &[])]), &s);
    assert!(matches!(d.audio, AudioOutputPlan::Pcm { output_channels: 2, downmix: true, .. }));
}

#[test]
fn missing_gpu_decoder_is_cpu_decode_not_transcode_by_default() {
    let c = caps(HdrState::Unsupported, vec![device("spk", 2, &[])]);
    let o = offer(video(VideoCodec::Vp9, 3840, 2160, 10, DynamicRange::Sdr), vec![], vec![]);
    let d = run(&o, &c, &Settings::default());
    assert_eq!(d.delivery, DeliveryRequest::Direct);
    assert_eq!(d.hardware_decode, Some(false));
    assert!(has(&d, codes::SWDEC));
}

#[test]
fn transcode_policy_for_heavy_cpu_decode_is_opt_in() {
    let c = caps(HdrState::Unsupported, vec![device("spk", 2, &[])]);
    let mut s = Settings::default();
    s.playback.transcode_without_hwdec_min_height = Some(2160);
    let o = offer(video(VideoCodec::Vp9, 3840, 2160, 10, DynamicRange::Sdr), vec![], vec![]);
    let d = run(&o, &c, &s);
    assert_eq!(d.label, StrategyLabel::ServerTranscode);
    assert!(matches!(d.delivery, DeliveryRequest::Transcode { video: Some(VideoCodec::Hevc), .. }));
}

#[test]
fn unknown_codec_requires_transcode() {
    let c = caps(HdrState::Unsupported, vec![device("spk", 2, &[])]);
    let o = offer(video(VideoCodec::Other("prores".into()), 1920, 1080, 10, DynamicRange::Sdr), vec![], vec![]);
    let d = run(&o, &c, &Settings::default());
    assert_eq!(d.label, StrategyLabel::ServerTranscode);
}

#[test]
fn unplayable_when_transcode_disabled() {
    let c = caps(HdrState::Unsupported, vec![device("spk", 2, &[])]);
    let mut s = Settings::default();
    s.playback.allow_transcode = false;
    let o = offer(video(VideoCodec::Other("prores".into()), 1920, 1080, 10, DynamicRange::Sdr), vec![], vec![]);
    let err = decide(&DecisionInput { offer: &o, caps: &c, display_id: None, settings: &s, audio: TrackRequest::Auto, subtitle: TrackRequest::Auto, original_language: None })
        .unwrap_err();
    assert!(err.reasons.iter().any(|r| r.severity == ReasonSeverity::Blocking));
}

#[test]
fn server_refusing_direct_play_gets_remux_then_transcode() {
    let c = caps(HdrState::Unsupported, vec![device("spk", 2, &[])]);
    let mut o = offer(video(VideoCodec::H264, 1920, 1080, 8, DynamicRange::Sdr), vec![audio(1, AudioCodec::Aac, 2, "en", None)], vec![]);
    o.policy = ServerPolicy {
        direct_play_allowed: false,
        direct_stream_allowed: true,
        transcode_allowed: true,
        server_reasons: vec!["ContainerNotSupported".into()],
    };
    let d = run(&o, &c, &Settings::default());
    assert_eq!(d.label, StrategyLabel::DirectStream);
    assert!(has(&d, codes::SERVER_REASON));

    o.policy.direct_stream_allowed = false;
    let d = run(&o, &c, &Settings::default());
    assert_eq!(d.label, StrategyLabel::ServerTranscode);
    assert!(has(&d, codes::SERVER_FORCED_TRANSCODE));
}

#[test]
fn bitrate_limit_triggers_capped_transcode() {
    let c = caps(HdrState::Unsupported, vec![device("spk", 2, &[])]);
    let mut s = Settings::default();
    s.playback.max_bitrate = Some(20_000_000);
    let o = offer(video(VideoCodec::Hevc, 3840, 2160, 10, DynamicRange::Sdr), vec![], vec![]);
    let d = run(&o, &c, &s);
    assert!(matches!(d.delivery, DeliveryRequest::Transcode { max_bitrate: Some(20_000_000), .. }));
}

#[test]
fn pgs_is_rendered_locally_in_direct_play_but_burned_when_transcoding() {
    let c = caps(HdrState::Unsupported, vec![device("spk", 2, &[])]);
    let subs = vec![subtitle(3, SubtitleFormat::Pgs, "fr", false)];
    let o = offer(video(VideoCodec::H264, 1920, 1080, 8, DynamicRange::Sdr), vec![audio(1, AudioCodec::Aac, 2, "en", None)], subs.clone());
    let pick = |o: &SourceOffer, s: &Settings| {
        decide(&DecisionInput { offer: o, caps: &c, display_id: None, settings: s, audio: TrackRequest::Auto, subtitle: TrackRequest::Index(3), original_language: None })
            .unwrap()
    };
    assert_eq!(pick(&o, &Settings::default()).subtitles, SubtitlePlan::Local { index: 3, external: false });

    let mut s = Settings::default();
    s.playback.max_bitrate = Some(1_000_000);
    let d = pick(&o, &s);
    assert_eq!(d.subtitles, SubtitlePlan::BurnIn { index: 3 });
    assert!(matches!(d.delivery, DeliveryRequest::Transcode { burn_subtitle: Some(3), .. }));
}

#[test]
fn audio_language_preference_picks_richest_track() {
    let c = caps(HdrState::Unsupported, vec![device("avr", 8, &[])]);
    let mut s = Settings::default();
    s.playback.preferred_audio_languages = vec!["fr".into()];
    let o = offer(
        video(VideoCodec::H264, 1920, 1080, 8, DynamicRange::Sdr),
        vec![
            audio(1, AudioCodec::TrueHd, 8, "eng", None),
            audio(2, AudioCodec::Aac, 2, "fre", None),
            audio(3, AudioCodec::Ac3, 6, "fra", None),
        ],
        vec![],
    );
    assert_eq!(run(&o, &c, &s).audio_stream, Some(3));
}

#[test]
fn smart_subtitles_show_forced_only_when_audio_is_understood() {
    let c = caps(HdrState::Unsupported, vec![device("spk", 2, &[])]);
    let mut s = Settings::default();
    s.subtitles.mode = SubtitleMode::Smart;
    s.subtitles.languages = vec!["fr".into()];
    let subs = vec![subtitle(4, SubtitleFormat::Srt, "fre", false), subtitle(5, SubtitleFormat::Ass, "fre", true)];

    // French audio: only the forced French track.
    s.playback.preferred_audio_languages = vec!["fr".into()];
    let fr = offer(video(VideoCodec::H264, 1920, 1080, 8, DynamicRange::Sdr), vec![audio(1, AudioCodec::Aac, 2, "fre", None)], subs.clone());
    assert_eq!(run(&fr, &c, &s).subtitles, SubtitlePlan::Local { index: 5, external: false });

    // English audio: full French subtitles.
    let en = offer(video(VideoCodec::H264, 1920, 1080, 8, DynamicRange::Sdr), vec![audio(1, AudioCodec::Aac, 2, "eng", None)], subs);
    assert_eq!(run(&en, &c, &s).subtitles, SubtitlePlan::Local { index: 4, external: false });
}

#[test]
fn full_subtitles_skip_tracks_titled_forced_even_without_the_flag() {
    let c = caps(HdrState::Unsupported, vec![device("spk", 2, &[])]);
    let sdr = || video(VideoCodec::H264, 1920, 1080, 8, DynamicRange::Sdr);
    let mut s = Settings::default();
    s.subtitles.mode = SubtitleMode::Always;
    s.subtitles.languages = vec!["fr".into()];
    let titled = |index, title: &str| SubtitleStream { title: Some(title.into()), ..subtitle(index, SubtitleFormat::Srt, "fre", false) };
    // Forced track listed first and only named so; SDH before the plain one.
    let subs = vec![
        titled(3, "Français (forcés)"),
        SubtitleStream { hearing_impaired: true, ..titled(4, "Français SDH") },
        titled(5, "Français"),
    ];
    let o = offer(sdr(), vec![audio(1, AudioCodec::Aac, 2, "eng", None)], subs);
    assert_eq!(run(&o, &c, &s).subtitles, SubtitlePlan::Local { index: 5, external: false });

    s.subtitles.mode = SubtitleMode::ForcedOnly;
    let o = offer(sdr(), vec![audio(1, AudioCodec::Aac, 2, "fre", None)], vec![titled(5, "Français"), titled(3, "Signs & Songs")]);
    assert_eq!(run(&o, &c, &s).subtitles, SubtitlePlan::Local { index: 3, external: false });
}

#[test]
fn unknown_platform_capabilities_stay_conservative() {
    let mut c = caps(HdrState::Unknown { reason: "not implemented".into() }, vec![]);
    c.video.hardware_probe_ok = false;
    c.audio = AudioCapabilities::default();
    let o = offer(
        video(VideoCodec::Hevc, 3840, 2160, 10, DynamicRange::Hdr10),
        vec![audio(1, AudioCodec::TrueHd, 8, "en", Some(SpatialAudio::DolbyAtmos))],
        vec![],
    );
    let d = run(&o, &c, &passthrough_on());
    assert!(matches!(d.video, VideoOutputPlan::ToneMapToSdr { .. }), "never promise HDR when unknown");
    assert!(matches!(d.audio, AudioOutputPlan::Pcm { spatial_lost: true, .. }), "never promise bitstream when unknown");
    assert_eq!(d.hardware_decode, None);
}

#[test]
fn client_profile_never_limits_to_browser_codecs() {
    let c = caps(HdrState::Unsupported, vec![device("spk", 2, &[])]);
    let p = client_profile(&c, &Settings::default());
    for codec in [VideoCodec::Hevc, VideoCodec::Av1, VideoCodec::Vp9] {
        assert!(p.video_codecs.contains(&codec));
    }
    for codec in [AudioCodec::TrueHd, AudioCodec::DtsHd, AudioCodec::Eac3, AudioCodec::Flac] {
        assert!(p.audio_codecs.contains(&codec));
    }
    assert!(p.containers.iter().any(|c| c == "mkv"));
    assert_eq!(p.max_audio_channels, 8);
}
