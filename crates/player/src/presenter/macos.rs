//! macOS video presentation: mpv's OpenGL render API into a `CAOpenGLLayer`
//! placed under the WebView's `NSView`. See ARCHITECTURE.md §4.2
//! (`LayerRender`) and `docs/superpowers/specs/2026-09-28-macos-video-presenter-design.md`.
//!
//! Shape (mirrors IINA and mpv's own `cocoa-cb`):
//!
//! - [`Surface`] owns a CGL pixel format + context and the mpv
//!   [`RenderContext`] created against it. It is built synchronously in
//!   [`Presenter::on_mpv_ready`] (on whatever thread starts the engine — CGL
//!   is thread-agnostic), so the render context exists before the first
//!   `loadfile` and `vo=libmpv` never races it.
//! - [`FlickMpvLayer`], a `CAOpenGLLayer` subclass, hands CoreAnimation that
//!   pixel format/context (`copyCGLPixelFormatForDisplayMask:` /
//!   `copyCGLContextForPixelFormat:`) and renders mpv's frame in
//!   `drawInCGLContext:…`. It is *not* `asynchronous` (CoreAnimation's
//!   vsync polling of async GL layers never fired in testing): mpv's update
//!   callback wakes a dedicated `mpv-display` thread that calls `-display`
//!   then `CATransaction::flush` — the IINA/`cocoa-cb` pattern — so mpv's
//!   blocking render call never runs on the AppKit main thread.
//! - The layer itself is created and inserted (index 0, i.e. under the
//!   WKWebView's layer) on the AppKit main thread via [`UiDispatch`], and
//!   lives in a main-thread-only registry keyed by presenter id — the same
//!   idiom `windows.rs` uses for its composition tree.
//!
//! Teardown: dropping the presenter dispatches a detach that removes the
//! layer and frees the render context with its GL context current, *then*
//! drops the `Mpv` handle the surface kept alive (libmpv requires the render
//! context to be freed before the core is destroyed).

// CAOpenGLLayer and CGL are deprecated by Apple but remain the only
// OpenGL-backed CoreAnimation path, which is what mpv's render API targets.
#![allow(deprecated)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use objc2::encode::{Encoding, RefEncode};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::AnyObject;
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_app_kit::NSView;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_quartz_core::{CALayer, CAOpenGLLayer, CATransaction, CATransform3D};
use oneshot_mpv::{Mpv, RenderContext};
use parking_lot::{Condvar, Mutex};

use super::{Presenter, PresenterKind, UiDispatch, Viewport};

// ---------------------------------------------------------------------------
// CGL / OpenGL FFI (OpenGL.framework). Only the handful of entry points the
// presenter needs; `objc2-open-gl` is not in the dependency graph.
// ---------------------------------------------------------------------------

/// Opaque `struct _CGLContextObject`.
#[repr(C)]
struct CGLContextObject {
    _private: [u8; 0],
}

/// Opaque `struct _CGLPixelFormatObject`.
#[repr(C)]
struct CGLPixelFormatObject {
    _private: [u8; 0],
}

/// Opaque stand-in for CoreVideo's `CVTimeStamp`; we only pass the pointer
/// through, but its encoding must match the superclass method's for
/// `define_class!`'s (debug-build) override verification.
#[repr(C)]
struct CVTimeStamp {
    _private: [u8; 0],
}

// SAFETY: matches `^{_CGLContextObject=}`, the encoding CoreAnimation's
// headers give `CGLContextObj`.
unsafe impl RefEncode for CGLContextObject {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("_CGLContextObject", &[]));
}

// SAFETY: matches `^{_CGLPixelFormatObject=}` (`CGLPixelFormatObj`).
unsafe impl RefEncode for CGLPixelFormatObject {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("_CGLPixelFormatObject", &[]));
}

const CV_SMPTE_TIME_ENCODING: Encoding = Encoding::Struct(
    "CVSMPTETime",
    &[
        Encoding::Short,
        Encoding::Short,
        Encoding::UInt,
        Encoding::UInt,
        Encoding::UInt,
        Encoding::Short,
        Encoding::Short,
        Encoding::Short,
        Encoding::Short,
    ],
);

