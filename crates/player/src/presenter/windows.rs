//! Windows composition presenter (validated, see ARCHITECTURE.md §4.3).
//!
//! mpv (≥ 0.41) renders into a swapchain created *for composition* and
//! exposes it as `display-swapchain`. We put that swapchain in a
//! DirectComposition visual targeting the app window with `topmost = FALSE`,
//! which DWM composes *behind* the window's child HWNDs — i.e. behind the
//! WebView2, whose transparent areas then reveal the video. DWM handles HDR
//! swapchains natively, so passthrough is preserved.
//!
//! COM objects here are not `Send`; they live in a thread-local on the UI
//! thread and every mutation is dispatched there.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;

use oneshot_mpv::{Mpv, Node};
use parking_lot::Mutex;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain1;
use windows::core::{IUnknown, Interface};

use super::{Presenter, PresenterKind, UiDispatch, Viewport};

struct Tree {
    device: IDCompositionDevice,
    _target: IDCompositionTarget,
    visual: IDCompositionVisual,
    swapchain: Option<IDXGISwapChain1>,
}

thread_local! {
    /// Composition trees keyed by host HWND. UI thread only.
    static TREES: RefCell<HashMap<isize, Tree>> = RefCell::new(HashMap::new());
}

pub struct CompositionPresenter {
    hwnd: isize,
    dispatch: UiDispatch,
    viewport: Mutex<Viewport>,
}

impl std::fmt::Debug for CompositionPresenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositionPresenter").field("hwnd", &self.hwnd).finish_non_exhaustive()
    }
}

impl CompositionPresenter {
    pub fn new(hwnd: isize, dispatch: UiDispatch) -> Self {
        Self { hwnd, dispatch, viewport: Mutex::new(Viewport::default()) }
    }

    fn on_ui(&self, f: impl FnOnce(isize) + Send + 'static) {
        let hwnd = self.hwnd;
        (self.dispatch)(Box::new(move || f(hwnd)));
    }
}

fn create_tree(hwnd: isize) -> windows::core::Result<Tree> {
    // SAFETY: plain COM object creation on the UI thread owning `hwnd`.
    unsafe {
        let device: IDCompositionDevice = DCompositionCreateDevice(None)?;
        let target = device.CreateTargetForHwnd(HWND(hwnd as *mut c_void), false)?;
        let visual = device.CreateVisual()?;
        target.SetRoot(&visual)?;
        device.Commit()?;
        Ok(Tree { device, _target: target, visual, swapchain: None })
    }
}

fn with_tree(hwnd: isize, f: impl FnOnce(&mut Tree) -> windows::core::Result<()>) {
    TREES.with(|trees| {
        let mut trees = trees.borrow_mut();
        let tree = match trees.entry(hwnd) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(v) => match create_tree(hwnd) {
                Ok(t) => v.insert(t),
                Err(e) => {
                    tracing::error!(target: "player", "DirectComposition init failed: {e}");
                    return;
                }
            },
        };
        if let Err(e) = f(tree).and_then(|()| {
            // SAFETY: committing a device we own.
            unsafe { tree.device.Commit() }
        }) {
            tracing::error!(target: "player", "DirectComposition update failed: {e}");
        }
    });
}

impl Presenter for CompositionPresenter {
    fn kind(&self) -> PresenterKind {
        PresenterKind::Composition
    }

    fn init_options(&self) -> Vec<(String, String)> {
        vec![
            ("gpu-context".into(), "d3d11".into()),
            ("d3d11-output-mode".into(), "composition".into()),
            // Real size arrives with the first set_viewport.
            ("d3d11-composition-size".into(), "1280x720".into()),
            ("force-window".into(), "yes".into()),
        ]
    }

    fn observed(&self) -> &'static [&'static str] {
        &["display-swapchain"]
    }

    fn on_property(&self, name: &str, value: &Node) {
        if name != "display-swapchain" {
            return;
        }
        let Some(ptr) = value.as_i64().filter(|p| *p != 0) else { return };
        tracing::info!(target: "player", "composition swapchain available");
        self.on_ui(move |hwnd| {
            with_tree(hwnd, |tree| {
                let raw = ptr as *mut c_void;
                // SAFETY: mpv documents `display-swapchain` as its live
                // IDXGISwapChain1; `clone()` takes our own reference.
                let Some(sc) = (unsafe { IDXGISwapChain1::from_raw_borrowed(&raw) }).cloned() else {
                    return Ok(());
                };
                // SAFETY: valid visual and content object.
                unsafe { tree.visual.SetContent(&sc.cast::<IUnknown>()?)? };
                tree.swapchain = Some(sc);
                Ok(())
            });
        });
    }

    fn set_viewport(&self, mpv: &Mpv, vp: Viewport) {
        if vp.width == 0 || vp.height == 0 {
            return;
        }
        *self.viewport.lock() = vp;
        if let Err(e) = mpv.set_property("d3d11-composition-size", format!("{}x{}", vp.width, vp.height)) {
            tracing::warn!(target: "player", "resize failed: {e}");
        }
        self.on_ui(move |hwnd| {
            with_tree(hwnd, |tree| {
                // SAFETY: valid visual.
                unsafe {
                    tree.visual.SetOffsetX2(vp.x as f32)?;
                    tree.visual.SetOffsetY2(vp.y as f32)?;
                }
                Ok(())
            });
        });
    }

    fn supports_viewport(&self) -> bool {
        true
    }

    fn set_visible(&self, visible: bool) {
        self.on_ui(move |hwnd| {
            with_tree(hwnd, |tree| {
                let content = if visible { tree.swapchain.as_ref().map(|s| s.cast::<IUnknown>()).transpose()? } else { None };
                // SAFETY: valid visual; `None` detaches the content.
                unsafe { tree.visual.SetContent(content.as_ref())? };
                Ok(())
            });
        });
    }
}
