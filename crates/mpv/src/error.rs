use std::ffi::c_ulong;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("libmpv could not be loaded; tried: {0:?}")]
    LibraryNotFound(Vec<String>),
    #[error("failed to load library: {0}")]
    Load(String),
    #[error("libmpv is missing symbol `{0}`: {1}")]
    MissingSymbol(&'static str, String),
    #[error("incompatible libmpv client API version {}.{} (need 2.x)", .found >> 16, .found & 0xffff)]
    IncompatibleApi { found: c_ulong },
    #[error("mpv_create failed (out of memory or invalid locale)")]
    Create,
    #[error("mpv error {code} ({message}) during `{context}`")]
    Mpv { code: i32, message: String, context: String },
    #[error("string contains an interior NUL byte: {0:?}")]
    Nul(String),
    #[error("unexpected property format for `{0}`")]
    Format(String),
}

impl Error {
    /// mpv error code, when this error originated from libmpv.
    pub fn mpv_code(&self) -> Option<i32> {
        match self {
            Self::Mpv { code, .. } => Some(*code),
            _ => None,
        }
    }

    /// `MPV_ERROR_PROPERTY_UNAVAILABLE` (-10): the property exists but has no
    /// value right now (e.g. `hwdec-current` before a decoder is loaded).
    pub fn is_unavailable(&self) -> bool {
        self.mpv_code() == Some(-10)
    }
}