// SAFETY: matches `const CVTimeStamp *` as CoreVideo declares it (an
// anonymous struct, hence `?`), field for field.
unsafe impl RefEncode for CVTimeStamp {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct(
        "?",
        &[
            Encoding::UInt,
            Encoding::Int,
            Encoding::LongLong,
            Encoding::ULongLong,
            Encoding::Double,
            Encoding::LongLong,
            CV_SMPTE_TIME_ENCODING,
            Encoding::ULongLong,
            Encoding::ULongLong,
        ],
    ));
}

type CglContext = *mut CGLContextObject;
type CglPixelFormat = *mut CGLPixelFormatObject;

const K_CGLPFA_DOUBLE_BUFFER: i32 = 5;
const K_CGLPFA_ACCELERATED: i32 = 73;
const K_CGLPFA_ALLOW_OFFLINE_RENDERERS: i32 = 96;
const K_CGLPFA_OPENGL_PROFILE: i32 = 99;
const K_CGLPFA_SUPPORTS_AUTOMATIC_GRAPHICS_SWITCHING: i32 = 101;
const K_CGL_OGLP_VERSION_3_2_CORE: i32 = 0x3200;
const K_CGL_OGLP_VERSION_LEGACY: i32 = 0x1000;

const GL_VIEWPORT: u32 = 0x0BA2;
const GL_DRAW_FRAMEBUFFER_BINDING: u32 = 0x8CA6;
const GL_COLOR_BUFFER_BIT: u32 = 0x4000;

/// `RTLD_DEFAULT` on Apple platforms (`<dlfcn.h>`).
const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

#[link(name = "OpenGL", kind = "framework")]
unsafe extern "C" {
    fn CGLChoosePixelFormat(attribs: *const i32, pix: *mut CglPixelFormat, npix: *mut i32) -> i32;
    fn CGLCreateContext(pix: CglPixelFormat, share: CglContext, ctx: *mut CglContext) -> i32;
    fn CGLRetainPixelFormat(pix: CglPixelFormat) -> CglPixelFormat;
    fn CGLReleasePixelFormat(pix: CglPixelFormat);
    fn CGLRetainContext(ctx: CglContext) -> CglContext;
    fn CGLReleaseContext(ctx: CglContext);
    fn CGLGetCurrentContext() -> CglContext;
    fn CGLSetCurrentContext(ctx: CglContext) -> i32;
    fn CGLLockContext(ctx: CglContext) -> i32;
    fn CGLUnlockContext(ctx: CglContext) -> i32;
    fn CGLErrorString(error: i32) -> *const c_char;

    fn glGetIntegerv(pname: u32, data: *mut i32);
    fn glClearColor(r: f32, g: f32, b: f32, a: f32);
    fn glClear(mask: u32);
}

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

fn cgl_error(code: i32) -> String {
    // SAFETY: CGLErrorString returns a static C string for any code.
    let s = unsafe { CGLErrorString(code) };
    if s.is_null() {
        return format!("CGL error {code}");
    }
    // SAFETY: non-null, NUL-terminated, static.
    format!("{} ({code})", unsafe { CStr::from_ptr(s) }.to_string_lossy())
}

/// mpv's OpenGL loader: every GL symbol OpenGL.framework exports (it is
/// linked into the process by the `#[link]` above).
unsafe extern "C" fn get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    // SAFETY: mpv passes a valid NUL-terminated symbol name.
    unsafe { dlsym(RTLD_DEFAULT, name) }
}

/// mpv's render update callback. Runs on arbitrary mpv threads and must not
/// call back into the render API, so it only wakes the display thread.
unsafe extern "C" fn on_mpv_update(ctx: *mut c_void) {
    // SAFETY: `ctx` is the `Surface` itself (`Arc::as_ptr`), which frees its
    // render context (stopping these callbacks) before it is deallocated.
    let surface = unsafe { &*(ctx as *const Surface) };
    surface.request_redraw();
}

/// Makes `ctx` current (locked) on this thread for the duration of `f`,
/// restoring whatever was current before.
fn with_current(ctx: CglContext, f: impl FnOnce()) {
    // SAFETY: `ctx` is a live context owned by the caller's `Surface`;
    // lock/current/unlock is CGL's documented cross-thread protocol.
    unsafe {
        CGLLockContext(ctx);
        let prev = CGLGetCurrentContext();
        CGLSetCurrentContext(ctx);
        f();
        CGLSetCurrentContext(prev);
        CGLUnlockContext(ctx);
    }
}

