//! Jellyfin provider (server ≥ 10.9, API routes introduced by 10.9 are used:
//! `/UserViews`, `/UserItems/Resume`, `/UserPlayedItems`, `/Items/{id}?userId=`).

mod admin;
pub mod auth;
mod dto;
mod map;
mod playback;
mod profile;
mod provider;

pub use auth::{ClientIdentity, Connector, Session};
pub use profile::device_profile;
pub use provider::JellyfinProvider;
