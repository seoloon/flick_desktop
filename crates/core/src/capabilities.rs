//! Capability model: what *this* machine can verifiably do.
//!
//! Every field distinguishes "known false" from "unknown". The decision
//! engine treats unknown as "not guaranteed" and must never promise a
//! capability (HDR output, bitstreaming...) that was not positively detected
//! or explicitly forced by the user.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::stream::{BitstreamFormat, VideoCodec};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CapabilityReport {
    pub system: SystemCapabilities,
    pub displays: Vec<DisplayCapabilities>,
    pub audio: AudioCapabilities,
    pub video: VideoCapabilities,
    pub probed_at: DateTime<Utc>,
    /// Human-readable caveats discovered while probing.
    pub notes: Vec<String>,
}

impl CapabilityReport {
    pub fn primary_display(&self) -> Option<&DisplayCapabilities> {
        self.displays.iter().find(|d| d.is_primary).or_else(|| self.displays.first())
    }

    pub fn display(&self, id: &str) -> Option<&DisplayCapabilities> {
        self.displays.iter().find(|d| d.id == id)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct SystemCapabilities {
    pub os: OsKind,
    pub os_version: String,
    pub arch: String,
    pub cpu_model: Option<String>,
    pub cpu_threads: u32,
    pub memory_bytes: Option<u64>,
    pub gpus: Vec<GpuInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum OsKind {
    Windows,
    MacOs,
    Linux,
    Other,
}

impl OsKind {
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Other
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub name: String,
    pub vendor: GpuVendor,
    pub dedicated_memory_bytes: Option<u64>,
    pub driver_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Apple,
    Qualcomm,
    Microsoft,
    Other,
}

impl GpuVendor {
    pub fn from_pci_id(id: u32) -> Self {
        match id {
            0x10DE => Self::Nvidia,
            0x1002 | 0x1022 => Self::Amd,
            0x8086 => Self::Intel,
            0x106B => Self::Apple,
            0x5143 | 0x4D4F4351 => Self::Qualcomm,
            0x1414 => Self::Microsoft,
            _ => Self::Other,
        }
    }
}

/// HDR state of one display *as the OS currently drives it*.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "state")]
pub enum HdrState {
    /// The OS reports the panel cannot do HDR.
    Unsupported,
    /// HDR capable, but the OS HDR mode is off (Windows "Use HDR" toggle).
    /// Playing HDR here means tone-mapping to SDR; we can suggest enabling it.
    SupportedButOff,
    /// OS composes in HDR (Windows advanced colour / macOS EDR / Wayland CM).
    Active {
        max_luminance: Option<f32>,
        min_luminance: Option<f32>,
        max_full_frame_luminance: Option<f32>,
    },
    Unknown { reason: String },
}

impl HdrState {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DisplayCapabilities {
    /// OS identifier (`\\.\DISPLAY1`, CGDirectDisplayID, output name).
    pub id: String,
    pub name: String,
    pub is_primary: bool,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: Option<f32>,
    pub bits_per_color: Option<u8>,
    pub hdr: HdrState,
    /// Desktop rectangle in virtual-screen coordinates (to map a window to a display).
    pub bounds: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width as i32 && y < self.y + self.height as i32
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AudioCapabilities {
    pub devices: Vec<AudioDevice>,
    /// Id of the OS default render device.
    pub default_device: Option<String>,
}

impl AudioCapabilities {
    pub fn device(&self, id: &str) -> Option<&AudioDevice> {
        self.devices.iter().find(|d| d.id == id)
    }

    pub fn default_output(&self) -> Option<&AudioDevice> {
        self.default_device.as_deref().and_then(|id| self.device(id))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum AudioConnection {
    Hdmi,
    DisplayPort,
    Spdif,
    Usb,
    Bluetooth,
    Analog,
    Builtin,
    Virtual,
    Unknown,
}

/// Result of probing a device for compressed passthrough.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "state")]
pub enum PassthroughProbe {
    /// Formats the OS audio stack accepted for exclusive IEC 61937 output.
    /// Acceptance is necessary but not sufficient: the sink (AVR/TV) EDID
    /// may still reject it, which the player detects at runtime.
    Probed { formats: Vec<BitstreamFormat> },
    /// The platform cannot bitstream at all (e.g. no API for it).
    NotSupportedByPlatform { reason: String },
    NotProbed { reason: String },
}

impl PassthroughProbe {
    pub fn supports(&self, format: BitstreamFormat) -> bool {
        matches!(self, Self::Probed { formats } if formats.contains(&format))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    /// OS endpoint id.
    pub id: String,
    /// Name libmpv uses for `--audio-device` (e.g. `wasapi/{guid}`).
    pub mpv_name: Option<String>,
    pub name: String,
    pub connection: AudioConnection,
    /// Channels of the OS mixer format (what shared-mode PCM output gets).
    pub channels: u8,
    pub channel_layout: Option<String>,
    pub sample_rate: Option<u32>,
    pub passthrough: PassthroughProbe,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct VideoCapabilities {
    /// Hardware decode profiles exposed by the GPU driver.
    pub hardware_decoders: Vec<HardwareDecoder>,
    /// Whether hardware decode enumeration actually ran (vs. unknown).
    pub hardware_probe_ok: bool,
    /// Decoders available in the playback engine (software, FFmpeg).
    pub software_codecs: Vec<VideoCodec>,
}

impl VideoCapabilities {
    pub fn hw_decoder(&self, codec: &VideoCodec, bit_depth: u8) -> Option<&HardwareDecoder> {
        self.hardware_decoders.iter().find(|d| &d.codec == codec && d.max_bit_depth >= bit_depth)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct HardwareDecoder {
    pub codec: VideoCodec,
    /// Driver profile name, e.g. `HEVC Main10`.
    pub profile: String,
    pub max_bit_depth: u8,
    /// Decoding API (`d3d11va`, `videotoolbox`, `vaapi`, `nvdec`, `vulkan`).
    pub api: String,
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
}
