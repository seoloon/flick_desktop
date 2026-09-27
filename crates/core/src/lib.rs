//! Flick domain core: the provider-agnostic model shared by every crate.
//!
//! Nothing here performs I/O. Providers (Jellyfin, Plex) translate their APIs
//! into these types; the UI and the playback pipeline only ever consume them.

pub mod capabilities;
pub mod error;
pub mod ids;
pub mod media;
pub mod playback;
pub mod provider;
pub mod query;
pub mod server;
pub mod settings;
pub mod stream;

pub use error::{Error, Result};
pub use ids::{ItemRef, ServerId};