// ---------------------------------------------------------------------------
// Surface: CGL context + mpv render context.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct GlState {
    render: Option<RenderContext>,
    /// Keeps the mpv core alive until `render` is freed (libmpv: the render
    /// context must be freed before the core is destroyed).
    mpv: Option<Mpv>,
}

#[derive(Default)]
struct Signal {
    pending: bool,
    stop: bool,
}

struct Surface {
    pixel_format: CglPixelFormat,
    context: CglContext,
    /// Set by mpv's update callback (and by geometry changes); consumed by
    /// the next draw (`canDrawInCGLContext:` reports it).
    needs_draw: AtomicBool,
    render_error_logged: AtomicBool,
    gl: Mutex<GlState>,
    /// Wakes the display thread (see `spawn_display_thread`).
    signal: Mutex<Signal>,
    wake: Condvar,
    /// Serialises `-display` between the display thread and any
    /// main-thread display CoreAnimation triggers on its own.
    display_lock: Mutex<()>,
}

// SAFETY: CGL pixel formats and contexts are reference-counted objects that
// may be used from any thread as long as a context is not current on two
// threads at once; every GL use of `context` goes through `gl`'s mutex and
// `CGLLockContext` (see `with_current` and `FlickMpvLayer::draw`).
unsafe impl Send for Surface {}
// SAFETY: see above.
unsafe impl Sync for Surface {}

impl Surface {
    fn create(mpv: &Mpv) -> Result<Arc<Self>, String> {
        Self::create_with_proc_address(mpv, get_proc_address)
    }

    /// The fallible core of `create`, with the GL loader as a parameter so
    /// tests can inject one that resolves nothing and exercise the failure
    /// path (`RenderContext::create_opengl` erroring) without needing a
    /// broken system OpenGL loader — see the `surface_create_*` tests below,
    /// which mirror `oneshot_mpv::render::tests::create_opengl_without_gl_context_fails_cleanly`
    /// (Task 2) one layer up, at the point `LayerPresenter::on_mpv_ready`
    /// actually depends on.
    fn create_with_proc_address(
        mpv: &Mpv,
        get_proc_address: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    ) -> Result<Arc<Self>, String> {
        let pixel_format = choose_pixel_format()?;
        let mut context: CglContext = std::ptr::null_mut();
        // SAFETY: `pixel_format` is valid; `context` is an out-pointer.
        let rc = unsafe { CGLCreateContext(pixel_format, std::ptr::null_mut(), &mut context) };
        if rc != 0 || context.is_null() {
            // SAFETY: we own the +1 reference from CGLChoosePixelFormat.
            unsafe { CGLReleasePixelFormat(pixel_format) };
            return Err(format!("CGLCreateContext failed: {}", cgl_error(rc)));
        }
        let surface = Arc::new(Self {
            pixel_format,
            context,
            needs_draw: AtomicBool::new(true),
            render_error_logged: AtomicBool::new(false),
            gl: Mutex::new(GlState::default()),
            signal: Mutex::new(Signal::default()),
            wake: Condvar::new(),
            display_lock: Mutex::new(()),
        });

        let mut created = Err(String::new());
        with_current(context, || {
            created = RenderContext::create_opengl(mpv, get_proc_address, std::ptr::null_mut()).map_err(|e| e.to_string());
        });
        // `surface` (and the CGL pixel format/context it owns) is dropped
        // cleanly here via `Surface::drop`/`shutdown` if this `?` returns:
        // no leak, no panic, just a `String` handed back to the caller.
        let render = created.map_err(|e| format!("mpv OpenGL render context: {e}"))?;
        render.set_update_callback(on_mpv_update, Arc::as_ptr(&surface).cast_mut().cast::<c_void>());
        *surface.gl.lock() = GlState { render: Some(render), mpv: Some(mpv.clone()) };
        Ok(surface)
    }

    /// Asks for a draw on the display thread even if mpv has no new frame
    /// (geometry changed, layer unhidden) — and is how mpv's own updates
    /// arrive too.
    fn request_redraw(&self) {
        self.needs_draw.store(true, Ordering::Release);
        self.signal.lock().pending = true;
        self.wake.notify_one();
    }

