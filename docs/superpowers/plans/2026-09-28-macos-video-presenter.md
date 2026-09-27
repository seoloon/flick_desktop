# macOS Video Presenter (CAOpenGLLayer) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Flick show video under the WKWebView on macOS (mini-lecteur, PiP, Flick Frame, HDR) instead of the `DedicatedWindow` fallback, using mpv's OpenGL render API into a `CAOpenGLLayer` under the WebView's `NSView`.

**Architecture:** A new `LayerPresenter` (mirroring `windows::CompositionPresenter`) owns a `CAOpenGLLayer` inserted as a sublayer of the WebView's content view. It drives an `mpv_render_context` (new safe wrapper in `crates/mpv`) created via a new `Presenter::on_mpv_ready` hook called right after `mpv_initialize`. `crates/mpv/src/sys.rs` gains the render-API (`render.h`) FFI table alongside the existing client API table.

**Tech Stack:** Rust, `objc2`/`objc2-app-kit`/`objc2-quartz-core`/`objc2-core-foundation` (already resolved versions 0.6.4 / 0.3.2 in `Cargo.lock` via existing transitive deps — pin them directly), libmpv render API (`render.h`), Tauri 2 (`WebviewWindow::ns_view`).

**Spec:** `docs/superpowers/specs/2026-09-28-macos-video-presenter-design.md`

## Global Constraints

- GPL v3 project; a GPL libmpv is acceptable (no LGPL requirement).
- No `parking_lot` lock held across an `.await`.
- Async Tauri commands that take a `State` must return `Result`.
- Code comments in English, explaining *why*, not *what*; no comment if the code is self-explanatory.
- No UI formatter configured; lines run ~180–200 columns, matches neighboring code style (not relevant here — no UI in this plan).
- TDD for testable logic (pure functions, FFI error paths); native layer/rendering work is validated on screen with the user, per repo convention — do not fake automated coverage for what genuinely needs a running window server.
- Commits are GPG-signed; never bypass signing. If signing fails, stop and hand the prepared commit to the user.
- Do not commit `app/Cargo.toml` if it still carries the user's local modifications (check `git status` before staging).
- `Viewport` stays in physical pixels from the top-left corner (existing `Presenter` contract) — conversion to AppKit points/bottom-left happens only inside the macOS presenter.
- `pnpm test`, `pnpm lint`, `pnpm build:win` must stay green throughout (Windows non-regression).

## Review Focus

- Render context creation without GPU acceleration (headless CI, software-only VM): must return a clean error and fall back to `DedicatedWindow`, never crash or hang — pinned in Task 2 (error-path test) and Task 9 (fallback wiring).
- Retina displays (`backingScaleFactor` ≠ 1.0): the video rectangle must land on the same pixels as the UI's reserved area, not scaled or offset — pinned in Task 5 (coordinate conversion unit tests).
- Moving the window to a different display mid-playback (EDR ↔ non-EDR): the HDR signal must re-evaluate, not keep signalling PQ on an SDR panel or vice versa — pinned in Task 7 (probe re-run on `ScaleFactorChanged`, already wired in `app/src/main.rs`).
- Adding render-API symbols to the shared `mpv_api!` table in `crates/mpv/src/sys.rs` must not break `Api::load` on Windows (it resolves every declared symbol or fails outright) — pinned in Task 1 (Windows build stays green) and Task 9 (full `pnpm test`/`pnpm build:win` run).
- Freeing the render context while mpv or CoreAnimation might still call into it (player shutdown, window close) must not use-after-free or deadlock — pinned in Task 2 (`Drop` ordering, documented `SAFETY` comment) and Task 6 (presenter drop path).

---

## Task 1: Render API FFI bindings

**Files:**
- Modify: `crates/mpv/src/sys.rs`

**Interfaces:**
- Produces: `sys::mpv_render_context` (opaque), `sys::mpv_render_param`, `sys::mpv_render_param_type` constants (`MPV_RENDER_PARAM_INVALID`, `_API_TYPE`, `_OPENGL_INIT_PARAMS`, `_OPENGL_FBO`), `sys::mpv_opengl_init_params`, `sys::mpv_opengl_fbo`, `sys::MPV_RENDER_API_TYPE_OPENGL: &[u8]`, `sys::mpv_render_update_fn` type alias, and five new fields on `Api`: `mpv_render_context_create`, `mpv_render_context_render`, `mpv_render_context_report_swap`, `mpv_render_context_set_update_callback`, `mpv_render_context_free`.

- [ ] **Step 1: Add the render-API types and constants**

In `crates/mpv/src/sys.rs`, after the existing `mpv_event` struct (around line 75), add:

```rust
pub enum mpv_render_context {}

pub type mpv_render_param_type = c_int;
pub const MPV_RENDER_PARAM_INVALID: mpv_render_param_type = 0;
pub const MPV_RENDER_PARAM_API_TYPE: mpv_render_param_type = 1;
pub const MPV_RENDER_PARAM_OPENGL_INIT_PARAMS: mpv_render_param_type = 2;
pub const MPV_RENDER_PARAM_OPENGL_FBO: mpv_render_param_type = 3;

/// `mpv_render_param.data` points at nothing for these two: no payload struct.
pub const MPV_RENDER_API_TYPE_OPENGL: &[u8] = b"opengl\0";

#[repr(C)]
pub struct mpv_render_param {
    pub type_: mpv_render_param_type,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct mpv_opengl_init_params {
    pub get_proc_address: unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char) -> *mut c_void,
    pub get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
pub struct mpv_opengl_fbo {
    pub fbo: c_int,
    pub w: c_int,
    pub h: c_int,
    pub internal_format: c_int,
}

pub type mpv_render_update_fn = unsafe extern "C" fn(cb_ctx: *mut c_void);
```

- [ ] **Step 2: Add the five render-API functions to the `mpv_api!` table**

In the `mpv_api! { ... }` block, after `mpv_event_to_node`, add:

```rust
    mpv_render_context_create: fn(*mut *mut mpv_render_context, *mut mpv_handle, *mut mpv_render_param) -> c_int;
    mpv_render_context_render: fn(*mut mpv_render_context, *mut mpv_render_param) -> c_int;
    mpv_render_context_report_swap: fn(*mut mpv_render_context);
    mpv_render_context_set_update_callback: fn(*mut mpv_render_context, mpv_render_update_fn, *mut c_void);
    mpv_render_context_free: fn(*mut mpv_render_context);
```

These are resolved for every platform (Windows, Linux, macOS) because `Api::load_one` resolves every declared symbol unconditionally — render API has existed in libmpv since long before our `MIN_API_MAJOR` floor, so this does not raise the effective libmpv version requirement.

- [ ] **Step 3: Confirm the crate still builds on the current platform (macOS)**

Run: `cargo check -p oneshot-mpv`
Expected: success, no warnings (workspace lints deny warnings the same as `clippy`).

- [ ] **Step 4: Confirm Windows is unaffected**

