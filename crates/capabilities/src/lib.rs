//! Capability Manager.
//!
//! Probes the machine and produces a [`CapabilityReport`]. Probing touches
//! OS APIs that can be slow (audio endpoint activation, D3D device creation),
//! so it runs off the UI thread and the result is cached until invalidated
//! (display change, audio device change, or an explicit refresh).
//!
//! Honesty rule: a probe that cannot run yields `Unknown`/`NotProbed` with a
//! reason — never an optimistic default.

use std::sync::Arc;

use chrono::Utc;
use oneshot_core::capabilities::{
    AudioCapabilities, CapabilityReport, DisplayCapabilities, OsKind, SystemCapabilities, VideoCapabilities,
};
use oneshot_core::stream::VideoCodec;
use parking_lot::RwLock;

#[cfg(windows)]
mod windows;
#[cfg(target_os = "macos")]
mod macos;

/// Codecs the bundled FFmpeg (inside libmpv) decodes in software. This is a
/// property of the engine build, verified at startup against mpv's
/// `decoder-list` by the player crate.
pub const SOFTWARE_VIDEO_CODECS: &[VideoCodec] = &[
    VideoCodec::H264,
    VideoCodec::Hevc,
    VideoCodec::Av1,
    VideoCodec::Vp9,
    VideoCodec::Vp8,
    VideoCodec::Mpeg2,
    VideoCodec::Mpeg4,
    VideoCodec::Vc1,
];

#[derive(Debug, Default)]
pub struct CapabilityManager {
    cached: RwLock<Option<Arc<CapabilityReport>>>,
}

impl CapabilityManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cached report, probing on first use.
    pub fn report(&self) -> Arc<CapabilityReport> {
        if let Some(r) = self.cached.read().as_ref() {
            return Arc::clone(r);
        }
        self.refresh()
    }

    /// Re-probes everything (call on display/audio device change events).
    pub fn refresh(&self) -> Arc<CapabilityReport> {
        let started = std::time::Instant::now();
        let report = Arc::new(probe());
        tracing::info!(
            target: "capabilities",
            elapsed_ms = started.elapsed().as_millis() as u64,
            displays = report.displays.len(),
            audio_devices = report.audio.devices.len(),
            hw_decoders = report.video.hardware_decoders.len(),
            "capabilities probed"
        );
        *self.cached.write() = Some(Arc::clone(&report));
        report
    }
}

fn probe() -> CapabilityReport {
    let mut notes = Vec::new();
    let system = probe_system();
    let (displays, audio, mut video) = probe_platform(&mut notes);
    video.software_codecs = SOFTWARE_VIDEO_CODECS.to_vec();
    CapabilityReport { system, displays, audio, video, probed_at: Utc::now(), notes }
}

fn probe_system() -> SystemCapabilities {
    use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
    let sys = System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::nothing())
            .with_memory(MemoryRefreshKind::nothing().with_ram()),
    );
    SystemCapabilities {
        os: OsKind::current(),
        os_version: System::long_os_version().unwrap_or_else(|| std::env::consts::OS.to_owned()),
        arch: std::env::consts::ARCH.to_owned(),
        cpu_model: sys.cpus().first().map(|c| c.brand().trim().to_owned()).filter(|s| !s.is_empty()),
        cpu_threads: std::thread::available_parallelism().map_or(1, |n| n.get() as u32),
        memory_bytes: Some(sys.total_memory()).filter(|m| *m > 0),
        gpus: platform_gpus(),
    }
}

#[cfg(windows)]
fn platform_gpus() -> Vec<oneshot_core::capabilities::GpuInfo> {
    windows::gpus()
}

#[cfg(not(windows))]
fn platform_gpus() -> Vec<oneshot_core::capabilities::GpuInfo> {
    Vec::new()
}

#[cfg(windows)]
fn probe_platform(notes: &mut Vec<String>) -> (Vec<DisplayCapabilities>, AudioCapabilities, VideoCapabilities) {
    windows::probe(notes)
}

#[cfg(target_os = "macos")]
fn probe_platform(notes: &mut Vec<String>) -> (Vec<DisplayCapabilities>, AudioCapabilities, VideoCapabilities) {
    macos::probe(notes)
}

/// Linux probing is not implemented yet. We report "unknown" so the
/// decision engine stays conservative (no HDR passthrough, no bitstreaming
/// promises) instead of guessing. See ARCHITECTURE.md §8.
#[cfg(not(any(windows, target_os = "macos")))]
fn probe_platform(notes: &mut Vec<String>) -> (Vec<DisplayCapabilities>, AudioCapabilities, VideoCapabilities) {
    notes.push(format!(
        "Display/audio/decoder probing is not implemented on {} yet; capabilities are reported as unknown.",
        std::env::consts::OS
    ));
    (Vec::new(), AudioCapabilities::default(), VideoCapabilities::default())
}