    /// Frees the render context (with its GL context current), stops the
    /// display thread and releases the mpv core. Idempotent.
    fn shutdown(&self) {
        self.signal.lock().stop = true;
        self.wake.notify_one();
        let (render, mpv) = {
            let mut gl = self.gl.lock();
            (gl.render.take(), gl.mpv.take())
        };
        if let Some(render) = render {
            with_current(self.context, || drop(render));
        }
        if let Some(mpv) = mpv {
            // The last `Mpv` clone runs `mpv_terminate_destroy`, which can
            // block; keep that off the AppKit main thread.
            let spawned = std::thread::Builder::new().name("mpv-teardown".into()).spawn(move || drop(mpv));
            if let Err(e) = spawned {
                tracing::warn!(target: "player", "mpv teardown thread: {e}");
            }
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        self.shutdown();
        // SAFETY: we own one reference to each, taken at creation.
        unsafe {
            CGLReleaseContext(self.context);
            CGLReleasePixelFormat(self.pixel_format);
        }
    }
}

fn choose_pixel_format() -> Result<CglPixelFormat, String> {
    let core = [
        K_CGLPFA_OPENGL_PROFILE,
        K_CGL_OGLP_VERSION_3_2_CORE,
        K_CGLPFA_ACCELERATED,
        K_CGLPFA_DOUBLE_BUFFER,
        K_CGLPFA_ALLOW_OFFLINE_RENDERERS,
        K_CGLPFA_SUPPORTS_AUTOMATIC_GRAPHICS_SWITCHING,
        0,
    ];
    let core_unaccelerated = [
        K_CGLPFA_OPENGL_PROFILE,
        K_CGL_OGLP_VERSION_3_2_CORE,
        K_CGLPFA_DOUBLE_BUFFER,
        K_CGLPFA_ALLOW_OFFLINE_RENDERERS,
        0,
    ];
    let legacy = [K_CGLPFA_OPENGL_PROFILE, K_CGL_OGLP_VERSION_LEGACY, K_CGLPFA_DOUBLE_BUFFER, 0];
    let mut last = String::from("no attempts");
    for attribs in [&core[..], &core_unaccelerated[..], &legacy[..]] {
        let mut pf: CglPixelFormat = std::ptr::null_mut();
        let mut count = 0;
        // SAFETY: `attribs` is a 0-terminated attribute list; out-pointers valid.
        let rc = unsafe { CGLChoosePixelFormat(attribs.as_ptr(), &mut pf, &mut count) };
        if rc == 0 && !pf.is_null() {
            return Ok(pf);
        }
        last = if rc == 0 { "no matching pixel format".into() } else { cgl_error(rc) };
    }
    Err(format!("CGLChoosePixelFormat failed: {last}"))
}

// ---------------------------------------------------------------------------
// The CAOpenGLLayer subclass.
// ---------------------------------------------------------------------------

struct Ivars {
    /// `None` only if CoreAnimation copies a layer that is not ours, which
    /// it never does; the layer then just draws nothing.
    surface: Option<Arc<Surface>>,
}

define_class!(
    // SAFETY:
    // - CAOpenGLLayer may be subclassed; overriding the CGL hooks below is
    //   its documented customisation mechanism.
    // - `FlickMpvLayer` does not implement `Drop` (its ivars do, which
    //   objc2 runs on dealloc).
    // - CoreAnimation calls the draw hooks from its own threads, so the
    //   class is `AnyThread` (the default) and the ivars are thread-safe.
    #[unsafe(super(CAOpenGLLayer, CALayer))]
    #[name = "FlickMpvLayer"]
    #[ivars = Ivars]
    struct FlickMpvLayer;

    impl FlickMpvLayer {
        /// `-display` runs the `canDraw…`/`draw…` pair synchronously on the
        /// calling thread: the display thread and CoreAnimation's own
        /// main-thread displays must not interleave on the one GL context.
        #[unsafe(method(display))]
        fn display_locked(&self) {
            let _guard = self.ivars().surface.as_ref().map(|s| s.display_lock.lock());
            // SAFETY: plain superclass call.
            let () = unsafe { msg_send![super(self), display] };
        }

        /// CoreAnimation creates presentation copies of layers through
        /// `initWithLayer:`; they must share the model layer's surface.
        #[unsafe(method_id(initWithLayer:))]
        fn init_with_layer(this: Allocated<Self>, layer: &AnyObject) -> Retained<Self> {
            let surface = layer.downcast_ref::<Self>().and_then(|l| l.ivars().surface.clone());
            let this = this.set_ivars(Ivars { surface });
            // SAFETY: forwarding CALayer's designated copy initialiser.
            unsafe { msg_send![super(this), initWithLayer: layer] }
        }

        #[unsafe(method(copyCGLPixelFormatForDisplayMask:))]
        fn copy_pixel_format(&self, mask: u32) -> CglPixelFormat {
            match &self.ivars().surface {
                // SAFETY: "copy" hands CoreAnimation its own +1 reference,
                // balanced by its `releaseCGLPixelFormat:`.
                Some(s) => unsafe { CGLRetainPixelFormat(s.pixel_format) },
                // SAFETY: plain superclass call.
                None => unsafe { msg_send![super(self), copyCGLPixelFormatForDisplayMask: mask] },
            }
        }

        #[unsafe(method(copyCGLContextForPixelFormat:))]
        fn copy_context(&self, pf: CglPixelFormat) -> CglContext {
            match &self.ivars().surface {
                // SAFETY: as above; balanced by `releaseCGLContext:`. The
                // mpv render context was created against this very context.
                Some(s) => unsafe { CGLRetainContext(s.context) },
                // SAFETY: plain superclass call.
                None => unsafe { msg_send![super(self), copyCGLContextForPixelFormat: pf] },
            }
        }

        #[unsafe(method(canDrawInCGLContext:pixelFormat:forLayerTime:displayTime:))]
        fn can_draw(&self, _ctx: CglContext, _pf: CglPixelFormat, _t: f64, _ts: *const CVTimeStamp) -> bool {
            self.ivars().surface.as_ref().is_some_and(|s| s.needs_draw.load(Ordering::Acquire))
        }

        #[unsafe(method(drawInCGLContext:pixelFormat:forLayerTime:displayTime:))]
        fn draw(&self, ctx: CglContext, pf: CglPixelFormat, t: f64, ts: *const CVTimeStamp) {
            if let Some(surface) = &self.ivars().surface {
                surface.needs_draw.store(false, Ordering::Release);
                let gl = surface.gl.lock();
                // CoreAnimation has `ctx` current and may have bound its own
                // framebuffer: render into whatever it set up.
                let mut fbo = 0;
                let mut viewport = [0i32; 4];
                // SAFETY: `ctx` is current on this thread (CAOpenGLLayer
                // contract); the out-pointers are valid.
                unsafe {
                    glGetIntegerv(GL_DRAW_FRAMEBUFFER_BINDING, &mut fbo);
                    glGetIntegerv(GL_VIEWPORT, viewport.as_mut_ptr());
                }
                let (w, h) = (viewport[2], viewport[3]);
                match gl.render.as_ref() {
                    Some(render) if w > 0 && h > 0 => {
                        if let Err(e) = render.render(fbo, w, h) {
                            if !surface.render_error_logged.swap(true, Ordering::Relaxed) {
                                tracing::error!(target: "player", "mpv render failed: {e}");
                            }
                        }
                    }
                    _ => {
                        // SAFETY: `ctx` is current on this thread.
                        unsafe {
                            glClearColor(0.0, 0.0, 0.0, 0.0);
                            glClear(GL_COLOR_BUFFER_BIT);
                        }
                    }
                }
                // SAFETY: superclass implementation flushes the context.
                let () = unsafe { msg_send![super(self), drawInCGLContext: ctx, pixelFormat: pf, forLayerTime: t, displayTime: ts] };
                if let Some(render) = gl.render.as_ref() {
                    render.report_swap();
                }
            } else {
                // SAFETY: plain superclass call with CoreAnimation's own arguments.
                let () = unsafe { msg_send![super(self), drawInCGLContext: ctx, pixelFormat: pf, forLayerTime: t, displayTime: ts] };
            }
        }
    }
);

impl FlickMpvLayer {
    fn new(surface: Arc<Surface>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Ivars { surface: Some(surface) });
        // SAFETY: CALayer's designated initialiser.
        unsafe { msg_send![super(this), init] }
    }

