//! Display probing.
//!
//! Two sources are combined:
//! * **DisplayConfig** (CCD API): monitor name, refresh rate, and the
//!   *advanced colour* state — "HDR capable" vs "HDR enabled" (Windows "Use
//!   HDR" toggle). DXGI alone cannot tell "capable but off".
//!   Since Windows 11 24H2 "advanced colour" also covers SDR panels running
//!   Auto Color Management / WCG, so the legacy `advancedColorSupported` bit
//!   is set on plain SDR monitors. The newer `ADVANCED_COLOR_INFO_2` query
//!   has a dedicated `highDynamicRangeSupported` bit and is used when the OS
//!   knows it.
//! * **DXGI `IDXGIOutput6::GetDesc1`**: desktop rectangle, active colour space
//!   and the luminance range the OS reports for the panel (from EDID/driver),
//!   used as `target-peak` when we signal HDR ourselves.

use std::collections::HashMap;

use oneshot_core::capabilities::{DisplayCapabilities, HdrState, Rect};
use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
    DISPLAYCONFIG_TARGET_DEVICE_NAME, DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS,
    QueryDisplayConfig,
};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::Graphics::Dxgi::Common::DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020;
use windows::Win32::Graphics::Dxgi::IDXGIOutput6;
use windows::core::Interface;

use super::{hardware_adapters, wide_to_string};

/// Facts from the CCD API, keyed by GDI device name (`\\.\DISPLAY1`).
#[derive(Debug, Default)]
struct CcdInfo {
    friendly_name: Option<String>,
    refresh_hz: Option<f32>,
    hdr_supported: Option<bool>,
    hdr_enabled: Option<bool>,
    bits_per_color: Option<u8>,
}

pub fn probe(notes: &mut Vec<String>) -> Vec<DisplayCapabilities> {
    let ccd = query_ccd().unwrap_or_else(|e| {
        notes.push(format!("DisplayConfig query failed ({e}); HDR 'capable but off' cannot be detected."));
        HashMap::new()
    });

    let mut displays = Vec::new();
    for adapter in hardware_adapters() {
        // SAFETY: enumerate outputs until DXGI_ERROR_NOT_FOUND.
        for output in (0..).map_while(|i| unsafe { adapter.EnumOutputs(i) }.ok()) {
            let Ok(output6) = output.cast::<IDXGIOutput6>() else {
                continue;
            };
            // SAFETY: valid output.
            let Ok(desc) = (unsafe { output6.GetDesc1() }) else {
                continue;
            };
            if !desc.AttachedToDesktop.as_bool() {
                continue;
            }
            let gdi_name = wide_to_string(&desc.DeviceName);
            let info = ccd.get(&gdi_name);
            let r = desc.DesktopCoordinates;
            let hdr_active = desc.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020;
            let lum = |v: f32| Some(v).filter(|v| *v > 0.0);
            let hdr = if hdr_active {
                HdrState::Active {
                    max_luminance: lum(desc.MaxLuminance),
                    min_luminance: Some(desc.MinLuminance).filter(|v| *v >= 0.0),
                    max_full_frame_luminance: lum(desc.MaxFullFrameLuminance),
                }
            } else {
                match info.and_then(|i| i.hdr_supported) {
                    Some(true) => HdrState::SupportedButOff,
                    Some(false) => HdrState::Unsupported,
                    None => HdrState::Unknown { reason: "advanced colour state unavailable".into() },
                }
            };
            displays.push(DisplayCapabilities {
                id: gdi_name.clone(),
                name: info.and_then(|i| i.friendly_name.clone()).unwrap_or_else(|| gdi_name.clone()),
                is_primary: r.left == 0 && r.top == 0,
                width: (r.right - r.left) as u32,
                height: (r.bottom - r.top) as u32,
                refresh_hz: info.and_then(|i| i.refresh_hz),
                bits_per_color: info.and_then(|i| i.bits_per_color).or(Some(desc.BitsPerColor as u8)),
                hdr,
                bounds: Rect { x: r.left, y: r.top, width: (r.right - r.left) as u32, height: (r.bottom - r.top) as u32 },
            });
            if let Some(i) = info
                && i.hdr_enabled == Some(true)
                && !hdr_active
            {
                notes.push(format!("{gdi_name}: HDR reported on but DXGI sees an SDR colour space"));
            }
        }
    }
    displays
}

/// `DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO_2` (Windows 11 24H2+),
/// not yet in the `windows` crate.
const GET_ADVANCED_COLOR_INFO_2: windows::Win32::Devices::Display::DISPLAYCONFIG_DEVICE_INFO_TYPE =
    windows::Win32::Devices::Display::DISPLAYCONFIG_DEVICE_INFO_TYPE(15);
/// `DISPLAYCONFIG_ADVANCED_COLOR_MODE_HDR`.
const ACTIVE_COLOR_MODE_HDR: u32 = 2;

