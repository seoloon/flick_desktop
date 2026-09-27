//! Plex provider: plex.tv PIN authentication, resource discovery with
//! connection ranking, and a PMS client mapped onto the common model.
//!
//! Known platform constraints (documented, not worked around):
//! * remote playback of personal media requires Plex Pass or Remote Watch
//!   Pass on the account or the server owner (enforcement for third-party
//!   clients announced for 2026) — surfaced as the server's decision text;
//! * no per-user favourites on library items → `Error::Unsupported`.

mod admin;
pub mod auth;
mod dto;
mod map;
mod playback;
mod provider;

pub use auth::{DiscoveredServer, PinChallenge, PlexAccount, PlexAuth, PlexIdentity};
pub use playback::profile_extra;
pub use provider::PlexProvider;
