use std::ffi::{CString, c_char, c_void};
use std::ptr::NonNull;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::event::Event;
use crate::node::{Node, OwnedCNode, cstr_lossy};
use crate::sys::{self, Api, mpv_handle, mpv_node};

struct Inner {
    api: Arc<Api>,
    raw: NonNull<mpv_handle>,
}

// SAFETY: libmpv documents every client API function as thread-safe except
// `mpv_wait_event`, which we only expose through the non-Clone `Events` type.
unsafe impl Send for Inner {}
// SAFETY: see above.
unsafe impl Sync for Inner {}

impl Drop for Inner {
    fn drop(&mut self) {
        // SAFETY: `raw` came from `mpv_create` and this is the last reference.
        unsafe { (self.api.mpv_terminate_destroy)(self.raw.as_ptr()) };
    }
}

/// Command/property side of an mpv instance. Cheap to clone, usable from any thread.
#[derive(Clone)]
pub struct Mpv {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Mpv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mpv").field("raw", &self.inner.raw).finish()
    }
}

/// Exclusive event side of an mpv instance (libmpv allows a single waiter).
pub struct Events {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Events {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Events").finish_non_exhaustive()
    }
}

fn cstring(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| Error::Nul(s.to_owned()))
}

impl Mpv {
    /// Creates, configures (pre-init options) and initialises an mpv core.
    ///
    /// Options set here are the ones that only take effect before
    /// `mpv_initialize` (`vo`, `gpu-context`, `wid`, `d3d11-output-mode`, ...).
    pub fn create<'a>(api: Arc<Api>, options: impl IntoIterator<Item = (&'a str, &'a str)>) -> Result<(Self, Events)> {
        // SAFETY: no preconditions.
        let raw = NonNull::new(unsafe { (api.mpv_create)() }).ok_or(Error::Create)?;
        let inner = Arc::new(Inner { api, raw });
        let mpv = Self { inner: Arc::clone(&inner) };
        for (name, value) in options {
            let (n, v) = (cstring(name)?, cstring(value)?);
            // SAFETY: valid handle and NUL-terminated strings.
            let rc = unsafe { (mpv.api().mpv_set_option_string)(mpv.raw(), n.as_ptr(), v.as_ptr()) };
            mpv.check(rc, || format!("set option {name}={value}"))?;
        }
        // SAFETY: valid, not yet initialised handle.
        let rc = unsafe { (mpv.api().mpv_initialize)(mpv.raw()) };
        mpv.check(rc, || "initialize".to_owned())?;
        Ok((mpv, Events { inner }))
    }

    pub(crate) fn api(&self) -> &Api {
        &self.inner.api
    }

    pub(crate) fn api_arc(&self) -> Arc<Api> {
        Arc::clone(&self.inner.api)
    }

    pub(crate) fn raw(&self) -> *mut mpv_handle {
        self.inner.raw.as_ptr()
    }

    fn check(&self, rc: i32, context: impl FnOnce() -> String) -> Result<()> {
        if rc >= 0 {
            return Ok(());
        }
        // SAFETY: mpv_error_string returns a static string for any code.
        let message = unsafe { cstr_lossy((self.api().mpv_error_string)(rc)) };
        Err(Error::Mpv { code: rc, message, context: context() })
    }

    /// Runs a command synchronously, e.g. `["loadfile", url, "replace"]`.
    pub fn command(&self, args: &[&str]) -> Result<()> {
        let owned = args.iter().map(|a| cstring(a)).collect::<Result<Vec<_>>>()?;
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        // SAFETY: NULL-terminated argv of valid C strings kept alive by `owned`.
        let rc = unsafe { (self.api().mpv_command)(self.raw(), ptrs.as_mut_ptr()) };
        self.check(rc, || format!("command {args:?}"))
    }

    /// Runs a command asynchronously; completion arrives as [`Event::CommandReply`].
    pub fn command_async(&self, reply_id: u64, args: &[&str]) -> Result<()> {
        let owned = args.iter().map(|a| cstring(a)).collect::<Result<Vec<_>>>()?;
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        // SAFETY: as in `command`; mpv copies the arguments before returning.
        let rc = unsafe { (self.api().mpv_command_async)(self.raw(), reply_id, ptrs.as_mut_ptr()) };
        self.check(rc, || format!("command_async {args:?}"))
    }

    pub fn set_property(&self, name: &str, value: impl Into<Node>) -> Result<()> {
        let value = value.into();
        let n = cstring(name)?;
        let mut c = OwnedCNode::new(&value)?;
        // SAFETY: `c.root` is a valid node tree alive for the duration of the call.
        let rc = unsafe {
            (self.api().mpv_set_property)(
                self.raw(),
                n.as_ptr(),
                sys::MPV_FORMAT_NODE,
                (&raw mut c.root).cast::<c_void>(),
            )
        };
        self.check(rc, || format!("set_property {name}={value:?}"))
    }

    pub fn get_property(&self, name: &str) -> Result<Node> {
        let n = cstring(name)?;
        let mut out = mpv_node::empty();
        // SAFETY: `out` is a valid destination for MPV_FORMAT_NODE.
        let rc = unsafe {
            (self.api().mpv_get_property)(self.raw(), n.as_ptr(), sys::MPV_FORMAT_NODE, (&raw mut out).cast())
        };
        self.check(rc, || format!("get_property {name}"))?;
        // SAFETY: mpv filled `out`; we deep-copy then free its allocations.
        let node = unsafe { Node::from_raw(&out) };
        // SAFETY: `out` was produced by mpv_get_property.
        unsafe { (self.api().mpv_free_node_contents)(&raw mut out) };
        Ok(node)
    }

    /// Like [`get_property`](Self::get_property) but maps "unavailable" to `None`.
    pub fn try_get_property(&self, name: &str) -> Result<Option<Node>> {
        match self.get_property(name) {
            Ok(n) => Ok(Some(n)),
            Err(e) if e.is_unavailable() => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Observes a property; changes arrive as [`Event::PropertyChange`] tagged with `id`.
    pub fn observe(&self, id: u64, name: &str) -> Result<()> {
        let n = cstring(name)?;
        // SAFETY: valid handle and string.
        let rc = unsafe { (self.api().mpv_observe_property)(self.raw(), id, n.as_ptr(), sys::MPV_FORMAT_NODE) };
        self.check(rc, || format!("observe {name}"))
    }

    /// Forwards mpv's internal log at `level` ("error", "warn", "info", "v", "debug").
    pub fn request_log_messages(&self, level: &str) -> Result<()> {
        let l = cstring(level)?;
        // SAFETY: valid handle and string.
        let rc = unsafe { (self.api().mpv_request_log_messages)(self.raw(), l.as_ptr()) };
        self.check(rc, || format!("request_log_messages {level}"))
    }

    /// Interrupts a blocking [`Events::wait`] on another thread.
    pub fn wakeup(&self) {
        // SAFETY: valid handle; documented as callable from any thread.
        unsafe { (self.api().mpv_wakeup)(self.raw()) };
    }

    pub fn library_path(&self) -> &std::path::Path {
        &self.api().path
    }
}

impl Events {
    /// Blocks up to `timeout` seconds (negative = forever) for the next event.
    /// Returns `None` on timeout or wakeup.
    pub fn wait(&mut self, timeout: f64) -> Option<Event> {
        let api = &self.inner.api;
        // SAFETY: `&mut self` guarantees a single concurrent waiter per handle.
        let ev = unsafe { (api.mpv_wait_event)(self.inner.raw.as_ptr(), timeout) };
        // SAFETY: mpv_wait_event never returns NULL.
        let ev = unsafe { &mut *ev };
        if ev.event_id == sys::MPV_EVENT_NONE {
            return None;
        }
        let mut node = mpv_node::empty();
        // SAFETY: `ev` is valid until the next wait; we copy before returning.
        unsafe { (api.mpv_event_to_node)(&raw mut node, ev) };
        // SAFETY: filled by mpv_event_to_node.
        let decoded = unsafe { Node::from_raw(&node) };
        // SAFETY: produced by mpv_event_to_node.
        unsafe { (api.mpv_free_node_contents)(&raw mut node) };
        Some(Event::from_node(ev.reply_userdata, decoded))
    }
}