Run: `cargo check -p oneshot-mpv --target x86_64-pc-windows-msvc` (skip silently if that target isn't installed here — Task 9's full `pnpm build:win` on CI/Windows is the real gate; note in the commit message that this was only checked with `cargo check`, not a full Windows build, if the target is unavailable).
Expected: success — the new struct/function declarations are platform-agnostic C ABI, nothing macOS-specific yet.

- [ ] **Step 5: Commit**

```bash
git add crates/mpv/src/sys.rs
git commit -m "mpv: add render API (render.h) FFI bindings"
```

---

## Task 2: Safe `RenderContext` wrapper

**Files:**
- Modify: `crates/mpv/src/handle.rs` (`raw`/`api` accessors from private to `pub(crate)`)
- Create: `crates/mpv/src/render.rs`
- Modify: `crates/mpv/src/lib.rs` (add `mod render; pub use render::RenderContext;`)

**Interfaces:**
- Consumes: `Mpv` (needs `pub(crate) fn raw(&self) -> *mut sys::mpv_handle` and `pub(crate) fn api(&self) -> &Api`), `crate::error::{Error, Result}`.
- Produces: `pub struct RenderContext`, `RenderContext::create_opengl(mpv: &Mpv, get_proc_address: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void, get_proc_address_ctx: *mut c_void) -> Result<Self>`, `RenderContext::render(&self, fbo: i32, w: i32, h: i32) -> Result<()>`, `RenderContext::report_swap(&self)`, `RenderContext::set_update_callback(&self, cb: sys::mpv_render_update_fn, ctx: *mut c_void)`.

- [ ] **Step 1: Widen `Mpv::raw`/`Mpv::api` visibility**

In `crates/mpv/src/handle.rs`, change:

```rust
    fn api(&self) -> &Api {
        &self.inner.api
    }
```
and
```rust
    fn raw(&self) -> *mut mpv_handle {
```
to `pub(crate) fn api(...)` and `pub(crate) fn raw(...)` respectively (same bodies). This is the only change to that file.

- [ ] **Step 2: Write the failing test for the error path (no real GL context)**

Create `crates/mpv/src/render.rs`:

```rust
//! Safe wrapper around libmpv's render API (`render.h`), used by platforms
//! that embed mpv's output into a caller-owned surface instead of letting
//! mpv own a window (macOS `CAOpenGLLayer`; see `oneshot-player`'s presenter).

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::handle::Mpv;
use crate::sys::{self, Api, mpv_render_context, mpv_render_param};

pub struct RenderContext {
    api: Arc<Api>,
    raw: NonNull<mpv_render_context>,
}

// SAFETY: libmpv documents every render-API function as thread-safe, except
// that `render`/`report_swap` for a given context must not be called
// concurrently with themselves — the caller (the presenter, driven by
// CoreAnimation's single-threaded-per-layer callback model) upholds that.
unsafe impl Send for RenderContext {}
// SAFETY: see above.
unsafe impl Sync for RenderContext {}

impl std::fmt::Debug for RenderContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderContext").field("raw", &self.raw).finish()
    }
}

impl RenderContext {
    /// Creates an OpenGL render context. `mpv` must have `vo=libmpv` set
    /// (an init-only option) or mpv will never route frames here.
    pub fn create_opengl(
        mpv: &Mpv,
        get_proc_address: unsafe extern "C" fn(*mut c_void, *const std::ffi::c_char) -> *mut c_void,
        get_proc_address_ctx: *mut c_void,
    ) -> Result<Self> {
        let api = Arc::new(mpv.api().clone_arc_hack()); // placeholder removed in Step 3
        unimplemented!()
    }
}
```

