//! Flick domain core: the provider-agnostic model shared by every crate.
//!
//! Nothing here performs I/O. Providers (Jellyfin, Plex) translate their APIs
//! into these types; the UI and the playback pipeline only ever consume them.

pub mod capabilities;
pub mod codes;
pub mod error;
pub mod ids;
pub mod media;
pub mod person;
pub mod playback;
pub mod profile;
pub mod provider;
pub mod query;
pub mod server;
pub mod settings;
pub mod stream;
pub mod text;

pub use error::{Error, Result, WithCode};
pub use ids::{ItemRef, ServerId};
