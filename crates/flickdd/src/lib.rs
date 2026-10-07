//! FlickDD client: downloads a movie or an episode **through Flick Server**,
//! in resumable, throttled segments, so that it can be watched offline.
//!
//! * [`api`]: the REST calls (create a grant, fetch a segment, cancel) and the
//!   failures they can end in.
//! * [`item`]: what is persisted and shown for one download.
//! * [`files`]: file names and the partial file.
//! * [`engine`]: the [`Manager`], which runs the resumable algorithm of
//!   `docs/flickdd-integration.md` for a queue of downloads.
//!
//! Nothing here talks to Jellyfin or Plex: the server does, with the
//! credentials its operator configured.

pub mod api;
pub mod engine;
pub mod files;
pub mod item;

pub use api::Failure;
pub use engine::{Connection, Event, EventSink, Manager, Source};
pub use item::{Backend, Item, Kind, NewDownload, State};
