//! Safe bindings to the libmpv client API, loaded at runtime.
//!
//! Runtime loading (instead of link-time) lets the application start, report
//! a precise diagnostic and fall back gracefully when libmpv is missing or too
//! old, and lets packagers ship an LGPL libmpv next to the executable.

mod error;
mod event;
mod handle;
mod node;
mod render;
pub mod sys;

use std::path::PathBuf;
use std::sync::Arc;

pub use error::{Error, Result};
pub use event::{EndReason, Event};
pub use handle::{Events, Mpv};
pub use node::Node;
pub use render::RenderContext;
pub use sys::Api;

/// Loads libmpv, trying `explicit` first, then `search_dirs`, then the system
/// loader path with platform default names.
pub fn load(explicit: Option<PathBuf>, search_dirs: &[PathBuf]) -> Result<Arc<Api>> {
    let names = sys::default_library_names();
    let mut candidates: Vec<PathBuf> = explicit.into_iter().collect();
    for dir in search_dirs {
        candidates.extend(names.iter().map(|n| dir.join(n)));
    }
    candidates.extend(names.iter().map(PathBuf::from));
    let api = Api::load(&candidates)?;
    let (major, minor) = api.client_api_version();
    tracing::info!(target: "mpv", path = %api.path.display(), major, minor, "libmpv loaded");
    Ok(Arc::new(api))
}
