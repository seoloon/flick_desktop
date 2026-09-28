//! Safe wrapper around libmpv's render API (`render.h`), used by platforms
//! that embed mpv's output into a caller-owned surface instead of letting
//! mpv own a window (macOS `CAOpenGLLayer`; see `oneshot-player`'s presenter).

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::handle::Mpv;
use crate::sys::{self, Api, mpv_render_context};

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
        let api = mpv.api_arc();
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
            // SAFETY: mpv_error_string returns a static string for any code.
            let message = unsafe { crate::node::cstr_lossy((api.mpv_error_string)(rc)) };
            return Err(Error::Mpv { code: rc, message, context: "mpv_render_context_create".into() });
        }
        let raw = NonNull::new(raw).ok_or(Error::Create)?;
        Ok(Self { api, raw })
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
            // SAFETY: mpv_error_string returns a static string for any code.
            let message = unsafe { crate::node::cstr_lossy((self.api.mpv_error_string)(rc)) };
            return Err(Error::Mpv { code: rc, message, context: "mpv_render_context_render".into() });
        }
        Ok(())
    }

    pub fn report_swap(&self) {
        // SAFETY: `self.raw` is a live context.
        unsafe { (self.api.mpv_render_context_report_swap)(self.raw.as_ptr()) };
    }

    // `ctx` is an opaque token handed back to `cb` unexamined by us; we never
    // dereference it, so this stays a safe fn per the interface contract.
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
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
        // context to be current when freeing; the presenter that owns this
        // `RenderContext` field makes its GL context current before
        // dropping it.
        unsafe { (self.api.mpv_render_context_free)(self.raw.as_ptr()) };
    }
}

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
    /// logic depends on.
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
