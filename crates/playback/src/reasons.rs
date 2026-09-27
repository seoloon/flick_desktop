/// Stable reason codes. The UI maps them to localized, user-friendly text and
/// the Debug panel shows them raw.
pub mod codes {
    pub const DIRECT_PLAY: &str = "direct-play";
    pub const DIRECT_STREAM: &str = "direct-stream";
    pub const SERVER_REASON: &str = "server-reason";
    pub const SERVER_FORCED_TRANSCODE: &str = "server-forced-transcode";
    pub const NO_DELIVERY: &str = "no-delivery";
    pub const BITRATE_LIMIT: &str = "bitrate-limit";
    pub const TRANSCODE_NO_HWDEC_POLICY: &str = "transcode-no-hwdec-policy";

    pub const VIDEO_CODEC_UNSUPPORTED: &str = "video-codec-unsupported";
    pub const HWDEC: &str = "hwdec";
    pub const SWDEC: &str = "swdec";
    pub const HWDEC_DISABLED: &str = "hwdec-disabled";
    pub const HWDEC_UNKNOWN: &str = "hwdec-unknown";

    pub const HDR_PASSTHROUGH: &str = "hdr-passthrough";
    pub const HDR_TONEMAP: &str = "hdr-tonemap";
    pub const DV_RESHAPE: &str = "dv-reshape";
    pub const DV_FEL_IGNORED: &str = "dv-fel-ignored";

    pub const AUDIO_CODEC_UNSUPPORTED: &str = "audio-codec-unsupported";
    pub const AUDIO_DEVICE_UNKNOWN: &str = "audio-device-unknown";
    pub const BITSTREAM: &str = "bitstream";
    pub const BITSTREAM_DTS_CORE: &str = "bitstream-dts-core";
    pub const AC3_REENCODE: &str = "ac3-reencode";
    pub const BITSTREAM_UNAVAILABLE: &str = "bitstream-unavailable";
    pub const SPATIAL_NEEDS_BITSTREAM: &str = "spatial-needs-bitstream";
    pub const SPATIAL_LOST: &str = "spatial-lost";
    pub const DOWNMIX: &str = "downmix";
    pub const PCM: &str = "pcm";

    pub const SUBTITLE_LOCAL: &str = "subtitle-local";
    pub const SUBTITLE_BURN_IN: &str = "subtitle-burn-in";
}