    fn surface(&self) -> Option<&Surface> {
        self.ivars().surface.as_deref()
    }

    fn request_redraw(&self) {
        if let Some(s) = self.surface() {
            s.request_redraw();
        }
    }
}

/// How soon the display thread retries a `-display` CoreAnimation declined.
const RETRY_DECLINED_DISPLAY: std::time::Duration = std::time::Duration::from_millis(4);
/// Consecutive retries (~200 ms) before waiting for the next update instead.
const MAX_DECLINED_RETRIES: u32 = 50;

/// A retained layer handed to the display thread.
struct SendLayer(Retained<FlickMpvLayer>);
// SAFETY: CALayer retain/release and `-display` are thread-safe (mpv's
// `cocoa-cb` and IINA both display their `CAOpenGLLayer` from a background
// queue); everything the draw path touches is behind the surface's locks.
unsafe impl Send for SendLayer {}

/// Displays the layer off the main thread whenever mpv (or a geometry
/// change) asks for a frame — mpv's render call blocks until the frame's
/// display time, which must never stall AppKit. Exits once the surface is
/// shut down.
fn spawn_display_thread(layer: &Retained<FlickMpvLayer>) {
    let Some(surface) = layer.ivars().surface.clone() else { return };
    let layer = SendLayer(layer.clone());
    let spawned = std::thread::Builder::new().name("mpv-display".into()).spawn(move || {
        let layer = layer;
        let mut declined = 0u32;
        loop {
            {
                let mut signal = surface.signal.lock();
                // CoreAnimation sometimes declines a `-display` without
                // calling `draw…` (it has no free buffer yet; mpv's
                // `cocoa-cb` hits the same). mpv sends no further update
                // until its frame is rendered, so retry shortly rather than
                // wait for one — bounded, so a hidden layer or a locked,
                // asleep display (which decline indefinitely) don't spin.
                if surface.needs_draw.load(Ordering::Acquire) && !signal.pending && !signal.stop && declined < MAX_DECLINED_RETRIES {
                    declined += 1;
                    surface.wake.wait_for(&mut signal, RETRY_DECLINED_DISPLAY);
                    signal.pending = true;
                }
                if !signal.pending && !signal.stop {
                    // A fresh request (mpv update, geometry) re-arms retries.
                    declined = 0;
                    while !signal.pending && !signal.stop {
                        surface.wake.wait(&mut signal);
                    }
                }
                if signal.stop {
                    break;
                }
                signal.pending = false;
            }
            layer.0.display();
            if !surface.needs_draw.load(Ordering::Acquire) {
                declined = 0;
            }
            // Commit the implicit transaction `-display` opened on this
            // (run-loop-less) thread so the frame reaches the screen.
            CATransaction::flush();
        }
    });
    if let Err(e) = spawned {
        tracing::error!(target: "player", "mpv display thread: {e}; video will not be shown");
    }
}

