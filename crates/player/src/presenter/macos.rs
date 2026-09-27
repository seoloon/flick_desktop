//! macOS video presentation: mpv's OpenGL render API into a `CAOpenGLLayer`
//! placed under the WebView's `NSView`. See ARCHITECTURE.md §4.2
//! (`LayerRender`) and `docs/superpowers/specs/2026-09-28-macos-video-presenter-design.md`.

use std::ffi::c_void;

use super::{Presenter, PresenterKind, UiDispatch};

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
