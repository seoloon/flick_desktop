//! User settings. One struct, persisted as JSON by `oneshot-storage`, edited
//! by the Settings UI, consumed by the decision engine and the player.
//!
//! Every field has a default so older settings files keep loading
//! (`#[serde(default)]` at every level).

use serde::{Deserialize, Serialize};

use crate::stream::BitstreamFormat;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub general: GeneralSettings,
    pub appearance: AppearanceSettings,
    pub playback: PlaybackSettings,
    pub audio: AudioSettings,
    pub video: VideoSettings,
    pub subtitles: SubtitleSettings,
    pub network: NetworkSettings,
    pub cache: CacheSettings,
    pub controller: ControllerSettings,
    pub notifications: NotificationSettings,
    pub privacy: PrivacySettings,
    pub advanced: AdvancedSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct GeneralSettings {
    /// BCP-47 UI language; `None` follows the OS.
    pub language: Option<String>,
    pub start_in_maxi_frame: bool,
    /// Look for a new release at launch and offer to install it.
    pub check_for_updates: bool,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self { language: None, start_in_maxi_frame: false, check_for_updates: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum Density {
    Compact,
    Comfortable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct AppearanceSettings {
    /// Accent colour as `#rrggbb`.
    pub accent: String,
    /// 0 = no motion (also honoured from the OS "reduce motion"), 1 = full.
    pub animation_intensity: f32,
    /// Strength of the adaptive artwork background, 0..=1.
    pub background_intensity: f32,
    pub blur: bool,
    pub density: Density,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            accent: "#e8b04b".into(),
            animation_intensity: 1.0,
            background_intensity: 0.6,
            blur: true,
            density: Density::Comfortable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ResumeBehavior {
    Ask,
    Resume,
    StartOver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum SkipMode {
    Off,
    /// Show a "Skip" button while the marker is active.
    Button,
    /// Skip automatically (with a short toast).
    Auto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct PlaybackSettings {
    /// Upper bound for streaming bitrate (bits/s). `None` = original quality.
    pub max_bitrate: Option<u64>,
    /// Allow the server to remux when it refuses direct play.
    pub allow_direct_stream: bool,
    /// Allow server transcoding at all. When false, unplayable = error.
    pub allow_transcode: bool,
    /// Prefer a server transcode over CPU decoding when the GPU cannot decode
    /// a source at or above this height (e.g. 2160 on a weak laptop).
    pub transcode_without_hwdec_min_height: Option<u32>,
    pub resume: ResumeBehavior,
    pub autoplay_next: bool,
    pub autoplay_countdown_secs: u32,
    pub skip_intro: SkipMode,
    pub skip_credits: SkipMode,
    /// Progress report interval to the server.
    pub report_interval_secs: u32,
    pub preferred_audio_languages: Vec<String>,
}

/// Widest picture worth sending at a bitrate, so a capped transcode drops
/// resolution instead of starving a 4K picture. Thresholds follow common
/// real-time encoder ladders (H.264/HEVC): ≈ 25 Mb/s for 4K, 6 for 1080p,
/// 3 for 720p, 1.5 for 480p. `None` = no reason to downscale.
pub fn max_width_for_bitrate(bits_per_sec: u64) -> Option<u32> {
    match bits_per_sec {
        25_000_000.. => None,
        6_000_000.. => Some(1920),
        3_000_000.. => Some(1280),
        1_500_000.. => Some(854),
        _ => Some(640),
    }
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self {
            max_bitrate: None,
            allow_direct_stream: true,
            allow_transcode: true,
            transcode_without_hwdec_min_height: None,
            resume: ResumeBehavior::Ask,
            autoplay_next: true,
            autoplay_countdown_secs: 5,
            skip_intro: SkipMode::Button,
            skip_credits: SkipMode::Button,
            report_interval_secs: 10,
            preferred_audio_languages: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ChannelOverride {
    /// Follow the OS speaker configuration.
    Auto,
    Stereo,
    Surround51,
    Surround71,
}

impl ChannelOverride {
    pub fn channels(self) -> Option<u8> {
        match self {
            Self::Auto => None,
            Self::Stereo => Some(2),
            Self::Surround51 => Some(6),
            Self::Surround71 => Some(8),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum Normalization {
    Off,
    /// Dynamic range compression for night listening.
    NightMode,
    /// EBU R128 loudness normalisation.
    Loudness,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct AudioSettings {
    /// OS endpoint id; `None` = OS default device.
    pub device: Option<String>,
    pub channels: ChannelOverride,
    /// Master switch for bitstreaming.
    pub passthrough: bool,
    /// Formats the user allows to bitstream (intersected with the probe).
    pub passthrough_formats: Vec<BitstreamFormat>,
    /// WASAPI exclusive mode for PCM too (bit-perfect, blocks other apps).
    pub exclusive: bool,
    pub normalization: Normalization,
    /// Encode multichannel PCM to AC3 for S/PDIF receivers (lossy).
    pub ac3_reencode: bool,
    pub volume: u8,
    /// Amplify beyond 100 % (quiet mixes, laptop speakers). Decoded audio only:
    /// bitstreamed tracks reach the receiver untouched.
    pub volume_boost: bool,
    /// Gain while boosting, in percent of the original level (110..=300).
    pub volume_boost_percent: u16,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            device: None,
            channels: ChannelOverride::Auto,
            passthrough: false,
            passthrough_formats: BitstreamFormat::ALL.to_vec(),
            exclusive: false,
            normalization: Normalization::Off,
            ac3_reencode: false,
            volume: 100,
            volume_boost: false,
            volume_boost_percent: 150,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum HardwareDecoding {
    /// mpv `auto-safe`: whitelisted hardware decoders, CPU fallback.
    Auto,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum HdrMode {
    /// HDR passthrough when the display is in HDR mode, tone-map otherwise.
    Auto,
    /// Always tone-map to SDR.
    ForceSdr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ToneMapping {
    Auto,
    Bt2390,
    Spline,
    Hable,
    Mobius,
    Clip,
}

impl ToneMapping {
    pub fn mpv_name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Bt2390 => "bt.2390",
            Self::Spline => "spline",
            Self::Hable => "hable",
            Self::Mobius => "mobius",
            Self::Clip => "clip",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum Deinterlace {
    Auto,
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum FrameSync {
    /// Audio clock master (mpv default), occasional frame drop/repeat.
    Audio,
    /// Resample audio to the display refresh for judder-free motion.
    DisplayResample,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct VideoSettings {
    pub hardware_decoding: HardwareDecoding,
    pub hdr: HdrMode,
    pub tone_mapping: ToneMapping,
    /// Per-scene peak detection for tone mapping (costs GPU time).
    pub hdr_peak_detection: bool,
    pub deinterlace: Deinterlace,
    pub frame_sync: FrameSync,
    /// Motion interpolation (requires display-resample).
    pub interpolation: bool,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            hardware_decoding: HardwareDecoding::Auto,
            hdr: HdrMode::Auto,
            tone_mapping: ToneMapping::Auto,
            hdr_peak_detection: true,
            deinterlace: Deinterlace::Auto,
            frame_sync: FrameSync::Audio,
            interpolation: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum SubtitleMode {
    Off,
    /// Only forced tracks (signs/foreign dialogue).
    ForcedOnly,
    /// Forced tracks, plus full subtitles when audio is not in a preferred language.
    Smart,
    Always,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct SubtitleSettings {
    pub mode: SubtitleMode,
    pub languages: Vec<String>,
    /// System font family; empty = the platform's default subtitle font.
    /// Replaces the former `font` field, whose default ("Inter") is only
    /// bundled with the UI, not installed where libass looks for fonts.
    pub font_family: String,
    pub bold: bool,
    /// Relative size, 1.0 = default.
    pub scale: f32,
    pub color: String,
    pub background: String,
    pub background_opacity: f32,
    pub outline: f32,
    /// Vertical position, 100 = bottom.
    pub position: u8,
    /// Override ASS styling with the settings above.
    pub override_ass: bool,
}

impl Default for SubtitleSettings {
    fn default() -> Self {
        Self {
            mode: SubtitleMode::Smart,
            languages: Vec::new(),
            font_family: String::new(),
            bold: true,
            scale: 1.0,
            color: "#ffffff".into(),
            background: "#000000".into(),
            background_opacity: 0.0,
            outline: 1.6,
            position: 100,
            override_ass: false,
        }
    }
}

/// What belongs to a person rather than to the machine (multi-user
/// profiles): languages, subtitle look, autoplay and skipping, quality cap,
/// motion. Everything else (audio output, decoding, network, cache,
/// controller…) stays shared.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PersonalSettings {
    pub subtitles: SubtitleSettings,
    pub preferred_audio_languages: Vec<String>,
    pub autoplay_next: bool,
    pub autoplay_countdown_secs: u32,
    pub skip_intro: SkipMode,
    pub skip_credits: SkipMode,
    pub max_bitrate: Option<u64>,
    pub resume: ResumeBehavior,
    pub accent: String,
    pub animation_intensity: f32,
    pub background_intensity: f32,
}

impl Default for PersonalSettings {
    fn default() -> Self {
        Self::from_settings(&Settings::default())
    }
}

impl PersonalSettings {
    pub fn from_settings(s: &Settings) -> Self {
        Self {
            subtitles: s.subtitles.clone(),
            preferred_audio_languages: s.playback.preferred_audio_languages.clone(),
            autoplay_next: s.playback.autoplay_next,
            autoplay_countdown_secs: s.playback.autoplay_countdown_secs,
            skip_intro: s.playback.skip_intro,
            skip_credits: s.playback.skip_credits,
            max_bitrate: s.playback.max_bitrate,
            resume: s.playback.resume,
            accent: s.appearance.accent.clone(),
            animation_intensity: s.appearance.animation_intensity,
            background_intensity: s.appearance.background_intensity,
        }
    }

    pub fn apply(&self, s: &mut Settings) {
        s.subtitles = self.subtitles.clone();
        s.playback.preferred_audio_languages = self.preferred_audio_languages.clone();
        s.playback.autoplay_next = self.autoplay_next;
        s.playback.autoplay_countdown_secs = self.autoplay_countdown_secs;
        s.playback.skip_intro = self.skip_intro;
        s.playback.skip_credits = self.skip_credits;
        s.playback.max_bitrate = self.max_bitrate;
        s.playback.resume = self.resume;
        s.appearance.accent = self.accent.clone();
        s.appearance.animation_intensity = self.animation_intensity;
        s.appearance.background_intensity = self.background_intensity;
    }
}

/// Splits settings edited while a profile is active: the personal part goes
/// to the profile; the rest becomes the new shared base, which keeps its own
/// personal values (the defaults new profiles start from).
pub fn split_settings(effective: &Settings, base: &Settings) -> (Settings, PersonalSettings) {
    let mut shared = effective.clone();
    PersonalSettings::from_settings(base).apply(&mut shared);
    (shared, PersonalSettings::from_settings(effective))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum IpFamily {
    Any,
    V4Only,
    V6Only,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct NetworkSettings {
    pub concurrent_requests: u8,
    pub timeout_secs: u32,
    /// Demuxer read-ahead buffer, in MiB.
    pub buffer_mib: u32,
    pub proxy: Option<String>,
    pub ip_family: IpFamily,
    /// Accept invalid/self-signed TLS certificates for *all* servers.
    /// Explicit opt-in for LAN servers with self-signed certificates; the UI
    /// shows a permanent warning while enabled.
    pub allow_invalid_certificates: bool,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            concurrent_requests: 6,
            timeout_secs: 15,
            buffer_mib: 150,
            proxy: None,
            ip_family: IpFamily::Any,
            allow_invalid_certificates: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct CacheSettings {
    pub image_cache_mib: u32,
    pub metadata_ttl_secs: u32,
}

impl Default for CacheSettings {
    fn default() -> Self {
        Self { image_cache_mib: 1024, metadata_ttl_secs: 300 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct ControllerSettings {
    pub enabled: bool,
    pub deadzone: f32,
    /// Swap A/B (Nintendo layout).
    pub swap_confirm: bool,
}

impl Default for ControllerSettings {
    fn default() -> Self {
        Self { enabled: true, deadzone: 0.35, swap_confirm: false }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct NotificationSettings {
    pub next_episode: bool,
    pub server_offline: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self { next_episode: true, server_offline: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct PrivacySettings {
    /// Report playback progress to servers (disabling breaks resume sync).
    pub report_progress: bool,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self { report_progress: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum PresenterChoice {
    Auto,
    Composition,
    ChildWindow,
    DedicatedWindow,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct AdvancedSettings {
    /// Explicit libmpv path (otherwise bundled/system).
    pub libmpv_path: Option<String>,
    pub presenter: PresenterChoice,
    /// Extra raw mpv options, applied last. Power users only.
    pub extra_mpv_options: Vec<(String, String)>,
    pub log_level: String,
    pub show_debug_overlay: bool,
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self {
            libmpv_path: None,
            presenter: PresenterChoice::Auto,
            extra_mpv_options: Vec::new(),
            log_level: "info".into(),
            show_debug_overlay: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_json_fills_defaults() {
        let s: Settings = serde_json::from_str(r#"{"audio":{"passthrough":true}}"#).unwrap();
        assert!(s.audio.passthrough);
        assert_eq!(s.audio.volume, 100);
        assert_eq!(s.video, VideoSettings::default());
    }

    #[test]
    fn legacy_subtitle_font_falls_back_to_platform_default() {
        let s: Settings = serde_json::from_str(r#"{"subtitles":{"font":"Inter","scale":1.2}}"#).unwrap();
        assert_eq!(s.subtitles.font_family, "");
        assert!(s.subtitles.bold);
        assert_eq!(s.subtitles.scale, 1.2);
    }

    #[test]
    fn bitrate_ladder_downscales_below_4k_rates() {
        assert_eq!(max_width_for_bitrate(80_000_000), None);
        assert_eq!(max_width_for_bitrate(20_000_000), Some(1920));
        assert_eq!(max_width_for_bitrate(4_000_000), Some(1280));
        assert_eq!(max_width_for_bitrate(2_000_000), Some(854));
        assert_eq!(max_width_for_bitrate(1_000_000), Some(640));
    }

    #[test]
    fn split_keeps_personal_fields_out_of_the_shared_base() {
        let mut base = Settings::default();
        base.audio.passthrough = true;
        let mut effective = base.clone();
        effective.subtitles.scale = 1.4;
        effective.playback.preferred_audio_languages = vec!["fra".into()];
        effective.playback.autoplay_next = false;
        effective.audio.volume = 70; // a machine setting changed while a profile is active

        let (shared, prefs) = split_settings(&effective, &base);
        assert_eq!(prefs.subtitles.scale, 1.4);
        assert_eq!(prefs.preferred_audio_languages, vec!["fra".to_string()]);
        assert!(!prefs.autoplay_next);
        assert_eq!(shared.audio.volume, 70);
        assert!(shared.audio.passthrough);
        assert_eq!(shared.subtitles.scale, base.subtitles.scale, "the base keeps its own defaults");

        let mut merged = shared.clone();
        prefs.apply(&mut merged);
        assert_eq!(merged, effective);
    }

    #[test]
    fn personal_settings_fill_defaults_from_partial_json() {
        let p: PersonalSettings = serde_json::from_str(r#"{"autoplayNext":false}"#).unwrap();
        assert!(!p.autoplay_next);
        assert_eq!(p.subtitles, SubtitleSettings::default());
        assert_eq!(p.resume, PlaybackSettings::default().resume);
    }
}
