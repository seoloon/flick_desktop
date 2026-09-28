//! macOS video presentation: mpv's OpenGL render API into a `CAOpenGLLayer`
//! placed under the WebView's `NSView`. See ARCHITECTURE.md §4.2
//! (`LayerRender`) and `docs/superpowers/specs/2026-09-28-macos-video-presenter-design.md`.

use std::ffi::c_void;

use super::{Presenter, PresenterKind, UiDispatch, Viewport};

pub struct LayerPresenter {
    ns_view: *mut c_void,
    // Unused until Task 6 wires up the real `CAOpenGLLayer` implementation,
    // which dispatches AppKit work onto the main thread through this.
    #[allow(dead_code)]
    dispatch: UiDispatch,
}

// SAFETY: `ns_view` is only ever dereferenced on the AppKit main thread via
// `dispatch` (see Task 6); the pointer itself is `Send`/`Sync` to store.
unsafe impl Send for LayerPresenter {}
unsafe impl Sync for LayerPresenter {}

impl std::fmt::Debug for LayerPresenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayerPresenter").field("ns_view", &self.ns_view).finish_non_exhaustive()
    }
}

impl LayerPresenter {
    pub fn new(ns_view: *mut c_void, dispatch: UiDispatch) -> Self {
        Self { ns_view, dispatch }
    }
}

impl Presenter for LayerPresenter {
    fn kind(&self) -> PresenterKind {
        PresenterKind::LayerRender
    }

    fn init_options(&self) -> Vec<(String, String)> {
        vec![("vo".into(), "libmpv".into()), ("force-window".into(), "no".into())]
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct AppKitRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Converts a top-left, physical-pixel [`Viewport`] into an AppKit rect:
/// points, origin at the bottom-left of `view_height_points`.
pub(super) fn appkit_rect(viewport: Viewport, view_height_points: f64, backing_scale: f64) -> AppKitRect {
    let width = f64::from(viewport.width) / backing_scale;
    let height = f64::from(viewport.height) / backing_scale;
    let x = f64::from(viewport.x) / backing_scale;
    let y = view_height_points - (f64::from(viewport.y) / backing_scale + height);
    AppKitRect { x, y, width, height }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presenter::Viewport;

    #[test]
    fn full_height_viewport_sits_flush_at_the_bottom() {
        let vp = Viewport { x: 0, y: 0, width: 1920, height: 1080 };
        let r = appkit_rect(vp, 540.0, 2.0);
        assert_eq!(r, AppKitRect { x: 0.0, y: 0.0, width: 960.0, height: 540.0 });
    }

    #[test]
    fn offset_viewport_converts_pixels_to_points_and_flips_y() {
        let vp = Viewport { x: 100, y: 50, width: 400, height: 300 };
        let r = appkit_rect(vp, 400.0, 2.0);
        assert_eq!(r, AppKitRect { x: 50.0, y: 225.0, width: 200.0, height: 150.0 });
    }
}
