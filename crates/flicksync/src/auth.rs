//! FlickSync authentication.
//!
//! FlickSync has no accounts: it trusts a short-lived JWT signed with the key
//! of the Flick Server invitation link (see `oneshot_flickserver`). The client asks a
//! [`TokenProvider`] for a fresh token before every (re)connection;
//! [`LocalKeyTokenProvider`] signs it itself. The key lives in the OS keychain
//! (Rust side only).

use async_trait::async_trait;

pub use oneshot_flickserver::key::{Identity, SigningKey, mint_token, now_unix, sanitize_user_id};

use crate::errors::Result;

#[async_trait]
pub trait TokenProvider: Send + Sync {
    /// A token valid for at least a few minutes. Never cached by the caller.
    async fn token(&self) -> Result<String>;
}

#[derive(Debug)]
pub struct LocalKeyTokenProvider {
    key: SigningKey,
    who: Identity,
}

impl LocalKeyTokenProvider {
    pub fn new(key: SigningKey, who: Identity) -> Self {
        Self { key, who }
    }
}

#[async_trait]
impl TokenProvider for LocalKeyTokenProvider {
    async fn token(&self) -> Result<String> {
        Ok(mint_token(&self.key, &self.who, 3600, now_unix()))
    }
}
