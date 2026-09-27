//! Windows probes: DXGI/DisplayConfig (HDR), D3D11 video decoder profiles,
//! WASAPI endpoints and IEC 61937 passthrough formats.

mod audio;
mod display;
mod video;

use oneshot_core::capabilities::{AudioCapabilities, DisplayCapabilities, GpuInfo, GpuVendor, VideoCapabilities};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIAdapter1, IDXGIFactory1};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

/// Balanced COM init for the probing thread. `RPC_E_CHANGED_MODE` (already
/// initialised as STA by the host, e.g. the UI thread) is fine for our calls.
struct ComGuard(bool);

impl ComGuard {
    fn new() -> Self {
        // SAFETY: plain COM initialisation for the current thread.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        Self(hr.is_ok())
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balanced with a successful CoInitializeEx on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

pub fn probe(notes: &mut Vec<String>) -> (Vec<DisplayCapabilities>, AudioCapabilities, VideoCapabilities) {
    let _com = ComGuard::new();
    let displays = display::probe(notes);
    let audio = audio::probe(notes);
    let video = video::probe(notes);
    (displays, audio, video)
}

pub(crate) fn hardware_adapters() -> Vec<IDXGIAdapter1> {
    // SAFETY: DXGI factory creation has no preconditions.
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return Vec::new();
    };
    (0..)
        // SAFETY: enumerating adapters until DXGI_ERROR_NOT_FOUND.
        .map_while(|i| unsafe { factory.EnumAdapters1(i) }.ok())
        .filter(|a| {
            // SAFETY: valid adapter.
            unsafe { a.GetDesc1() }.is_ok_and(|d| d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0)
        })
        // Virtual display drivers (e.g. Parsec) re-expose the physical GPU
        // under another index; keep one entry per adapter LUID.
        .fold(Vec::<IDXGIAdapter1>::new(), |mut acc, a| {
            // SAFETY: valid adapters.
            let luid = |x: &IDXGIAdapter1| unsafe { x.GetDesc1() }.map(|d| (d.AdapterLuid.HighPart, d.AdapterLuid.LowPart));
            if !acc.iter().any(|b| luid(b).ok() == luid(&a).ok()) {
                acc.push(a);
            }
            acc
        })
}

pub fn gpus() -> Vec<GpuInfo> {
    let _com = ComGuard::new();
    hardware_adapters()
        .iter()
        .filter_map(|a| {
            // SAFETY: valid adapter.
            let d = unsafe { a.GetDesc1() }.ok()?;
            Some(GpuInfo {
                name: wide_to_string(&d.Description),
                vendor: GpuVendor::from_pci_id(d.VendorId),
                dedicated_memory_bytes: Some(d.DedicatedVideoMemory as u64).filter(|m| *m > 0),
                driver_version: None,
            })
        })
        // Virtual display drivers (Parsec, IddCx) have no memory and no decoder.
        .filter(|g| g.dedicated_memory_bytes.is_some() || g.vendor != GpuVendor::Other)
        // Indirect-display drivers can clone the physical GPU's description
        // under a different LUID; report each physical GPU once.
        .fold(Vec::<GpuInfo>::new(), |mut acc, g| {
            if !acc.iter().any(|x| x.name == g.name && x.dedicated_memory_bytes == g.dedicated_memory_bytes) {
                acc.push(g);
            }
            acc
        })
}

pub(crate) fn wide_to_string(w: &[u16]) -> String {
    let end = w.iter().position(|c| *c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end])
}
