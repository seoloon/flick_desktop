//! FlickSync client for Flick Desktop: watch-together rooms.
//!
//! * [`protocol`]: wire types, parsing and validation of untrusted server data.
//! * [`sync`]: the deterministic synchronization engine (no I/O, injectable clock).
//! * [`room`]: the one canonical client-side room state.
//! * [`connection`] / [`auth`]: REST, WebSocket, back-off, token providers.
//! * [`client`]: ties them together and drives a [`client::PlaybackController`].
//!
//! FlickSync coordinates; Flick plays. Nothing here opens a media file.

pub mod auth;
pub mod client;
pub mod clock;
pub mod connection;
pub mod diagnose;
pub mod errors;
pub mod invite;
pub mod protocol;
pub mod room;
pub mod sync;

pub use client::{ClientEvent, DebugInfo, EventSink, FlickSyncClient, LoadError, PlaybackController};
pub use errors::{Error, Result, UserMessage};
