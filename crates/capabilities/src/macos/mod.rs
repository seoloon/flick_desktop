//! macOS display probing: EDR headroom only (§4.3 of the macOS port is the
//! full capability-probing sub-project; this is the minimum `video_target()`
//! in `crates/player/src/options.rs` needs to signal PQ).
//! Audio and video decoder probing stay `Unknown`/default here.
//!
//! `screens()` below runs off the main thread (`CapabilityManager::refresh()`
//! runs on a background thread by design) and bypasses `objc2`'s
//! `MainThreadMarker` guard with a documented `unsafe` rather than requiring
//! a hop to the main thread — see the `SAFETY` comment on `screens()`.

use oneshot_core::capabilities::{AudioCapabilities, DisplayCapabilities, HdrState, Rect, VideoCapabilities};

/// Maps `NSScreen.maximumPotentialExtendedDynamicRangeColorComponentValue`
/// (> 1.0 means the panel supports EDR) into our `HdrState`. Pure so it's
/// testable without a real `NSScreen`; the real probe (below) just reads
/// the value and calls this.
pub(crate) fn hdr_state_from_edr_headroom(headroom: f64) -> HdrState {
    if headroom > 1.0 {
        HdrState::Active { max_luminance: None, min_luminance: None, max_full_frame_luminance: None }
    } else {
        HdrState::SupportedButOff
    }
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGDisplayModelNumber(display: u32) -> u32;
}

/// The id `tao` gives a monitor (`MonitorHandle::name`), so that
/// `commands::playback::current_display` finds the window's screen in the
/// capability report. `NSScreenNumber` is the screen's `CGDirectDisplayID`.
fn screen_id(screen: &objc2_app_kit::NSScreen) -> Option<String> {
    use objc2_foundation::{NSNumber, NSString};

    let description = screen.deviceDescription();
    let number = description.objectForKey(&NSString::from_str("NSScreenNumber"))?;
    let display_id = number.downcast::<NSNumber>().ok()?.unsignedIntValue();
    // SAFETY: plain CoreGraphics getter taking a display id; unknown ids yield 0.
    Some(format!("Monitor #{}", unsafe { CGDisplayModelNumber(display_id) }))
}

pub fn probe(notes: &mut Vec<String>) -> (Vec<DisplayCapabilities>, AudioCapabilities, VideoCapabilities) {
    notes.push("macOS audio/decoder probing is not implemented yet (sub-project 4.3); reported as unknown.".into());
    let mut displays = screens();
    // Stop-gap: never report `Active` EDR, even for genuinely EDR-capable
    // screens. The `CAOpenGLLayer` we render into today has an 8-bit
    // integer CGL pixel format with no PQ color space set on the layer, so
    // claiming `Active` here would make the playback pipeline send
    // `target-trc=pq`/`target-prim=bt.2020` to mpv, which the layer cannot
    // actually display -- washed-out/wrong colors on real EDR hardware, not
    // just "no HDR". Downgrade to `SupportedButOff` until a color-managed
    // layer (proper PQ colorspace, float pixel format) lands; mpv will then
    // tone-map to SDR instead of sending unmanaged PQ.
    for display in &mut displays {
        if matches!(display.hdr, HdrState::Active { .. }) {
            display.hdr = HdrState::SupportedButOff;
        }
    }
    (displays, AudioCapabilities::default(), VideoCapabilities::default())
}

fn screens() -> Vec<DisplayCapabilities> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;

    // SAFETY: NSScreen property reads are not documented by Apple as
    // thread-safe, but are widely relied upon as such in practice (verified
    // empirically); `CapabilityManager::refresh()` runs off the main thread
    // by design and cannot wait for one, so this is an accepted risk rather
    // than a guarantee.
    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    let screens = NSScreen::screens(mtm);
    screens
        .iter()
        .enumerate()
        .map(|(i, screen)| {
            let frame = screen.frame();
            let headroom = screen.maximumPotentialExtendedDynamicRangeColorComponentValue();
            DisplayCapabilities {
                id: screen_id(&screen).unwrap_or_else(|| format!("nsscreen-{i}")),
                name: screen.localizedName().to_string(),
                is_primary: i == 0,
                width: frame.size.width as u32,
                height: frame.size.height as u32,
                refresh_hz: None,
                bits_per_color: None,
                hdr: hdr_state_from_edr_headroom(headroom),
                bounds: Rect {
                    x: frame.origin.x as i32,
                    y: frame.origin.y as i32,
                    width: frame.size.width as u32,
                    height: frame.size.height as u32,
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headroom_above_one_means_active_edr() {
        assert_eq!(
            hdr_state_from_edr_headroom(4.0),
            HdrState::Active { max_luminance: None, min_luminance: None, max_full_frame_luminance: None }
        );
    }

    #[test]
    fn headroom_at_one_means_supported_but_off() {
        assert_eq!(hdr_state_from_edr_headroom(1.0), HdrState::SupportedButOff);
    }

    #[test]
    fn screens_use_the_monitor_ids_tao_reports() {
        // Headless CI has no screens; on a real Mac every id must be `Monitor #n`.
        for screen in screens() {
            assert!(screen.id.starts_with("Monitor #"), "unexpected id {:?}", screen.id);
        }
    }
}
