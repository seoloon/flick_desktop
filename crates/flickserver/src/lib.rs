//! Flick Server: one server behind one invitation link.
//!
//! The link carries the server's address and the key its clients sign in with.
//! Everything the server offers (watch-together rooms with FlickSync, downloads
//! with FlickDD) is reached through that one address and signed with that one
//! key, so neither feature owns it: it lives here.
//!
//! * [`invite`]: parsing of `flickserver://` links.
//! * [`key`]: the signing key and the short-lived JWTs minted from it.

pub mod invite;
pub mod key;

pub use invite::{Invitation, InviteError};
pub use key::{Identity, KeyError, SigningKey, mint_token, sanitize_user_id};
