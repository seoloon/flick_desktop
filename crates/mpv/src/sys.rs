//! Raw libmpv ABI (client API 2.x), resolved at runtime with `libloading`.
//!
//! Only the subset of `client.h` we actually use is declared. Struct layouts
//! mirror the C definitions exactly; everything else is decoded through
//! `mpv_event_to_node`, which keeps us independent of per-event struct layouts.

#![allow(non_camel_case_types, missing_debug_implementations)]

use std::ffi::{c_char, c_double, c_int, c_ulong, c_void};
use std::path::{Path, PathBuf};

use libloading::Library;

use crate::error::{Error, Result};

pub enum mpv_handle {}

pub type mpv_format = c_int;
pub const MPV_FORMAT_NONE: mpv_format = 0;
pub const MPV_FORMAT_STRING: mpv_format = 1;
pub const MPV_FORMAT_FLAG: mpv_format = 3;
pub const MPV_FORMAT_INT64: mpv_format = 4;
pub const MPV_FORMAT_DOUBLE: mpv_format = 5;
pub const MPV_FORMAT_NODE: mpv_format = 6;
pub const MPV_FORMAT_NODE_ARRAY: mpv_format = 7;
pub const MPV_FORMAT_NODE_MAP: mpv_format = 8;
pub const MPV_FORMAT_BYTE_ARRAY: mpv_format = 9;

pub type mpv_event_id = c_int;
pub const MPV_EVENT_NONE: mpv_event_id = 0;

#[repr(C)]
#[derive(Clone, Copy)]
pub union mpv_node_u {
    pub string: *mut c_char,
    pub flag: c_int,
    pub int64: i64,
    pub double_: c_double,
    pub list: *mut mpv_node_list,
    pub ba: *mut mpv_byte_array,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct mpv_node {
    pub u: mpv_node_u,
    pub format: mpv_format,
}

impl mpv_node {
    pub const fn empty() -> Self {
        Self { u: mpv_node_u { int64: 0 }, format: MPV_FORMAT_NONE }
    }
}

#[repr(C)]
pub struct mpv_node_list {
    pub num: c_int,
    pub values: *mut mpv_node,
    pub keys: *mut *mut c_char,
}

#[repr(C)]
pub struct mpv_byte_array {
    pub data: *mut c_void,
    pub size: usize,
}

#[repr(C)]
pub struct mpv_event {
    pub event_id: mpv_event_id,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

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

/// Minimum client API we require: 2.2 (mpv 0.38) introduced nothing we need
/// beyond 2.0, but older builds lack `d3d11-output-mode=composition` and the
/// modern `target-colorspace-hint` behaviour, which we probe separately.
pub const MIN_API_MAJOR: c_ulong = 2;

macro_rules! mpv_api {
    ($( $name:ident : fn($($arg:ty),*) $(-> $ret:ty)? ; )*) => {
        /// Function table resolved from the libmpv shared library.
        pub struct Api {
            _lib: Library,
            pub path: PathBuf,
            $( pub $name: unsafe extern "C" fn($($arg),*) $(-> $ret)?, )*
        }

        impl Api {
            fn from_library(lib: Library, path: PathBuf) -> Result<Self> {
                // SAFETY: every symbol is looked up with the exact signature
                // declared in libmpv's client.h (API 2.x). The function
                // pointers are copied out and stay valid because `_lib` is
                // stored alongside them and never unloaded before `Api` drops.
                unsafe {
                    $(
                        let $name = *lib
                            .get::<unsafe extern "C" fn($($arg),*) $(-> $ret)?>(
                                concat!(stringify!($name), "\0").as_bytes(),
                            )
                            .map_err(|e| Error::MissingSymbol(stringify!($name), e.to_string()))?;
                    )*
                    Ok(Self { _lib: lib, path, $( $name, )* })
                }
            }
        }
    };
}

mpv_api! {
    mpv_client_api_version: fn() -> c_ulong;
    mpv_error_string: fn(c_int) -> *const c_char;
    mpv_free: fn(*mut c_void);
    mpv_create: fn() -> *mut mpv_handle;
    mpv_initialize: fn(*mut mpv_handle) -> c_int;
    mpv_terminate_destroy: fn(*mut mpv_handle);
    mpv_set_option_string: fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int;
    mpv_command: fn(*mut mpv_handle, *mut *const c_char) -> c_int;
    mpv_command_async: fn(*mut mpv_handle, u64, *mut *const c_char) -> c_int;
    mpv_set_property: fn(*mut mpv_handle, *const c_char, mpv_format, *mut c_void) -> c_int;
    mpv_set_property_string: fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int;
    mpv_get_property: fn(*mut mpv_handle, *const c_char, mpv_format, *mut c_void) -> c_int;
    mpv_free_node_contents: fn(*mut mpv_node);
    mpv_observe_property: fn(*mut mpv_handle, u64, *const c_char, mpv_format) -> c_int;
    mpv_request_log_messages: fn(*mut mpv_handle, *const c_char) -> c_int;
    mpv_wait_event: fn(*mut mpv_handle, c_double) -> *mut mpv_event;
    mpv_wakeup: fn(*mut mpv_handle);
    mpv_event_to_node: fn(*mut mpv_node, *mut mpv_event) -> c_int;
    mpv_render_context_create: fn(*mut *mut mpv_render_context, *mut mpv_handle, *mut mpv_render_param) -> c_int;
    mpv_render_context_render: fn(*mut mpv_render_context, *mut mpv_render_param) -> c_int;
    mpv_render_context_report_swap: fn(*mut mpv_render_context);
    mpv_render_context_set_update_callback: fn(*mut mpv_render_context, mpv_render_update_fn, *mut c_void);
    mpv_render_context_free: fn(*mut mpv_render_context);
}

/// Platform file names tried when no explicit path is configured.
pub fn default_library_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["libmpv-2.dll", "mpv-2.dll", "mpv-1.dll"]
    } else if cfg!(target_os = "macos") {
        &["libmpv.2.dylib", "libmpv.dylib"]
    } else {
        &["libmpv.so.2", "libmpv.so"]
    }
}

impl Api {
    /// Loads libmpv from the first candidate that exists and exposes a
    /// compatible client API.
    pub fn load(candidates: &[PathBuf]) -> Result<Self> {
        let mut tried = Vec::new();
        for path in candidates {
            match Self::load_one(path) {
                Ok(api) => return Ok(api),
                Err(e) => tried.push(format!("{}: {e}", path.display())),
            }
        }
        Err(Error::LibraryNotFound(tried))
    }

    fn load_one(path: &Path) -> Result<Self> {
        // SAFETY: loading libmpv runs its static initialisers, which have no
        // preconditions. We only ever load libmpv builds through this path.
        let lib = unsafe { Library::new(path) }.map_err(|e| Error::Load(e.to_string()))?;
        let api = Self::from_library(lib, path.to_path_buf())?;
        // SAFETY: symbol resolved with the documented signature; no arguments.
        let version = unsafe { (api.mpv_client_api_version)() };
        if version >> 16 != MIN_API_MAJOR {
            return Err(Error::IncompatibleApi { found: version });
        }
        Ok(api)
    }

    pub fn client_api_version(&self) -> (u32, u32) {
        // SAFETY: no preconditions. `c_ulong` is 32-bit on Windows, 64-bit elsewhere.
        let v = u64::from(unsafe { (self.mpv_client_api_version)() });
        ((v >> 16) as u32, (v & 0xffff) as u32)
    }
}