// ---------------------------------------------------------------------------
// Main-thread side: layer registry, attach/geometry/visibility/detach.
// ---------------------------------------------------------------------------

thread_local! {
    /// Live layers keyed by presenter id. AppKit main thread only.
    static LAYERS: RefCell<HashMap<u64, Retained<FlickMpvLayer>>> = RefCell::new(HashMap::new());
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// State the UI thread applies when the layer appears (it may be created
/// after the first `set_viewport`/`set_visible`).
#[derive(Debug, Default, Clone, Copy)]
struct Placement {
    viewport: Option<Viewport>,
    visible: bool,
}

/// Runs `f` inside a transaction with implicit animations disabled: a
/// manually-added sublayer otherwise animates every geometry change.
fn without_animation(f: impl FnOnce()) {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    f();
    CATransaction::commit();
}

/// # Safety
/// `ns_view` must be null or a live `NSView*`; call on the AppKit main thread.
unsafe fn view<'a>(ns_view: *mut c_void) -> Option<&'a NSView> {
    // SAFETY: upheld by the caller.
    unsafe { (ns_view as *const NSView).as_ref() }
}

fn host_layer(view: &NSView) -> Option<Retained<CALayer>> {
    view.setWantsLayer(true);
    // SAFETY: `-[NSView layer]` returns the (possibly nil) backing layer.
    unsafe { msg_send![view, layer] }
}

fn apply_geometry(view: &NSView, layer: &FlickMpvLayer, viewport: Option<Viewport>) {
    let bounds = view.bounds();
    let window = view.window();
    let scale = window.as_ref().map_or(1.0, |w| w.backingScaleFactor());
    let (x, y, width, height) = match viewport {
        Some(vp) => {
            let r = appkit_rect(vp, bounds.size.height, scale);
            // `appkit_rect` is bottom-left based; a flipped view's layer is too
            // (AppKit sets `geometryFlipped`), so convert back to top-left.
            let y = if view.isFlipped() { bounds.size.height - r.y - r.height } else { r.y };
            (r.x, y, r.width, r.height)
        }
        None => (0.0, 0.0, bounds.size.width, bounds.size.height),
    };
    layer.setBounds(CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(width, height)));
    // Default anchor point is the centre.
    layer.setPosition(CGPoint::new(x + width / 2.0, y + height / 2.0));
    layer.setContentsScale(scale);
    let headroom = window
        .and_then(|w| w.screen())
        .map_or(1.0, |s| s.maximumPotentialExtendedDynamicRangeColorComponentValue());
    layer.setWantsExtendedDynamicRangeContent(headroom > 1.0);
    layer.request_redraw();
}

