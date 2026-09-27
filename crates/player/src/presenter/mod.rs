//! Video presentation: how mpv's output ends up *under* the WebView UI.
//!
//! See ARCHITECTURE.md §4. The strategy is chosen per OS (and can be forced
//! in Advanced settings); everything above this module is strategy-agnostic.

use std::sync::Arc;

use oneshot_core::settings::PresenterChoice;
use oneshot_mpv::{Mpv, Node};
use serde::Serialize;

#[cfg(windows)]
mod windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum PresenterKind {
    /// Windows: mpv swapchain in a DirectComposition visual behind the WebView.
    Composition,
    /// Windows/X11: mpv child window inside the app window (`--wid`).
    ChildWindow,
    /// mpv owns a separate top-level window; player chrome via mpv OSD.
    DedicatedWindow,
}

/// The native window hosting the WebView.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostWindow {
    Win32 { hwnd: isize },
    Other,
}

/// Runs a closure on the UI (window-owning) thread. Supplied by the shell.
pub type UiDispatch = Arc<dyn Fn(Box<dyn FnOnce() + Send>) + Send + Sync>;

/// Video rectangle in physical pixels, relative to the host client area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Viewport {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub trait Presenter: Send + Sync + std::fmt::Debug {
    fn kind(&self) -> PresenterKind;
    /// Options that must be set before `mpv_initialize`.
    fn init_options(&self) -> Vec<(String, String)>;
    /// Properties this presenter needs to observe.
    fn observed(&self) -> &'static [&'static str] {
        &[]
    }
    fn on_property(&self, _name: &str, _value: &Node) {}
    /// Places the video. Strategies that cannot position it ignore this and
    /// say so in [`Presenter::supports_viewport`].
    fn set_viewport(&self, _mpv: &Mpv, _viewport: Viewport) {}
    fn supports_viewport(&self) -> bool {
        false
    }
    fn set_visible(&self, _visible: bool) {}
    /// Called once, right after `mpv_initialize` succeeds. Presenters that
    /// need the live `Mpv` handle to set something up themselves (rather
    /// than waiting on an mpv-driven property, like `CompositionPresenter`
    /// does) use this. Most presenters don't need it.
    fn on_mpv_ready(&self, _mpv: &Mpv) {}
}

/// Picks the presenter for this platform and libmpv build.
pub fn choose(choice: PresenterChoice, host: HostWindow, dispatch: UiDispatch, composition_supported: bool) -> Arc<dyn Presenter> {
    #[cfg(windows)]
    if let HostWindow::Win32 { hwnd } = host {
        let use_composition = match choice {
            PresenterChoice::Composition | PresenterChoice::Auto => composition_supported,
            _ => false,
        };
        return match choice {
            PresenterChoice::DedicatedWindow => Arc::new(DedicatedWindow),
            _ if use_composition => Arc::new(windows::CompositionPresenter::new(hwnd, dispatch)),
            _ => Arc::new(ChildWindow { wid: hwnd as i64 }),
        };
    }
    let _ = (choice, host, dispatch, composition_supported);
    Arc::new(DedicatedWindow)
}

/// `--wid` embedding: mpv creates a child window covering the whole host.
#[derive(Debug)]
pub struct ChildWindow {
    wid: i64,
}

impl Presenter for ChildWindow {
    fn kind(&self) -> PresenterKind {
        PresenterKind::ChildWindow
    }
    fn init_options(&self) -> Vec<(String, String)> {
        vec![("wid".into(), self.wid.to_string()), ("force-window".into(), "yes".into())]
    }
}

/// Separate mpv-owned window. Used on Linux (X11 cannot alpha-blend a
/// WebKitGTK view over a child window; Wayland has no `--wid`) and as a last
/// resort elsewhere. HDR works here (Wayland colour management, d3d11).
#[derive(Debug)]
pub struct DedicatedWindow;

impl Presenter for DedicatedWindow {
    fn kind(&self) -> PresenterKind {
        PresenterKind::DedicatedWindow
    }
    fn init_options(&self) -> Vec<(String, String)> {
        vec![
            ("force-window".into(), "no".into()),
            ("fullscreen".into(), "yes".into()),
            ("title".into(), "Flick".into()),
            ("osd-level".into(), "1".into()),
        ]
    }
}
