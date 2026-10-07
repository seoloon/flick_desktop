//! FlickSync authentication.
//!
//! FlickSync has no accounts: it trusts a short-lived JWT signed with the key
//! of an invitation link (see [`crate::invite`]). The client asks a
//! [`TokenProvider`] for a fresh token before every (re)connection;
//! [`LocalKeyTokenProvider`] signs it itself. The key lives in the OS keychain
//! (Rust side only).

use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::Sha256;

use crate::errors::{Error, Result};

#[async_trait]
pub trait TokenProvider: Send + Sync {
    /// A token valid for at least a few minutes. Never cached by the caller.
    async fn token(&self) -> Result<String>;
}

/// The identity a token is minted for.
#[derive(Debug, Clone)]
pub struct Identity {
    /// Becomes the participant id (`[A-Za-z0-9._:@-]`, at most 128 characters).
    pub user_id: String,
    pub display_name: String,
}

/// Keeps only the characters the server accepts in `sub`.
pub fn sanitize_user_id(s: &str) -> String {
    let cleaned: String = s.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '@' | '-')).take(128).collect();
    if cleaned.is_empty() { "flick".into() } else { cleaned }
}

/// `kid:server_id:secret`, the same shape FlickSync's `FLICKSYNC_AUTH_KEYS` uses.
#[derive(Clone)]
pub struct SigningKey {
    pub kid: String,
    pub server_id: String,
    secret: String,
}

impl std::fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SigningKey").field("kid", &self.kid).field("server_id", &self.server_id).finish_non_exhaustive()
    }
}

/// What FlickSync accepts in a `kid` or `server_id`.
fn valid_ident(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':' | b'@'))
}

/// Shortest secret a FlickSync server accepts.
const MIN_SECRET_LEN: usize = 32;

impl SigningKey {
    /// Parses `kid:server_id:secret` under the server's own rules (the secret is
    /// everything after the second colon).
    pub fn parse(s: &str) -> Result<Self> {
        let mut parts = s.trim().splitn(3, ':');
        let (Some(kid), Some(server_id), Some(secret)) = (parts.next(), parts.next(), parts.next()) else {
            return Err(Error::NotConfigured);
        };
        if !valid_ident(kid) || !valid_ident(server_id) || secret.len() < MIN_SECRET_LEN {
            return Err(Error::NotConfigured);
        }
        Ok(Self { kid: kid.into(), server_id: server_id.into(), secret: secret.into() })
    }

    /// `kid:server_id:secret`, to store or encode. A secret: never log it.
    pub(crate) fn expose(&self) -> String {
        format!("{}:{}:{}", self.kid, self.server_id, self.secret)
    }
}

fn now_unix() -> u64 {
    // Wall clock on purpose: a JWT `exp` is a wall-clock claim. Playback maths never use it.
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// HS256 JWT with the claims FlickSync requires.
pub fn mint_token(key: &SigningKey, who: &Identity, ttl_secs: u64, now: u64) -> String {
    let header = json!({ "alg": "HS256", "typ": "JWT", "kid": key.kid });
    let claims = json!({
        "sub": sanitize_user_id(&who.user_id),
        "server_id": key.server_id,
        "aud": "flicksync",
        "name": who.display_name.chars().take(64).collect::<String>(),
        "perms": ["rooms:create", "rooms:join", "chat:send", "downloads:create"],
        "iat": now,
        "exp": now + ttl_secs,
    });
    let enc = |v: &serde_json::Value| URL_SAFE_NO_PAD.encode(v.to_string());
    let signing_input = format!("{}.{}", enc(&header), enc(&claims));
    let mut mac = Hmac::<Sha256>::new_from_slice(key.secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(signing_input.as_bytes());
    format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> SigningKey {
        SigningKey::parse("main:my-flick:0123456789abcdef0123456789abcdef").unwrap()
    }

    #[test]
    fn key_parsing_requires_all_parts_and_a_real_secret() {
        assert!(SigningKey::parse("main:my-flick").is_err());
        assert!(SigningKey::parse("main:my-flick:short").is_err());
        assert!(SigningKey::parse("ma in:my-flick:0123456789abcdef0123456789abcdef").is_err(), "space in the kid");
        let k = SigningKey::parse("main:srv:secret:with:colons:0123456789abcdef0123456789").unwrap();
        assert_eq!(k.server_id, "srv");
        assert!(!format!("{k:?}").contains("0123456789"), "the secret must not be printable");
    }

    #[test]
    fn minted_token_has_the_claims_flicksync_requires() {
        let who = Identity { user_id: "alice".into(), display_name: "Alice".into() };
        let t = mint_token(&key(), &who, 3600, 1_790_000_000);
        let parts: Vec<_> = t.split('.').collect();
        assert_eq!(parts.len(), 3);
        let header: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        let claims: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(header["alg"], "HS256");
        assert_eq!(header["kid"], "main");
        assert_eq!(claims["sub"], "alice");
        assert_eq!(claims["server_id"], "my-flick");
        assert_eq!(claims["aud"], "flicksync");
        assert_eq!(claims["exp"], 1_790_003_600u64);
        assert!(claims["perms"].as_array().unwrap().iter().any(|p| p == "rooms:create"));
    }

    #[test]
    fn signature_is_standard_hmac_sha256() {
        // RFC 7515-style check: recompute independently.
        let who = Identity { user_id: "bob".into(), display_name: "Bob".into() };
        let t = mint_token(&key(), &who, 60, 1);
        let (input, sig) = t.rsplit_once('.').unwrap();
        let mut mac = Hmac::<Sha256>::new_from_slice(b"0123456789abcdef0123456789abcdef").unwrap();
        mac.update(input.as_bytes());
        assert_eq!(sig, URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()));
    }

    #[test]
    fn user_ids_are_sanitized() {
        assert_eq!(sanitize_user_id("a b/c<d>"), "abcd");
        assert_eq!(sanitize_user_id("///"), "flick");
        assert_eq!(sanitize_user_id(&"x".repeat(300)).len(), 128);
    }
}