fn attach(id: u64, ns_view: *mut c_void, surface: Arc<Surface>, placement: Placement) {
    // SAFETY: `ns_view` comes from Tauri's `ns_view()` and outlives the
    // player; this runs on the main thread (via `UiDispatch`).
    let Some(view) = (unsafe { view(ns_view) }) else {
        tracing::error!(target: "player", "no host NSView; video will not be shown");
        return;
    };
    let Some(host) = host_layer(view) else {
        tracing::error!(target: "player", "WebView host view has no backing layer; video will not be shown");
        return;
    };
    let layer = FlickMpvLayer::new(surface);
    layer.setOpaque(false);
    // mpv renders top-down, but CoreAnimation reads the layer's framebuffer
    // bottom-up (GL convention) — verified by reading back a half-white test
    // frame. `RenderContext` doesn't expose `MPV_RENDER_PARAM_FLIP_Y` (what
    // IINA/`cocoa-cb` use), so flip the layer about its centre instead.
    layer.setTransform(CATransform3D::new_scale(1.0, -1.0, 1.0));
    layer.setHidden(!placement.visible);
    without_animation(|| {
        // Index 0: below the WKWebView's layer, whose transparent regions
        // reveal the video.
        host.insertSublayer_atIndex(&layer, 0);
        apply_geometry(view, &layer, placement.viewport);
    });
    spawn_display_thread(&layer);
    tracing::info!(target: "player", "macOS video layer attached");
    if let Some(old) = LAYERS.with(|l| l.borrow_mut().insert(id, layer)) {
        detach_layer(&old);
    }
}

fn detach_layer(layer: &FlickMpvLayer) {
    without_animation(|| layer.removeFromSuperlayer());
    if let Some(s) = layer.surface() {
        s.shutdown();
    }
}

fn with_layer(id: u64, f: impl FnOnce(&FlickMpvLayer)) {
    LAYERS.with(|l| {
        if let Some(layer) = l.borrow().get(&id) {
            f(layer);
        }
    });
}

// ---------------------------------------------------------------------------
// Presenter.
// ---------------------------------------------------------------------------

pub struct LayerPresenter {
    id: u64,
    ns_view: *mut c_void,
    dispatch: UiDispatch,
    placement: Arc<Mutex<Placement>>,
}

// SAFETY: `ns_view` is only ever dereferenced on the AppKit main thread via
// `dispatch`; the layer itself never leaves the main thread (it lives in
// `LAYERS`), and the rest is `Mutex`-guarded.
unsafe impl Send for LayerPresenter {}
// SAFETY: see above.
unsafe impl Sync for LayerPresenter {}

impl std::fmt::Debug for LayerPresenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayerPresenter").field("id", &self.id).field("ns_view", &self.ns_view).finish_non_exhaustive()
    }
}

