//! Profile PINs: a local lock (like a streaming service's profile PIN), not
//! protection against someone with access to the session's files. Hashed
//! with argon2id; failures slow down after five attempts.

use std::time::{Duration, Instant};

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use oneshot_core::profile::{ProfileMode, ProfilesConfig};
use oneshot_core::{Error, Result};

pub fn hash_pin(pin: &str) -> Result<String> {
    if pin.len() != 4 || !pin.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::Invalid("a PIN is 4 digits".into()));
    }
    // uuid v4 is 16 random bytes: a fine salt, and no extra RNG dependency.
    let salt = SaltString::encode_b64(uuid::Uuid::new_v4().as_bytes()).map_err(|e| Error::Other(e.to_string()))?;
    Argon2::default().hash_password(pin.as_bytes(), &salt).map(|h| h.to_string()).map_err(|e| Error::Other(e.to_string()))
}

pub fn verify_pin(hash: &str, pin: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| Argon2::default().verify_password(pin.as_bytes(), &h).is_ok())
}

#[derive(Debug, Default, Clone)]
pub struct PinGuard {
    failures: u32,
    locked_until: Option<Instant>,
}

impl PinGuard {
    /// `Err(wait)` while locked.
    pub fn check(&self, now: Instant) -> std::result::Result<(), Duration> {
        match self.locked_until {
            Some(t) if t > now => Err(t - now),
            _ => Ok(()),
        }
    }

    pub fn fail(&mut self, now: Instant) {
        self.failures += 1;
        let secs = match self.failures {
            0..=4 => return,
            5 => 30,
            6 => 60,
            _ => 300,
        };
        self.locked_until = Some(now + Duration::from_secs(secs));
    }

    pub fn succeed(&mut self) {
        *self = Self::default();
    }
}

/// `hash` is the profile's PIN (none = open). Counts failures in `guard`.
pub fn check_pin(hash: Option<&str>, given: Option<&str>, guard: &mut PinGuard, now: Instant) -> Result<()> {
    let Some(hash) = hash else { return Ok(()) };
    if let Err(wait) = guard.check(now) {
        return Err(Error::PinLocked(wait.as_secs().max(1)));
    }
    if given.is_some_and(|p| verify_pin(hash, p)) {
        guard.succeed();
        return Ok(());
    }
    guard.fail(now);
    Err(match guard.check(now) {
        Err(wait) => Error::PinLocked(wait.as_secs().max(1)),
        Ok(()) => Error::WrongPin,
    })
}

/// Whether turning multi-user off, or changing where profiles come from,
/// would let anyone past the PINs (the implicit profile sees everything).
fn is_sensitive_config_change(config: &ProfilesConfig, enabled: bool, mode: ProfileMode) -> bool {
    config.enabled && (!enabled || mode != config.mode)
}

/// Turning multi-user off, or changing where profiles come from, would let
/// anyone past the PINs (the implicit profile sees everything): it needs the
/// PIN of one protected profile, when there is one.
pub fn authorize_config_change(config: &ProfilesConfig, enabled: bool, mode: ProfileMode, pin: Option<&str>) -> Result<()> {
    let sensitive = is_sensitive_config_change(config, enabled, mode);
    let mut hashes = config.profiles.iter().filter_map(|p| p.pin.as_deref()).peekable();
    if !sensitive || hashes.peek().is_none() {
        return Ok(());
    }
    match pin {
        Some(pin) if hashes.any(|h| verify_pin(h, pin)) => Ok(()),
        _ => Err(Error::WrongPin),
    }
}

/// Like `authorize_config_change`, but counts failures in `guard` so config
/// changes cannot be used to brute-force a profile PIN, and a locked guard
/// does not block a change that needs no PIN at all.
pub fn authorize_config_change_guarded(
    config: &ProfilesConfig,
    enabled: bool,
    mode: ProfileMode,
    pin: Option<&str>,
    guard: &mut PinGuard,
    now: Instant,
) -> Result<()> {
    let sensitive = is_sensitive_config_change(config, enabled, mode);
    let mut hashes = config.profiles.iter().filter_map(|p| p.pin.as_deref()).peekable();
    if !sensitive || hashes.peek().is_none() {
        return Ok(());
    }
    if let Err(wait) = guard.check(now) {
        return Err(Error::PinLocked(wait.as_secs().max(1)));
    }
    let Some(pin) = pin else { return Err(Error::WrongPin) };
    if hashes.any(|h| verify_pin(h, pin)) {
        guard.succeed();
        return Ok(());
    }
    guard.fail(now);
    Err(match guard.check(now) {
        Err(wait) => Error::PinLocked(wait.as_secs().max(1)),
        Ok(()) => Error::WrongPin,
    })
}

#[cfg(test)]
mod tests {
    use oneshot_core::profile::{Origin, Profile};

    use super::*;

    #[test]
    fn pins_hash_and_verify() {
        let h = hash_pin("0427").unwrap();
        assert!(!h.contains("0427"));
        assert!(verify_pin(&h, "0427"));
        assert!(!verify_pin(&h, "0428"));
        assert!(!verify_pin("garbage", "0427"));
    }