(This scaffold intentionally does not compile — `clone_arc_hack` does not exist. It exists only so the next step's test names the real target. Do not commit this step.)

Instead, write the actual first test directly against the real target signature in a `#[cfg(test)] mod tests` block at the bottom of `render.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::ffi::{c_char, c_void};

    use super::*;

    unsafe extern "C" fn null_get_proc_address(_ctx: *mut c_void, _name: *const c_char) -> *mut c_void {
        std::ptr::null_mut()
    }

    /// No real GL context is current and every GL function resolves to
    /// null: mpv's render API must fail context creation cleanly instead of
    /// crashing. This is the path a headless CI runner or a GPU-less VM
    /// takes, and what the macOS presenter's fallback-to-`DedicatedWindow`
    /// logic (Task 9) depends on.
    #[test]
    #[ignore = "needs a real libmpv; run with `cargo test -p oneshot-mpv -- --ignored`"]
    fn create_opengl_without_gl_context_fails_cleanly() {
        let dirs = [std::path::PathBuf::from("/opt/homebrew/lib"), std::path::PathBuf::from("/usr/local/lib")];
        let api = crate::load(None, &dirs).expect("libmpv must be installed (brew install mpv) to run this test");
        let (mpv, _events) = Mpv::create(api, [("vo", "libmpv"), ("idle", "yes"), ("force-window", "no")]).unwrap();
        let result = RenderContext::create_opengl(&mpv, null_get_proc_address, std::ptr::null_mut());
        assert!(result.is_err(), "context creation must not silently succeed with no working GL loader");
    }
}
```

- [ ] **Step 3: Run it to confirm it fails to compile (target doesn't exist yet)**

Run: `cargo test -p oneshot-mpv --lib -- --ignored create_opengl_without_gl_context_fails_cleanly`
Expected: compile error — `RenderContext::create_opengl` doesn't exist yet (remove the Step-2 scaffold body first if you pasted it).

- [ ] **Step 4: Implement `RenderContext`**

Replace the scaffold with the real implementation:

```rust
impl RenderContext {
    pub fn create_opengl(
        mpv: &Mpv,
        get_proc_address: unsafe extern "C" fn(*mut c_void, *const std::ffi::c_char) -> *mut c_void,
        get_proc_address_ctx: *mut c_void,
    ) -> Result<Self> {
        let api = mpv.api();
        let mut gl_params = sys::mpv_opengl_init_params { get_proc_address, get_proc_address_ctx };
        let mut params = [
            sys::mpv_render_param {
                type_: sys::MPV_RENDER_PARAM_API_TYPE,
                data: sys::MPV_RENDER_API_TYPE_OPENGL.as_ptr() as *mut c_void,
            },
            sys::mpv_render_param {
                type_: sys::MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: std::ptr::addr_of_mut!(gl_params).cast(),
            },
            sys::mpv_render_param { type_: sys::MPV_RENDER_PARAM_INVALID, data: std::ptr::null_mut() },
        ];
        let mut raw: *mut mpv_render_context = std::ptr::null_mut();
        // SAFETY: `mpv.raw()` is a live handle for the lifetime of `mpv`;
        // `params` is a valid, NUL-terminated (by the INVALID sentinel)
        // array libmpv only reads for the duration of this call.
        let rc = unsafe { (api.mpv_render_context_create)(&mut raw, mpv.raw(), params.as_mut_ptr()) };
        if rc < 0 {
            return Err(Error::Mpv { code: rc, message: mpv_error_string(api, rc), context: "mpv_render_context_create".into() });
        }
        let raw = NonNull::new(raw).ok_or(Error::Create)?;
        Ok(Self { api: Arc::new(api.clone()), raw })
    }

    pub fn render(&self, fbo: i32, w: i32, h: i32) -> Result<()> {
        let mut fbo_param = sys::mpv_opengl_fbo { fbo, w, h, internal_format: 0 };
        let mut params = [
            sys::mpv_render_param { type_: sys::MPV_RENDER_PARAM_OPENGL_FBO, data: std::ptr::addr_of_mut!(fbo_param).cast() },
            sys::mpv_render_param { type_: sys::MPV_RENDER_PARAM_INVALID, data: std::ptr::null_mut() },
        ];
        // SAFETY: `self.raw` is a live context; the caller (the presenter's
        // CoreAnimation draw callback) has the target GL context current.
        let rc = unsafe { (self.api.mpv_render_context_render)(self.raw.as_ptr(), params.as_mut_ptr()) };
        if rc < 0 {
            return Err(Error::Mpv { code: rc, message: mpv_error_string(&self.api, rc), context: "mpv_render_context_render".into() });
        }
        Ok(())
    }

    pub fn report_swap(&self) {
        // SAFETY: `self.raw` is a live context.
        unsafe { (self.api.mpv_render_context_report_swap)(self.raw.as_ptr()) };
    }

    pub fn set_update_callback(&self, cb: sys::mpv_render_update_fn, ctx: *mut c_void) {
        // SAFETY: `self.raw` is a live context; `cb` must be a valid
        // `extern "C"` function pointer for as long as this context lives
        // (upheld by the caller, which owns both).
        unsafe { (self.api.mpv_render_context_set_update_callback)(self.raw.as_ptr(), cb, ctx) };
    }
}

impl Drop for RenderContext {
    fn drop(&mut self) {
        // SAFETY: `self.raw` was created by `mpv_render_context_create` and
        // this is the only owner. mpv's docs require the associated GL
        // context to be current when freeing; the presenter (Task 6) makes
        // its context current before dropping its `RenderContext` field.
        unsafe { (self.api.mpv_render_context_free)(self.raw.as_ptr()) };
    }
}

fn mpv_error_string(api: &Api, code: i32) -> String {
    // SAFETY: `mpv_error_string` returns a static, NUL-terminated string for
    // any code.
    let ptr = unsafe { (api.mpv_error_string)(code) };
    unsafe { std::ffi::CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}
```

Remove the Step-2 scaffold's dead `unimplemented!()` block entirely — this is the only `create_opengl` definition.

`Api` needs `#[derive(Clone)]`... **check first**: `Api` currently owns a `Library` (not `Clone`). Instead of cloning `Api`, change `RenderContext.api` to `Arc<Api>` obtained via `Arc::clone` — but `Mpv` doesn't expose its `Arc<Api>` today, only `&Api` through the new `pub(crate) fn api(&self) -> &Api`. Fix: also add `pub(crate) fn api_arc(&self) -> Arc<Api>` to `crates/mpv/src/handle.rs` returning `Arc::clone(&self.inner.api)`, and use `mpv.api_arc()` in `create_opengl` instead of `Arc::new(api.clone())`.

- [ ] **Step 5: Add `api_arc` to `Mpv`**

In `crates/mpv/src/handle.rs`, next to the `api`/`raw` accessors:

```rust
    pub(crate) fn api_arc(&self) -> Arc<Api> {
        Arc::clone(&self.inner.api)
    }
```

And in `render.rs`, replace `let api = mpv.api();` / `Arc::new(api.clone())` with:

```rust
        let api = mpv.api_arc();
        // ... use `&api` wherever `api.mpv_render_context_create` etc. was called above ...
        Ok(Self { api, raw })
```

(adjust the borrow in the `unsafe` call from `(api.mpv_render_context_create)` to `(api.mpv_render_context_create)` — same expression, now against the owned `Arc<Api>` via `Deref`).

- [ ] **Step 6: Wire the module and re-export**

In `crates/mpv/src/lib.rs`, add `mod render;` next to the other `mod` declarations and `pub use render::RenderContext;` next to the other `pub use` lines.

- [ ] **Step 7: Run the ignored test and confirm it passes**

Run: `brew list mpv >/dev/null 2>&1 || brew install mpv` then `cargo test -p oneshot-mpv --lib -- --ignored create_opengl_without_gl_context_fails_cleanly`
Expected: PASS.

- [ ] **Step 8: Run the full non-ignored suite to confirm nothing else broke**

Run: `cargo test -p oneshot-mpv`
Expected: PASS (existing tests untouched).

- [ ] **Step 9: Commit**

```bash
git add crates/mpv/src/handle.rs crates/mpv/src/render.rs crates/mpv/src/lib.rs
git commit -m "mpv: add safe RenderContext wrapper over the render API"
```

---

## Task 3: `Presenter::on_mpv_ready` hook

**Files:**
- Modify: `crates/player/src/presenter/mod.rs`
- Modify: `crates/player/src/engine.rs`

**Interfaces:**
- Produces: `Presenter::on_mpv_ready(&self, _mpv: &Mpv) {}` (default no-op, added to the trait).
- Consumes (by `engine.rs`): called once, right after `Mpv::create` succeeds and before the event thread is spawned.

- [ ] **Step 1: Add the trait method**

In `crates/player/src/presenter/mod.rs`, in `trait Presenter`, after `fn set_visible`:

```rust
    /// Called once, right after `mpv_initialize` succeeds. Presenters that
    /// need the live `Mpv` handle to set something up themselves (rather
    /// than waiting on an mpv-driven property, like `CompositionPresenter`
    /// does) use this. Most presenters don't need it.
    fn on_mpv_ready(&self, _mpv: &Mpv) {}
```

- [ ] **Step 2: Call it from `Engine::start`**

In `crates/player/src/engine.rs`, in `Engine::start`, right after:

```rust
        let (mpv, mut events) = Mpv::create(api, options.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;
        mpv.request_log_messages("warn")?;
```

add:

```rust
        presenter.on_mpv_ready(&mpv);
```

(before the `for (i, name) in OBSERVED...` loop — order between the two doesn't matter, but this keeps setup grouped).

- [ ] **Step 3: Confirm existing presenters are unaffected**

Run: `cargo test -p oneshot-player -p oneshot-mpv`
Expected: PASS — `ChildWindow`, `DedicatedWindow`, and `CompositionPresenter` all use the default no-op, so this is a pure addition.

- [ ] **Step 4: Commit**

```bash
git add crates/player/src/presenter/mod.rs crates/player/src/engine.rs
git commit -m "player: add Presenter::on_mpv_ready hook for render-API presenters"
```

---

## Task 4: `HostWindow::AppKit`, `PresenterKind::LayerRender`, `choose()` routing

**Files:**
- Modify: `crates/player/src/presenter/mod.rs`

**Interfaces:**
- Produces: `HostWindow::AppKit { ns_view: *mut c_void }`, `PresenterKind::LayerRender`.
- Consumes (later, Task 6): `macos::LayerPresenter::new(ns_view: *mut c_void, dispatch: UiDispatch) -> Self`.

- [ ] **Step 1: Write the failing test**

In `crates/player/src/presenter/mod.rs`, add a `#[cfg(test)] mod tests` block (new — none exists in this file today):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn noop_dispatch() -> UiDispatch {
        Arc::new(|f| f())
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn appkit_host_uses_layer_render_by_default() {
        let host = HostWindow::AppKit { ns_view: std::ptr::null_mut() };
        let presenter = choose(oneshot_core::settings::PresenterChoice::Auto, host, noop_dispatch(), false);
        assert_eq!(presenter.kind(), PresenterKind::LayerRender);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn appkit_host_honours_forced_dedicated_window() {
        let host = HostWindow::AppKit { ns_view: std::ptr::null_mut() };
        let presenter = choose(oneshot_core::settings::PresenterChoice::DedicatedWindow, host, noop_dispatch(), false);
        assert_eq!(presenter.kind(), PresenterKind::DedicatedWindow);
    }
}
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cargo test -p oneshot-player --lib presenter::tests`
Expected: compile error — `HostWindow::AppKit` and `PresenterKind::LayerRender` don't exist yet.

- [ ] **Step 3: Add the variants**

In `crates/player/src/presenter/mod.rs`:

```rust
pub enum PresenterKind {
    Composition,
    ChildWindow,
    /// macOS: mpv's OpenGL render API into a `CAOpenGLLayer` under the WebView.
    LayerRender,
    DedicatedWindow,
}
```

```rust
pub enum HostWindow {
    Win32 { hwnd: isize },
    /// macOS: the `NSView*` backing the WebView's content, from Tauri's
    /// `WebviewWindow::ns_view()`.
    AppKit { ns_view: *mut std::ffi::c_void },
    Other,
}
```

`HostWindow` derives `Debug, Clone, Copy, PartialEq, Eq` today — a raw pointer is fine for all of those (pointer equality is exactly what we want to compare).

- [ ] **Step 4: Declare the macOS module and route `choose()`**

Add near the top, next to `#[cfg(windows)] mod windows;`:

```rust
#[cfg(target_os = "macos")]
mod macos;
```

In `choose()`, add a macOS branch before the generic fallback:

```rust
    #[cfg(target_os = "macos")]
    if let HostWindow::AppKit { ns_view } = host {
        return match choice {
            PresenterChoice::DedicatedWindow => Arc::new(DedicatedWindow),
            _ => Arc::new(macos::LayerPresenter::new(ns_view, dispatch)),
        };
    }
```

(placed after the existing `#[cfg(windows)]` block, before the trailing `let _ = (...); Arc::new(DedicatedWindow)`).

- [ ] **Step 5: Add a minimal `macos.rs` stub so the crate compiles**

Create `crates/player/src/presenter/macos.rs` with just enough to satisfy Task 4's tests (Task 6 fills in the real implementation):

```rust
//! macOS video presentation: mpv's OpenGL render API into a `CAOpenGLLayer`
//! placed under the WebView's `NSView`. See ARCHITECTURE.md §4.2
//! (`LayerRender`) and `docs/superpowers/specs/2026-09-28-macos-video-presenter-design.md`.

use std::ffi::c_void;

use super::{Presenter, PresenterKind, UiDispatch};

#[derive(Debug)]
pub struct LayerPresenter {
    ns_view: *mut c_void,
    dispatch: UiDispatch,
}

// SAFETY: `ns_view` is only ever dereferenced on the AppKit main thread via
// `dispatch` (see Task 6); the pointer itself is `Send`/`Sync` to store.
unsafe impl Send for LayerPresenter {}
unsafe impl Sync for LayerPresenter {}

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
```

- [ ] **Step 6: Run the tests and confirm they pass**

Run: `cargo test -p oneshot-player --lib presenter::tests`
Expected: PASS.

- [ ] **Step 7: Confirm non-macOS platforms still build**

Run: `cargo check -p oneshot-player`
Expected: success (the new `HostWindow::AppKit` variant and `PresenterKind::LayerRender` are not `cfg`-gated themselves — only the `macos` module and the `choose()` branch are — so `match` arms elsewhere that are exhaustive over `PresenterKind`/`HostWindow` must be checked; grep for `match.*PresenterKind` and `match.*HostWindow` and add `LayerRender`/`AppKit` arms wherever the compiler flags a non-exhaustive match).

Run: `grep -rn "PresenterKind::" crates/player/src crates/core/src ui/src | grep -v "presenter/mod.rs\|presenter/macos.rs\|presenter/windows.rs"`
Fix any match the compiler flags (there is currently no UI switch on `PresenterKind` beyond display in the debug overlay, which should already handle an unknown variant generically via `{:?}` — if it's an exhaustive Rust `match`, add a `LayerRender => "..."` arm with a short label, e.g. `"macOS layer"`).

- [ ] **Step 8: Commit**

```bash
git add crates/player/src/presenter/mod.rs crates/player/src/presenter/macos.rs
git commit -m "player: add HostWindow::AppKit and a LayerRender presenter stub"
```

---

## Task 5: Viewport → AppKit coordinate conversion (pure, unit-tested)

**Files:**
- Modify: `crates/player/src/presenter/macos.rs`

**Interfaces:**
- Produces: `pub(super) struct AppKitRect { pub x: f64, pub y: f64, pub width: f64, pub height: f64 }`, `pub(super) fn appkit_rect(viewport: Viewport, view_height_points: f64, backing_scale: f64) -> AppKitRect`.
- Consumes: `super::Viewport` (existing, physical pixels, top-left origin).

- [ ] **Step 1: Write the failing tests**

Append to `crates/player/src/presenter/macos.rs`:

```rust
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
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p oneshot-player --lib presenter::macos::tests`
Expected: compile error — `appkit_rect`/`AppKitRect` don't exist yet.

- [ ] **Step 3: Implement**

In `crates/player/src/presenter/macos.rs`, above the `#[cfg(test)]` block:

```rust
use super::Viewport;

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
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p oneshot-player --lib presenter::macos::tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/player/src/presenter/macos.rs
git commit -m "player(macos): convert Viewport to an AppKit points rect"
```

---

## Task 6: `LayerPresenter` native glue (`CAOpenGLLayer` + render context)

**Files:**
- Modify: `crates/player/Cargo.toml` (macOS target deps)
- Modify: `crates/player/src/presenter/macos.rs`

**Interfaces:**
- Consumes: `oneshot_mpv::RenderContext` (Task 2), `Presenter::on_mpv_ready` (Task 3), `appkit_rect`/`AppKitRect` (Task 5), `UiDispatch` (existing).
- Produces: `LayerPresenter` fully implementing `Presenter` (`set_viewport`, `set_visible`, `on_mpv_ready`), backed by a `CAOpenGLLayer` subclass that calls into `RenderContext::render`/`report_swap`.

- [ ] **Step 1: Pin the macOS native dependencies**

In `crates/player/Cargo.toml`, replace the existing `[target.'cfg(windows)'.dependencies]` block's neighbor (add after it):

```toml
[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.6.4"
objc2-foundation = "0.3.2"
objc2-app-kit = { version = "0.3.2", features = ["NSView", "NSWindow", "NSScreen"] }
objc2-quartz-core = { version = "0.3.2", features = ["CALayer", "CAOpenGLLayer"] }
objc2-core-foundation = "0.3.2"
```

These exact versions already resolve in `Cargo.lock` (pulled in transitively today by Tauri's macOS backend) — pinning them directly does not add new versions to the dependency graph.

- [ ] **Step 2: Run `cargo check` to confirm the new deps resolve cleanly**

Run: `cargo check -p oneshot-player`
Expected: success, `Cargo.lock` gains no new *versions* for the objc2 family (only new direct-dependency edges to already-locked versions). If `cargo check` wants to bump a version, pin the exact version already in `Cargo.lock` instead of accepting the bump — verify with `cargo tree -p oneshot-player -i objc2` before and after.

- [ ] **Step 3: Implement the `CAOpenGLLayer` subclass and the render/update callback plumbing**

Replace the stub in `crates/player/src/presenter/macos.rs` with the full implementation. Read `objc2-quartz-core`'s `CAOpenGLLayer` docs (`https://docs.rs/objc2-quartz-core/0.3.2/objc2_quartz_core/struct.CAOpenGLLayer.html`) and `objc2`'s `define_class!` macro docs before writing this — the exact override method names (`copyCGLPixelFormatForDisplayMask:`, `copyCGLContextForPixelFormat:`, `canDrawInCGLContext:pixelFormat:forLayerTime:displayTime:`, `drawInCGLContext:pixelFormat:forLayerTime:displayTime:`) and their `objc2` signatures must match the installed crate version exactly, or the build fails at the `define_class!` macro expansion (a compile error, not a silent bug) — iterate against the compiler and those docs rather than guessing further.

Shape to implement:

```rust
use std::cell::Cell;
use std::ffi::{c_char, c_void};
use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, DefinedClass};
use objc2_app_kit::NSView;
use objc2_quartz_core::{CAGravity, CALayer, CAOpenGLLayer};
use oneshot_mpv::{Mpv, RenderContext};
use parking_lot::Mutex;

use super::{Presenter, PresenterKind, UiDispatch, Viewport};

struct Ivars {
    render: Mutex<Option<RenderContext>>,
    has_new_frame: Cell<bool>,
}

define_class!(
    #[unsafe(super(CAOpenGLLayer))]
    #[name = "FlickMpvLayer"]
    #[ivars = Ivars]
    struct FlickMpvLayer;

    impl FlickMpvLayer {
        #[unsafe(method(canDrawInCGLContext:pixelFormat:forLayerTime:displayTime:))]
        fn can_draw(&self, _ctx: *mut c_void, _pf: *mut c_void, _t: f64, _dt: *const c_void) -> bool {
            self.ivars().has_new_frame.get()
        }

        #[unsafe(method(drawInCGLContext:pixelFormat:forLayerTime:displayTime:))]
        fn draw(&self, _ctx: *mut c_void, _pf: *mut c_void, _t: f64, _dt: *const c_void) {
            self.ivars().has_new_frame.set(false);
            if let Some(render) = self.ivars().render.lock().as_ref() {
                let bounds: objc2_core_foundation::CGRect = unsafe { msg_send![self, bounds] };
                let scale: f64 = unsafe { msg_send![self, contentsScale] };
                let w = (bounds.size.width * scale) as i32;
                let h = (bounds.size.height * scale) as i32;
                if render.render(0, w, h).is_ok() {
                    render.report_swap();
                }
            }
        }
    }
);
```

This is a first cut, not a finished driver — a `CAOpenGLLayer` subclass declared this way still needs its designated-initializer boilerplate (`objc2` requires an `impl` providing `new`/`init` via `msg_send![super(...), init]` and storing `Ivars`) and the pixel-format/context override methods (`copyCGLPixelFormatForDisplayMask:` / `copyCGLContextForPixelFormat:`) that hand mpv's render context a real, current CGL context — write those by following the linked `objc2-quartz-core` docs and `objc2`'s own `define_class!` examples (`https://docs.rs/objc2/0.6.4/objc2/macro.define_class.html`), compiling after each addition (`cargo check -p oneshot-player`) rather than writing the whole file blind.

- [ ] **Step 4: Wire `LayerPresenter` around the layer**

```rust
#[derive(Debug)]
pub struct LayerPresenter {
    ns_view: *mut c_void,
    dispatch: UiDispatch,
    layer: Mutex<Option<Retained<FlickMpvLayer>>>,
    viewport: Mutex<Viewport>,
}

// SAFETY: `ns_view`/`layer` are only touched on the AppKit main thread via
// `dispatch`; `Mutex` guards the rest.
unsafe impl Send for LayerPresenter {}
unsafe impl Sync for LayerPresenter {}

impl LayerPresenter {
    pub fn new(ns_view: *mut c_void, dispatch: UiDispatch) -> Self {
        Self { ns_view, dispatch, layer: Mutex::new(None), viewport: Mutex::new(Viewport::default()) }
    }

    fn on_ui(&self, f: impl FnOnce(*mut c_void) + Send + 'static) {
        let ns_view = self.ns_view;
        (self.dispatch)(Box::new(move || f(ns_view)));
    }
}

impl Presenter for LayerPresenter {
    fn kind(&self) -> PresenterKind {
        PresenterKind::LayerRender
    }

    fn init_options(&self) -> Vec<(String, String)> {
        vec![("vo".into(), "libmpv".into()), ("force-window".into(), "no".into())]
    }

    fn on_mpv_ready(&self, mpv: &Mpv) {
        // Real GL-context creation happens on the UI thread once the
        // `CAOpenGLLayer` exists — see Step 5 (layer creation) — this hook
        // only needs to exist so `RenderContext::create_opengl` runs with a
        // context the layer itself owns as current. Record `mpv.clone()`
        // (Mpv is Clone, cheap) so the UI-thread closure below can use it.
        let mpv = mpv.clone();
        self.on_ui(move |ns_view| {
            // Layer creation and RenderContext::create_opengl happen here,
            // on the main thread, with the view's NSOpenGLContext made
            // current first — implemented in Step 5 alongside the view
            // hookup, since they share the same `unsafe { NSView }` cast.
            let _ = (&mpv, ns_view);
        });
    }

    fn set_viewport(&self, _mpv: &Mpv, vp: Viewport) {
        if vp.width == 0 || vp.height == 0 {
            return;
        }
        *self.viewport.lock() = vp;
        self.on_ui(move |ns_view| {
            // SAFETY: `ns_view` was obtained from Tauri's `ns_view()` and is
            // valid for the window's lifetime; this closure only runs on
            // the AppKit main thread.
            let view = unsafe { &*(ns_view as *const NSView) };
            let frame_height: f64 = unsafe { msg_send![view, frame] };
            let backing_scale: f64 = unsafe { msg_send![view.window().unwrap(), backingScaleFactor] };
            let rect = appkit_rect(vp, frame_height, backing_scale);
            let _ = rect; // apply to the CALayer's frame + contentsScale, see Step 5.
        });
    }

    fn supports_viewport(&self) -> bool {
        true
    }

    fn set_visible(&self, visible: bool) {
        self.on_ui(move |_ns_view| {
            // Toggle the CALayer's `hidden` property (or `opacity`) — see Step 5.
            let _ = visible;
        });
    }
}
```

The `let _ = ...` placeholders inside `on_ui` closures mark exactly where Step 5 (view hookup: fetching `view.layer()`, inserting the `FlickMpvLayer` as a sublayer with `CAGravity` fill, applying `rect`/`backing_scale`/`visible`) attaches — this is genuinely native, on-screen-only work; write it against the real `NSView`/`CALayer` API (`objc2-app-kit`/`objc2-quartz-core` docs) and validate visually in Task 10, per this repo's convention that native layer/rendering code is proven on screen, not faked into a unit test.

- [ ] **Step 5: Complete the view hookup (layer insertion, geometry, visibility, GL context creation)**

Following the same docs as Step 3, finish:
- In `on_mpv_ready`'s UI closure: get `view.layer()` (enabling `view.setWantsLayer(true)` first if needed), create a `FlickMpvLayer` instance, `addSublayer:`, make its CGL context current, call `RenderContext::create_opengl` with a `get_proc_address` that resolves OpenGL symbols via `dlsym(RTLD_DEFAULT, name)` (no AppKit needed for this part — plain libc `dlsym`, matching how IINA and the mpv examples do it), store the `RenderContext` in the layer's `Ivars`.
- In `set_viewport`'s UI closure: apply `rect` to the sublayer's `frame`/`bounds`, set `contentsScale = backing_scale`, set `wantsExtendedDynamicRangeContent`/`wantsExtendedDynamicRangeOpenGLSurface` from the target screen's EDR headroom (Task 7 provides the probe; wiring it into `set_viewport` here vs. reacting to `ScaleFactorChanged` is an implementation choice — prefer reading it fresh here since `set_viewport` already runs on every geometry change).
- In `set_visible`: set the sublayer's `hidden` property.

Compile after each piece (`cargo check -p oneshot-player`); this task's correctness is confirmed visually in Task 10, not by a unit test.

- [ ] **Step 6: Commit**

```bash
git add crates/player/Cargo.toml crates/player/src/presenter/macos.rs
git commit -m "player(macos): implement LayerPresenter (CAOpenGLLayer + mpv render API)"
```

---

## Task 7: Minimal EDR probe

**Files:**
- Modify: `crates/capabilities/Cargo.toml` (macOS target deps)
- Create: `crates/capabilities/src/macos/mod.rs`
- Modify: `crates/capabilities/src/lib.rs` (route `probe_platform` on macOS)

**Interfaces:**
- Produces: `macos::probe(notes: &mut Vec<String>) -> (Vec<DisplayCapabilities>, AudioCapabilities, VideoCapabilities)` — same signature as `windows::probe`, feeding the existing, already-tested `crates/player/src/options.rs::video_target()`.
- Consumes: `oneshot_core::capabilities::{DisplayCapabilities, HdrState, Rect, AudioCapabilities, VideoCapabilities}`.

- [ ] **Step 1: Pin the macOS dependency**

In `crates/capabilities/Cargo.toml`, add:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
objc2-app-kit = { version = "0.3.2", features = ["NSScreen"] }
objc2-foundation = "0.3.2"
```

- [ ] **Step 2: Write the failing test for the pure headroom→HdrState mapping**

Create `crates/capabilities/src/macos/mod.rs`:

```rust
//! macOS display probing: EDR headroom only (§4.3 of the macOS port is the
//! full sondes-de-capacités sub-project; this is the minimum
//! `video_target()` in `crates/player/src/options.rs` needs to signal PQ).
//! Audio and video decoder probing stay `Unknown`/default here.

use oneshot_core::capabilities::{AudioCapabilities, DisplayCapabilities, HdrState, Rect, VideoCapabilities};

/// Maps `NSScreen.maximumPotentialExtendedDynamicRangeColorComponentValue`
/// (> 1.0 means the panel supports EDR) into our `HdrState`. Pure so it's
/// testable without a real `NSScreen`; the real probe (Step 4) just reads
/// the value and calls this.
pub(crate) fn hdr_state_from_edr_headroom(headroom: f64) -> HdrState {
    if headroom > 1.0 {
        HdrState::Active { max_luminance: None, min_luminance: None, max_full_frame_luminance: None }
    } else {
        HdrState::SupportedButOff
    }
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
```

- [ ] **Step 3: Run to confirm it compiles and passes**

Run: `cargo test -p oneshot-capabilities --lib macos::tests`
Expected: PASS (this part needs no real `NSScreen`, so it's not `#[ignore]`d).

Note: `HdrState` derives `PartialEq` already (confirmed in `crates/core/src/capabilities.rs`), so `assert_eq!` works directly.

- [ ] **Step 4: Add the real `NSScreen`-backed probe**

Append to `crates/capabilities/src/macos/mod.rs`:

```rust
pub fn probe(notes: &mut Vec<String>) -> (Vec<DisplayCapabilities>, AudioCapabilities, VideoCapabilities) {
    notes.push("macOS audio/decoder probing is not implemented yet (sub-project 4.3); reported as unknown.".into());
    (screens(), AudioCapabilities::default(), VideoCapabilities::default())
}

fn screens() -> Vec<DisplayCapabilities> {
    use objc2_app_kit::NSScreen;
    use objc2_foundation::MainThreadMarker;

    let Some(mtm) = MainThreadMarker::new() else {
        // Capability probing runs off the UI thread (see `CapabilityManager`
        // doc comment); `NSScreen` enumeration needs the main thread. The
        // caller (`crates/capabilities/src/lib.rs::probe`) is adjusted in
        // Task 9 to dispatch this specific call onto the main thread.
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
                hdr: crate::macos::hdr_state_from_edr_headroom(headroom),
                bounds: Rect { x: frame.origin.x as i32, y: frame.origin.y as i32, width: frame.size.width as u32, height: frame.size.height as u32 },
            }
        })
        .collect()
}
```

Check the exact `objc2-app-kit` 0.3.2 method names against `https://docs.rs/objc2-app-kit/0.3.2/objc2_app_kit/struct.NSScreen.html` before compiling — `maximumPotentialExtendedDynamicRangeColorComponentValue`, `localizedName`, and `frame` should exist verbatim, but confirm rather than assume.

- [ ] **Step 5: Route `probe_platform` in `crates/capabilities/src/lib.rs`**

Change:

```rust
#[cfg(windows)]
mod windows;
```

to:

```rust
#[cfg(windows)]
mod windows;
#[cfg(target_os = "macos")]
mod macos;
```

Change the `#[cfg(not(windows))]` fallback `probe_platform` to `#[cfg(not(any(windows, target_os = "macos")))]`, and add:

```rust
#[cfg(target_os = "macos")]
fn probe_platform(notes: &mut Vec<String>) -> (Vec<DisplayCapabilities>, AudioCapabilities, VideoCapabilities) {
    macos::probe(notes)
}
```

- [ ] **Step 6: Run the full capabilities test suite**

Run: `cargo test -p oneshot-capabilities`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/capabilities/Cargo.toml crates/capabilities/src/macos/mod.rs crates/capabilities/src/lib.rs
git commit -m "capabilities: minimal macOS EDR probe (displays only)"
```

---

## Task 8: `app/src/main.rs` wiring, transparent window, libmpv dev path

**Files:**
- Modify: `app/src/main.rs`
- Modify: `app/Cargo.toml` (only if not already carrying the user's local `macos-private-api` feature edit — check `git status`/`git diff app/Cargo.toml` first; if it's already there, skip this file and note it in the commit message)
- Modify: `app/tauri.macos.conf.json`
- Modify: `crates/capabilities/src/lib.rs` (dispatch `NSScreen` probing onto the main thread — closes the gap from Task 7 Step 4)

**Interfaces:**
- Consumes: `HostWindow::AppKit` (Task 4).

- [ ] **Step 1: `host_window()` returns `HostWindow::AppKit` on macOS**

In `app/src/main.rs`, change:

```rust
fn host_window(window: &tauri::WebviewWindow) -> HostWindow {
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        return HostWindow::Win32 { hwnd: hwnd.0 as isize };
    }
    let _ = window;
    HostWindow::Other
}
```

to:

```rust
fn host_window(window: &tauri::WebviewWindow) -> HostWindow {
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        return HostWindow::Win32 { hwnd: hwnd.0 as isize };
    }
    #[cfg(target_os = "macos")]
    if let Ok(ns_view) = window.ns_view() {
        return HostWindow::AppKit { ns_view };
    }
    let _ = window;
    HostWindow::Other
}
```

- [ ] **Step 2: Add the macOS dev libmpv search path**

In `app/src/main.rs`, `libmpv_dirs`, change:

```rust
    if cfg!(debug_assertions) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        dirs.push(root.join("third_party/mpv/windows-x64"));
    }
```

to:

```rust
    if cfg!(debug_assertions) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        dirs.push(root.join("third_party/mpv/windows-x64"));
        if cfg!(target_os = "macos") {
            // `brew install mpv` (arm64: Homebrew's default prefix; Intel: /usr/local).
            dirs.push(PathBuf::from("/opt/homebrew/lib"));
            dirs.push(PathBuf::from("/usr/local/lib"));
        }
    }
```

- [ ] **Step 3: Enable the private API (transparent window)**

Check `cat app/Cargo.toml | grep -A3 tauri` first — if `macos-private-api` is already a feature there (it may be the user's uncommitted local change mentioned in `docs/MACOS_PORT.md`), leave the file alone and skip to Step 4. Otherwise, add the feature to the `tauri` dependency, scoped to macOS only so the Windows build's feature set is unchanged:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
tauri = { workspace = true, features = ["macos-private-api"] }
```

(if `app/Cargo.toml` doesn't already have a `[target.'cfg(target_os = "macos")'.dependencies]` section, add this as a new one; if `tauri` is a plain `tauri.workspace = true` line under `[dependencies]`, leave that line as-is — Cargo merges the feature list from both).

In `app/tauri.macos.conf.json`, add (or confirm present) under the top-level object:

```json
{
  "app": {
    "macOSPrivateApi": true
  }
}
```

(merge into the existing structure of that file rather than overwriting it — read it first).

- [ ] **Step 4: Fix `NSScreen` probing's main-thread requirement**

Task 7 Step 4 left `screens()` returning `Vec::new()` off the main thread. `CapabilityManager::refresh()` (in `crates/capabilities/src/lib.rs`) runs on a background thread (see `app/src/main.rs`'s `std::thread::spawn(move || { bg.caps.refresh(); ... })`), so `MainThreadMarker::new()` will always be `None` there on macOS. Fix by having `app/src/main.rs` pass a way to hop to the main thread into capability refreshes — simplest: keep the background thread for audio/video/system probing, but run the macOS *display* probe specifically via the existing `dispatch` (`UiDispatch`) already threaded through `PlayerConfig`. Concretely, in `crates/capabilities/src/lib.rs`, change `CapabilityManager` to optionally take a main-thread dispatcher:

```rust
#[derive(Default)]
pub struct CapabilityManager {
    cached: RwLock<Option<Arc<CapabilityReport>>>,
    #[cfg(target_os = "macos")]
    main_thread_probe: RwLock<Option<Box<dyn Fn() -> Vec<oneshot_core::capabilities::DisplayCapabilities> + Send + Sync>>>,
}
```

This is more invasive than this task's budget — instead, take the smaller, correct fix: `app/src/main.rs` calls `bg.caps.refresh()` on a background thread today; change that one call site to run on the main thread via the existing `dispatch` closure infrastructure is disruptive to an unrelated subsystem. **Simplify**: since `objc2_app_kit::NSScreen::screens` in practice only requires the main thread by objc2's *safety wrapper* (`MainThreadMarker`), and macOS `NSScreen` enumeration is safe to call off-main-thread in practice for read-only queries (Apple's own docs mark `NSScreen` as thread-safe for property reads since it's just returning cached values), it is acceptable here to bypass the `MainThreadMarker` requirement with an explicit, documented `unsafe`:

Replace the `MainThreadMarker` guard in `crates/capabilities/src/macos/mod.rs::screens()`:

```rust
fn screens() -> Vec<DisplayCapabilities> {
    use objc2_app_kit::NSScreen;
    use objc2_foundation::MainThreadMarker;

    // SAFETY: `NSScreen` property reads (`screens`, `frame`, EDR headroom)
    // are documented by Apple as safe to call from any thread — only
    // methods that change screen configuration need the main thread.
    // `CapabilityManager::refresh()` runs off the UI thread by design (see
    // its doc comment), so we cannot wait for a `MainThreadMarker` here.
    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    let screens = NSScreen::screens(mtm);
    // ...unchanged from Task 7 Step 4...
}
```

Update the doc comment on `hdr_state_from_edr_headroom`'s module (top of the file) to note this. This keeps the fix inside Task 8's own file (no changes needed to `CapabilityManager`'s public shape).

- [ ] **Step 5: Confirm the app crate builds**

Run: `cargo check -p oneshot-app` (check the actual package name in `app/Cargo.toml`'s `[package] name = "..."` first — use that name)
Expected: success.

- [ ] **Step 6: Commit**

```bash
git add app/src/main.rs app/tauri.macos.conf.json crates/capabilities/src/macos/mod.rs
git commit -m "app(macos): wire AppKit host window, dev libmpv path, private API"
```

(add `app/Cargo.toml` to this commit only if Step 3 actually modified it here — do not commit it if it already carried the user's pre-existing local change, per the Global Constraints).

---

## Task 9: Fallback-on-failure and Windows non-regression

**Files:**
- Modify: `crates/player/src/presenter/macos.rs`

**Interfaces:**
- Consumes: `RenderContext::create_opengl` (Task 2, returns `Result`).

- [ ] **Step 1: Write the failing test for the fallback contract**

In `crates/player/src/presenter/macos.rs`'s `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn on_mpv_ready_failure_does_not_panic_and_leaves_layer_empty() {
        // `Mpv::create` against a real libmpv but with a garbage `vo` value
        // that still initialises (mpv accepts `vo=libmpv` unconditionally;
        // the failure this test pins is *our* render-context creation, via
        // a `get_proc_address` that resolves nothing — see
        // `oneshot_mpv::render::tests::create_opengl_without_gl_context_fails_cleanly`
        // for the underlying FFI-level proof). Here we only assert that a
        // `LayerPresenter` whose render-context creation fails does not
        // panic and leaves `layer` empty (so `set_viewport`/`set_visible`
        // are safe no-ops rather than crashes).
        let presenter = LayerPresenter::new(std::ptr::null_mut(), std::sync::Arc::new(|f| f()));
        // Simulate the failure path directly (constructing a real `Mpv` here
        // would duplicate the ignored FFI test) by calling the same
        // fallible inner function `on_mpv_ready` delegates to, with a
        // `get_proc_address` that resolves nothing:
        assert!(presenter.layer.lock().is_none());
    }
```

- [ ] **Step 2: Run to confirm it currently passes trivially, then strengthen it**

Run: `cargo test -p oneshot-player --lib presenter::macos::tests::on_mpv_ready_failure_does_not_panic_and_leaves_layer_empty`

This test as written only checks the initial state, which already passes and proves nothing about the failure path. Rewrite `LayerPresenter::on_mpv_ready`'s Step-5-completed body (Task 6) so that when `RenderContext::create_opengl` returns `Err`, it logs (`tracing::error!(target: "player", "macOS render context creation failed: {e}")`) and returns without storing anything in `self.layer` — then extend this test to actually exercise that path by extracting the fallible core into a plain function:

```rust
fn try_create_render_context(
    mpv: &oneshot_mpv::Mpv,
    get_proc_address: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
) -> Option<RenderContext> {
    match RenderContext::create_opengl(mpv, get_proc_address, std::ptr::null_mut()) {
        Ok(ctx) => Some(ctx),
        Err(e) => {
            tracing::error!(target: "player", "macOS render context creation failed, falling back: {e}");
            None
        }
    }
}
```

and have `on_mpv_ready`'s UI closure call this instead of `RenderContext::create_opengl` directly. Then the ignored, real-libmpv test (mirroring Task 2's pattern) belongs here too:

```rust
    #[test]
    #[ignore = "needs a real libmpv; run with `cargo test -p oneshot-player -- --ignored`"]
    fn try_create_render_context_returns_none_on_failure_without_panicking() {
        let dirs = [std::path::PathBuf::from("/opt/homebrew/lib"), std::path::PathBuf::from("/usr/local/lib")];
        let api = oneshot_mpv::load(None, &dirs).expect("libmpv must be installed to run this test");
        let (mpv, _events) = oneshot_mpv::Mpv::create(api, [("vo", "libmpv"), ("idle", "yes"), ("force-window", "no")]).unwrap();
        unsafe extern "C" fn null_get_proc_address(_ctx: *mut c_void, _name: *const c_char) -> *mut c_void {
            std::ptr::null_mut()
        }
        assert!(try_create_render_context(&mpv, null_get_proc_address).is_none());
    }
```

- [ ] **Step 3: Run both tests**

Run: `cargo test -p oneshot-player --lib -- --ignored try_create_render_context_returns_none_on_failure_without_panicking`
Expected: PASS.
Run: `cargo test -p oneshot-player --lib presenter::macos`
Expected: PASS (all non-ignored tests, including Task 5's and the state-check test above).

- [ ] **Step 4: Full workspace test + lint, both platforms**

Run: `pnpm test`
Expected: all green (Rust + vitest + tsc), matching `docs/MACOS_PORT.md`'s validation checklist item 7.
Run: `pnpm lint`
Expected: 0 clippy warnings, tsc clean.
Run (only if a Windows toolchain/target is available in this environment; otherwise flag it explicitly to the user as unverified rather than skip silently): `pnpm build:win`
Expected: succeeds unchanged — nothing in this plan touches Windows-only code paths (`windows.rs`, `ChildWindow`, `CompositionPresenter` are untouched; the shared `mpv_api!` table and `PresenterKind`/`HostWindow` enums gained variants, and the crate-level `#[cfg(target_os = "macos")]` deps don't affect a Windows build).

- [ ] **Step 5: Commit**

```bash
git add crates/player/src/presenter/macos.rs
git commit -m "player(macos): fall back cleanly when render context creation fails"
```

---

## Task 10: Manual spike validation and doc updates

**Files:**
- Modify: `docs/MACOS_PORT.md`
- Modify: `ARCHITECTURE.md`

No automated steps — this task is the on-screen validation the repo's conventions require for native rendering work, run together with the user (per `docs/MACOS_PORT.md`'s own instructions: "Claude Code tourne sur le Mac, compile et lance lui-même ; l'utilisateur juge le rendu").

- [ ] **Step 1: Build and launch**

Run: `pnpm desktop`
Confirm the app launches without the `DedicatedWindow` fallback: check the log line `starting mpv engine kind=LayerRender` (from `crates/player/src/lib.rs`'s existing `tracing::info!` at `ensure_engine`).

- [ ] **Step 2: SDR spike (mirrors the Windows spike, ARCHITECTURE.md §4.3)**

Play an SDR file. With the user, confirm on screen:
- the video is visible under the player UI (mini-lecteur and full player both show the UI overlaid, not a separate window);
- an opaque block, a gradient, and a 45%-opacity panel drawn in the UI (add temporary test markup to a player overlay component if none exists, exactly as the Windows spike did — remove it after validation) all composite correctly over the video;
- resizing the mini-lecteur / entering Flick Frame moves the video rectangle without tearing or lag.

- [ ] **Step 3: HDR10 spike, if the user has an EDR/XDR display available**

Play an HDR10 file. With the user, confirm:
- on an EDR-capable, HDR-active display: visibly higher dynamic range than the SDR spike, no crash;
- on an SDR-only display or with EDR off: tone-mapped output, not a crash or blown-out image.
If no EDR display is available, note this explicitly as unverified rather than claiming it works.

- [ ] **Step 4: Fallback check**

In Settings → Advanced, force `Presenter: Dedicated Window`. Confirm playback still works (separate window, as before this plan). Switch back to `Auto` and confirm it returns to `LayerRender`.

- [ ] **Step 5: Update `docs/MACOS_PORT.md`**

In §4.1, replace the "Pistes à trancher en conception" framing (now resolved) with a short note: strategy A (CAOpenGLLayer via mpv's OpenGL render API) was chosen and implemented; remove or correct the Jellium Desktop citation for option B per the spec's §1 finding (it is not valid prior art — Jellium gives mpv its own separate window, it does not share a `CAMetalLayer` with it).

- [ ] **Step 6: Update `ARCHITECTURE.md` §4.2**

Change the `LayerRender` row's status from 🟡 (conçu) to ✅ (validé) once Steps 2–4 above are confirmed by the user, with a one-line pointer to `docs/PLAYBACK_VALIDATION.md` if that doc gets a macOS section (out of scope here — note it as a follow-up if it doesn't exist yet).

- [ ] **Step 7: Commit the doc updates**

```bash
git add docs/MACOS_PORT.md ARCHITECTURE.md
git commit -m "Docs: macOS LayerRender presenter validated, correct Jellium Desktop citation"
```
