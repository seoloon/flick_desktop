//! Plex provider: plex.tv PIN authentication, resource discovery with
//! connection ranking, and a PMS client mapped onto the common model.
//!
//! Known platform constraints (documented, not worked around):
//! * remote playback of personal media requires Plex Pass or Remote Watch
//!   Pass on the account or the server owner (enforcement for third-party
//!   clients announced for 2026) — surfaced as the server's decision text;
//! * no per-user favourites on library items: Flick's favourites use the
//!   account's plex.tv Watchlist instead (`watchlist`), matched to the
//!   library by guid.

mod admin;
pub mod auth;
mod dto;
mod map;
mod playback;
mod provider;
pub mod watchlist;

pub use auth::{DiscoveredServer, HomeMember, PinChallenge, PlexAccount, PlexAuth, PlexIdentity};
pub use playback::profile_extra;
pub use provider::PlexProvider;
pub use watchlist::{Watchlist, WatchlistEntry};