    #[test]
    fn only_four_digits_are_pins() {
        for bad in ["", "123", "12345", "12a4", "１２３４"] {
            assert!(hash_pin(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn guard_locks_after_five_failures_then_longer() {
        let t0 = Instant::now();
        let mut g = PinGuard::default();
        for _ in 0..4 {
            g.fail(t0);
        }
        assert!(g.check(t0).is_ok());
        g.fail(t0);
        assert_eq!(g.check(t0), Err(Duration::from_secs(30)));
        assert!(g.check(t0 + Duration::from_secs(31)).is_ok());
        g.fail(t0 + Duration::from_secs(31));
        assert_eq!(g.check(t0 + Duration::from_secs(31)), Err(Duration::from_secs(60)));
        g.fail(t0);
        assert_eq!(g.check(t0), Err(Duration::from_secs(300)));
        g.succeed();
        assert!(g.check(t0).is_ok());
    }

    #[test]
    fn check_pin_reports_wrong_then_locked() {
        let h = hash_pin("1111").unwrap();
        let now = Instant::now();
        let mut g = PinGuard::default();
        assert!(check_pin(None, None, &mut g, now).is_ok(), "no PIN, nothing to check");
        assert!(matches!(check_pin(Some(&h), None, &mut g, now), Err(Error::WrongPin)));
        for _ in 0..3 {
            let _ = check_pin(Some(&h), Some("0000"), &mut g, now);
        }
        assert!(matches!(check_pin(Some(&h), Some("0000"), &mut g, now), Err(Error::PinLocked(30))));
        assert!(matches!(check_pin(Some(&h), Some("1111"), &mut g, now), Err(Error::PinLocked(_))), "even the right PIN waits");
        assert!(check_pin(Some(&h), Some("1111"), &mut g, now + Duration::from_secs(30)).is_ok());
    }

    fn config_with_pin() -> ProfilesConfig {
        let kid = Profile::new("Kid", "#ff6b6b", Origin::Manual, Default::default());
        let mut parent = Profile::new("Parent", "#5e8bff", Origin::Manual, Default::default());
        parent.pin = Some(hash_pin("9876").unwrap());
        ProfilesConfig { enabled: true, profiles: vec![kid, parent], ..Default::default() }
    }

    #[test]
    fn turning_multi_user_off_or_changing_mode_needs_a_pin() {
        let c = config_with_pin();
        assert!(matches!(authorize_config_change(&c, false, c.mode, None), Err(Error::WrongPin)));
        assert!(matches!(authorize_config_change(&c, true, ProfileMode::Local, Some("0000")), Err(Error::WrongPin)));
        assert!(authorize_config_change(&c, false, c.mode, Some("9876")).is_ok());
        assert!(authorize_config_change(&c, true, c.mode, None).is_ok(), "ask-on-startup alone is not guarded");
    }

    #[test]
    fn no_pin_anywhere_means_no_lock() {
        let mut c = config_with_pin();
        c.profiles[1].pin = None;
        assert!(authorize_config_change(&c, false, ProfileMode::Linked, None).is_ok());
        let off = ProfilesConfig::default();
        assert!(authorize_config_change(&off, true, ProfileMode::Local, None).is_ok(), "turning it on is free");
    }

    #[test]
    fn guarded_a_non_sensitive_change_with_junk_pin_does_not_touch_the_guard() {
        let c = config_with_pin();
        let now = Instant::now();
        let mut g = PinGuard::default();
        for _ in 0..4 {
            g.fail(now);
        }
        // ask_on_startup alone is not sensitive: a junk PIN here must not reset the guard.
        assert!(authorize_config_change_guarded(&c, true, c.mode, Some("0000"), &mut g, now).is_ok());
        g.fail(now);
        assert_eq!(g.check(now), Err(Duration::from_secs(30)), "the 4 prior failures were preserved");
    }

    #[test]
    fn guarded_wrong_pin_on_a_sensitive_change_counts_toward_the_lock() {
        let c = config_with_pin();
        let now = Instant::now();
        let mut g = PinGuard::default();
        for _ in 0..4 {
            assert!(matches!(authorize_config_change_guarded(&c, false, c.mode, Some("0000"), &mut g, now), Err(Error::WrongPin)));
        }
        assert!(matches!(authorize_config_change_guarded(&c, false, c.mode, Some("0000"), &mut g, now), Err(Error::PinLocked(30))));
    }

    #[test]
    fn guarded_missing_pin_is_not_an_attempt() {
        let c = config_with_pin();
        let now = Instant::now();
        let mut g = PinGuard::default();
        assert!(matches!(authorize_config_change_guarded(&c, false, c.mode, None, &mut g, now), Err(Error::WrongPin)));
        assert_eq!(g.check(now), Ok(()), "no PIN typed is a prompt, not a failure");
    }

    #[test]
    fn guarded_right_pin_resets_the_guard() {
        let c = config_with_pin();
        let now = Instant::now();
        let mut g = PinGuard::default();
        for _ in 0..3 {
            let _ = authorize_config_change_guarded(&c, false, c.mode, Some("0000"), &mut g, now);
        }
        assert!(authorize_config_change_guarded(&c, false, c.mode, Some("9876"), &mut g, now).is_ok());
        assert_eq!(g.check(now), Ok(()));
        // Confirm it was really reset: three more wrong guesses do not lock yet.
        for _ in 0..3 {
            let _ = authorize_config_change_guarded(&c, false, c.mode, Some("0000"), &mut g, now);
        }
        assert_eq!(g.check(now), Ok(()));
    }

    #[test]
    fn guarded_a_locked_guard_does_not_block_a_non_sensitive_change() {
        let c = config_with_pin();
        let now = Instant::now();
        let mut g = PinGuard::default();
        for _ in 0..5 {
            let _ = authorize_config_change_guarded(&c, false, c.mode, Some("0000"), &mut g, now);
        }
        assert_eq!(g.check(now), Err(Duration::from_secs(30)), "guard is locked");
        assert!(authorize_config_change_guarded(&c, true, c.mode, None, &mut g, now).is_ok(), "ask_on_startup alone is not sensitive");
    }
}
