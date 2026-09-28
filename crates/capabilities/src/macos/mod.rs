//! macOS display probing: EDR headroom only (§4.3 of the macOS port is the
//! full capability-probing sub-project; this is the minimum `video_target()`
//! in `crates/player/src/options.rs` needs to signal PQ).
//! Audio and video decoder probing stay `Unknown`/default here.

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

pub fn probe(notes: &mut Vec<String>) -> (Vec<DisplayCapabilities>, AudioCapabilities, VideoCapabilities) {
    notes.push("macOS audio/decoder probing is not implemented yet (sub-project 4.3); reported as unknown.".into());
    (screens(), AudioCapabilities::default(), VideoCapabilities::default())
}

fn screens() -> Vec<DisplayCapabilities> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;

    let Some(mtm) = MainThreadMarker::new() else {
        // Capability probing runs off the UI thread (see `CapabilityManager`
        // doc comment); `NSScreen` enumeration needs the main thread. The
        // caller (`crates/capabilities/src/lib.rs::probe`) is adjusted in
        // Task 8 to dispatch this specific call onto the main thread.
        return Vec::new();
    };
    let screens = NSScreen::screens(mtm);
    screens
        .iter()
        .enumerate()
        .map(|(i, screen)| {
            let frame = screen.frame();
            let headroom = screen.maximumPotentialExtendedDynamicRangeColorComponentValue();
            DisplayCapabilities {
                id: format!("nsscreen-{i}"),
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
}