/// `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2` (wingdi.h).
#[repr(C)]
#[derive(Default)]
struct AdvancedColorInfo2 {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    /// Bitfield: 0 advancedColorSupported, 1 advancedColorActive,
    /// 3 advancedColorLimitedByPolicy, 4 highDynamicRangeSupported,
    /// 5 highDynamicRangeUserEnabled, 6 wideColorSupported, 7 wideColorUserEnabled.
    value: u32,
    color_encoding: i32,
    bits_per_color_channel: u32,
    active_color_mode: u32,
}

/// `(hdr supported, hdr enabled, bits per channel)` from the 24H2 query;
/// `None` on older Windows, which rejects the request type.
fn hdr_info2(adapter: windows::Win32::Foundation::LUID, id: u32) -> Option<(bool, bool, Option<u8>)> {
    let mut info = AdvancedColorInfo2 {
        header: header::<AdvancedColorInfo2>(GET_ADVANCED_COLOR_INFO_2, adapter, id),
        ..Default::default()
    };
    // SAFETY: header describes the enclosing, wingdi-compatible struct.
    if unsafe { DisplayConfigGetDeviceInfo(&mut info.header) } != 0 {
        return None;
    }
    let supported = info.value & (1 << 4) != 0;
    let enabled = info.active_color_mode == ACTIVE_COLOR_MODE_HDR;
    Some((supported, enabled, Some(info.bits_per_color_channel as u8).filter(|b| *b > 0)))
}

fn query_ccd() -> Result<HashMap<String, CcdInfo>, String> {
    let (mut n_paths, mut n_modes) = (0u32, 0u32);
    // SAFETY: out-params are valid u32s.
    let rc = unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut n_paths, &mut n_modes) };
    if rc != ERROR_SUCCESS {
        return Err(format!("GetDisplayConfigBufferSizes: {rc:?}"));
    }
    let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); n_paths as usize];
    let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); n_modes as usize];
    // SAFETY: buffers sized from GetDisplayConfigBufferSizes.
    let rc = unsafe {
        QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut n_paths, paths.as_mut_ptr(), &mut n_modes, modes.as_mut_ptr(), None)
    };
    if rc != ERROR_SUCCESS {
        return Err(format!("QueryDisplayConfig: {rc:?}"));
    }
    paths.truncate(n_paths as usize);

    let mut out = HashMap::new();
    for path in &paths {
        let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
            header: header::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>(
                DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                path.sourceInfo.adapterId,
                path.sourceInfo.id,
            ),
            ..Default::default()
        };
        // SAFETY: header describes the enclosing struct type and size.
        if unsafe { DisplayConfigGetDeviceInfo(&mut source.header) } != 0 {
            continue;
        }
        let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME {
            header: header::<DISPLAYCONFIG_TARGET_DEVICE_NAME>(
                DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                path.targetInfo.adapterId,
                path.targetInfo.id,
            ),
            ..Default::default()
        };
        // SAFETY: as above.
        let target_ok = unsafe { DisplayConfigGetDeviceInfo(&mut target.header) } == 0;
        let mut color = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
            header: header::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>(
                DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
                path.targetInfo.adapterId,
                path.targetInfo.id,
            ),
            ..Default::default()
        };
        // SAFETY: as above.
        let color_ok = unsafe { DisplayConfigGetDeviceInfo(&mut color.header) } == 0;
        // SAFETY: `value` is the plain u32 view of the bitfield union.
        let bits = unsafe { color.Anonymous.value };
        // bit 0 advancedColorSupported, bit 1 advancedColorEnabled
        let legacy = color_ok.then(|| (bits & 0b01 != 0, bits & 0b10 != 0, Some(color.bitsPerColorChannel as u8).filter(|b| *b > 0)));
        let color_info = hdr_info2(path.targetInfo.adapterId, path.targetInfo.id).or(legacy);
        let rr = path.targetInfo.refreshRate;
        out.insert(
            wide_to_string(&source.viewGdiDeviceName),
            CcdInfo {
                friendly_name: target_ok
                    .then(|| wide_to_string(&target.monitorFriendlyDeviceName))
                    .filter(|s| !s.is_empty()),
                refresh_hz: (rr.Denominator != 0).then(|| rr.Numerator as f32 / rr.Denominator as f32),
                hdr_supported: color_info.map(|c| c.0),
                hdr_enabled: color_info.map(|c| c.1),
                bits_per_color: color_info.and_then(|c| c.2),
            },
        );
    }
    Ok(out)
}

fn header<T>(
    kind: windows::Win32::Devices::Display::DISPLAYCONFIG_DEVICE_INFO_TYPE,
    adapter: windows::Win32::Foundation::LUID,
    id: u32,
) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
    DISPLAYCONFIG_DEVICE_INFO_HEADER { r#type: kind, size: std::mem::size_of::<T>() as u32, adapterId: adapter, id }
}