impl LayerPresenter {
    pub fn new(ns_view: *mut c_void, dispatch: UiDispatch) -> Self {
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            ns_view,
            dispatch,
            placement: Arc::new(Mutex::new(Placement::default())),
        }
    }

    fn on_ui(&self, f: impl FnOnce(u64, *mut c_void) + Send + 'static) {
        struct ViewPtr(*mut c_void);
        // SAFETY: only dereferenced by `f` on the main thread.
        unsafe impl Send for ViewPtr {}
        let (id, view) = (self.id, ViewPtr(self.ns_view));
        (self.dispatch)(Box::new(move || {
            let view = view;
            f(id, view.0);
        }));
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
        // GL setup happens here, synchronously, so the render context exists
        // before the caller issues its first `loadfile`.
        let surface = match Surface::create(mpv) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(target: "player", "macOS video layer unavailable, no video will be shown: {e}");
                return;
            }
        };
        tracing::info!(target: "player", "mpv OpenGL render context created");
        let placement = Arc::clone(&self.placement);
        self.on_ui(move |id, ns_view| attach(id, ns_view, surface, *placement.lock()));
    }

    fn set_viewport(&self, _mpv: &Mpv, vp: Viewport) {
        if vp.width == 0 || vp.height == 0 {
            return;
        }
        self.placement.lock().viewport = Some(vp);
        self.on_ui(move |id, ns_view| {
            with_layer(id, |layer| {
                // SAFETY: see `attach`.
                if let Some(view) = unsafe { view(ns_view) } {
                    without_animation(|| apply_geometry(view, layer, Some(vp)));
                }
            });
        });
    }

    fn supports_viewport(&self) -> bool {
        true
    }

    fn set_visible(&self, visible: bool) {
        self.placement.lock().visible = visible;
        self.on_ui(move |id, _| {
            with_layer(id, |layer| {
                without_animation(|| layer.setHidden(!visible));
                if visible {
                    layer.request_redraw();
                }
            });
        });
    }
}

impl Drop for LayerPresenter {
    fn drop(&mut self) {
        self.on_ui(|id, _| {
            if let Some(layer) = LAYERS.with(|l| l.borrow_mut().remove(&id)) {
                detach_layer(&layer);
            }
        });
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

    /// A `get_proc_address` that resolves nothing — mirrors
    /// `oneshot_mpv::render::tests::create_opengl_without_gl_context_fails_cleanly`
    /// (Task 2), one layer up: the same failure, exercised through the
    /// macOS presenter's own `Surface::create_with_proc_address`, which is
    /// what `LayerPresenter::on_mpv_ready` (Task 6) actually calls.
    unsafe extern "C" fn null_get_proc_address(_ctx: *mut c_void, _name: *const c_char) -> *mut c_void {
        std::ptr::null_mut()
    }

    /// Proves the fallback contract Task 9 is about: when mpv's OpenGL
    /// render context can't be created (here, forced by a GL loader that
    /// resolves nothing), `Surface::create` returns `Err` instead of
    /// panicking or crashing, and nothing is leaked (`Surface::drop` runs
    /// on the early return inside `create_with_proc_address`). This is the
    /// exact call `LayerPresenter::on_mpv_ready` makes; on `Err` it logs and
    /// returns without ever touching the `LAYERS` registry, so
    /// `set_viewport`/`set_visible` remain safe no-ops for that presenter id.
    #[test]
    #[ignore = "needs a real libmpv; run with `cargo test -p oneshot-player -- --ignored`"]
    fn surface_create_fails_cleanly_without_a_working_gl_loader() {
        let dirs = [std::path::PathBuf::from("/opt/homebrew/lib"), std::path::PathBuf::from("/usr/local/lib")];
        let api = oneshot_mpv::load(None, &dirs).expect("libmpv must be installed (brew install mpv) to run this test");
        let (mpv, _events) = Mpv::create(api, [("vo", "libmpv"), ("idle", "yes"), ("force-window", "no")]).unwrap();

        let result = Surface::create_with_proc_address(&mpv, null_get_proc_address);

        assert!(result.is_err(), "render context creation must not silently succeed with no working GL loader");

        // `on_mpv_ready` (Task 6) matches on exactly this `Result` and, on
        // `Err`, logs and returns before ever calling `attach` — so a
        // failure here never inserts a `LAYERS` entry, leaving
        // `set_viewport`/`set_visible` (which look the id up in `LAYERS`
        // and no-op if absent) safe for a presenter whose render context
        // never came up. That control flow is a plain `match`/`return`
        // (see `on_mpv_ready` above) with no unsafe code in the error arm,
        // so it needs no separate test beyond this one proving the `Result`
        // it matches on is itself produced without panicking.
    }
}
