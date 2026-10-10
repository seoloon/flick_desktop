//! AirPlay: the video-URL protocol (`/play`, `/rate`, `/scrub`,
//! `/playback-info`, `/stop`) with binary plists, over one HTTP connection.
//!
//! Receivers that ask for a PIN are paired once ([`begin_pairing`] shows the
//! PIN on the screen, [`finish_pairing`] checks it and keeps the credentials);
//! after that every connection starts with `pair-verify` and the rest of the
//! conversation is encrypted ([`crate::link`]).
//!
//! The receiver fetches the stream itself.

use std::net::SocketAddr;

use async_trait::async_trait;
use oneshot_core::codes;
use oneshot_core::{Error, Result};
use parking_lot::Mutex;
use plist::{Dictionary, Value};

use crate::hap::{Credentials, PairError, PairSetup, PairVerify};
use crate::link::{Link, LinkError, Response};
use crate::{CastMedia, CastState, Receiver, Remote};

const USER_AGENT: &str = "MediaControl/1.0";
const PAIRING_AGENT: &str = "AirPlay/320.20";
const TLV: &str = "application/octet-stream";

pub(crate) struct AirPlay {
    link: tokio::sync::Mutex<Link>,
    session: String,
    last: Mutex<Remote>,
    /// A stream that was playing and whose info has gone: it ended.
    seen_playing: Mutex<bool>,
}

/// The receiver cannot be reached, or stopped answering.
fn lost(why: impl std::fmt::Display) -> Error {
    tracing::debug!(target: "cast", "AirPlay link: {why}");
    Error::Network(codes::CAST_AIRPLAY_LOST.tag("The AirPlay device could not be reached or stopped answering. Check it is on and on the same network as this computer."))
}

fn link_err(e: LinkError) -> Error {
    match e {
        LinkError::Io(_) | LinkError::Timeout => lost(e),
        LinkError::Malformed => Error::Protocol(codes::CAST_PROTOCOL.tag("The AirPlay device answered with something Flick does not understand. Restart the device and retry.")),
        LinkError::Crypto => Error::Protocol(codes::CAST_AIRPLAY_CHANNEL.tag("The encrypted connection with the AirPlay device broke. Retry; if it persists, pair the device again.")),
    }
}

fn pin_wanted() -> Error {
    Error::Forbidden(codes::AUTH_AIRPLAY_PAIRING.tag("This AirPlay device asks for a PIN. Enter the code shown on the TV."))
}

fn pair_error(e: PairError) -> Error {
    match e {
        PairError::WrongPin => Error::Forbidden(codes::AUTH_AIRPLAY_WRONG_PIN.tag("That PIN is not the one shown on the TV. Try again.")),
        PairError::Backoff => Error::Forbidden(codes::CAST_PAIR_WAIT.tag("The AirPlay device asks you to wait before trying another PIN. Try again in a minute.")),
        PairError::Busy => Error::Forbidden(codes::CAST_PAIR_WAIT.tag("The AirPlay device is busy with another pairing. Try again in a moment.")),
        PairError::Unexpected(why) => {
            tracing::debug!(target: "cast", "AirPlay pairing: unexpected answer ({why})");
            Error::Protocol(codes::CAST_PAIR_FAILED.tag("The AirPlay device answered the pairing in a way Flick does not understand. Retry; if it persists, remove Flick from the device's paired remotes."))
        }
        PairError::Proof(why) => {
            tracing::debug!(target: "cast", "AirPlay pairing: proof failed ({why})");
            Error::Protocol(codes::CAST_PAIR_FAILED.tag("The pairing with the AirPlay device could not be verified. Retry; if it persists, remove Flick from the device's paired remotes and pair again."))
        }
        PairError::Device(code) => Error::Protocol(codes::CAST_PAIR_FAILED.tag(format!("The AirPlay device refused the pairing (error {code})."))),
    }
}

/// One TLV step of pairing: `POST`, and the answer's body.
async fn tlv_step(link: &mut Link, path: &str, body: &[u8]) -> Result<Vec<u8>> {
    let r = link.request("POST", path, &[("Content-Type", TLV), ("User-Agent", PAIRING_AGENT), ("X-Apple-HKP", "3"), ("Connection", "keep-alive")], body).await.map_err(link_err)?;
    tracing::debug!(target: "cast", path, status = r.status, bytes = r.body.len(), "AirPlay pairing step");
    match r.status {
        200 => Ok(r.body),
        470 | 401 | 403 => Err(pin_wanted()),
        // A TLV error rides on a 200 normally; some receivers use the status.
        s => Err(Error::Protocol(codes::CAST_PAIR_FAILED.tag(format!("The AirPlay device refused a pairing step (HTTP {s}).")))),
    }
}

/// Asks the receiver to show its PIN; the connection is kept for [`finish_pairing`].
pub(crate) async fn begin_pairing(addr: SocketAddr) -> Result<Link> {
    let mut link = Link::connect(addr).await.map_err(link_err)?;
    let r = link.request("POST", "/pair-pin-start", &[("User-Agent", PAIRING_AGENT), ("Connection", "keep-alive")], b"").await.map_err(link_err)?;
    if r.status != 200 {
        return Err(Error::Protocol(codes::CAST_PAIR_FAILED.tag(format!("The AirPlay device did not show a PIN (HTTP {}). It may not need one: try casting directly.", r.status))));
    }
    Ok(link)
}

/// Checks the PIN the person read on the screen; the credentials to keep.
pub(crate) async fn finish_pairing(mut link: Link, pin: &str) -> Result<Credentials> {
    // Mid-pairing, a refusal is not a request for the PIN: the person just typed it.
    let refused = |e: Error| if e.code() == codes::AUTH_AIRPLAY_PAIRING.id { Error::Protocol(codes::CAST_PAIR_FAILED.tag("The AirPlay device refused the pairing. Ask it for a new code and retry.")) } else { e };
    let mut setup = PairSetup::new(pin);
    let m2 = tlv_step(&mut link, "/pair-setup", &setup.m1(false)).await.map_err(refused)?;
    let m3 = setup.m3(&m2).map_err(pair_error)?;
    let m4 = tlv_step(&mut link, "/pair-setup", &m3).await.map_err(refused)?;
    let m5 = setup.m5(&m4, false).map_err(pair_error)?.ok_or_else(|| pair_error(PairError::Unexpected("a transient pairing")))?;
    let m6 = tlv_step(&mut link, "/pair-setup", &m5).await.map_err(refused)?;
    setup.finish(&m6).map_err(pair_error)
}

/// A connection to the receiver, verified with `creds`, and encrypted from then on.
async fn verified_link(addr: SocketAddr, creds: &Credentials) -> Result<Link> {
    let mut link = Link::connect(addr).await.map_err(link_err)?;
    let mut verify = PairVerify::new(creds.clone());
    let m2 = tlv_step(&mut link, "/pair-verify", &verify.m1()).await?;
    let m3 = verify.m3(&m2).map_err(|e| match e {
        // The receiver no longer knows us (reset, or another device): pair again.
        PairError::Unexpected(_) | PairError::Proof(_) | PairError::WrongPin | PairError::Device(_) => pin_wanted(),
        other => pair_error(other),
    })?;
    let m4 = tlv_step(&mut link, "/pair-verify", &m3).await?;
    let keys = verify.finish(&m4).map_err(|_| pin_wanted())?;
    link.encrypt_with(&keys);
    Ok(link)
}

/// The `/play` body.
fn play_body(url: &str, start_fraction: f64) -> Result<Vec<u8>> {
    let mut d = Dictionary::new();
    d.insert("Content-Location".into(), Value::String(url.into()));
    d.insert("Start-Position".into(), Value::Real(start_fraction.clamp(0.0, 1.0)));
    let mut out = Vec::new();
    plist::to_writer_binary(&mut out, &Value::Dictionary(d)).map_err(|e| Error::Protocol(codes::CAST_PROTOCOL.tag(format!("Flick could not prepare the stream for AirPlay ({e})."))))?;
    Ok(out)
}

/// `/playback-info`: what the receiver is doing, from its plist.
fn parse_info(body: &[u8], previous: &Remote, seen_playing: bool) -> Remote {
    let mut r = previous.clone();
    r.error = None;
    let Ok(Value::Dictionary(info)) = Value::from_reader(std::io::Cursor::new(body)) else { return r };
    let real = |k: &str| info.get(k).and_then(|v| v.as_real().or_else(|| v.as_signed_integer().map(|i| i as f64)));
    match real("duration") {
        Some(duration) => {
            r.duration_ms = Some((duration.max(0.0) * 1000.0) as u64);
            r.position_ms = (real("position").unwrap_or(0.0).max(0.0) * 1000.0) as u64;
            let ready = info.get("readyToPlay").and_then(Value::as_boolean).unwrap_or(true);
            r.state = match real("rate") {
                Some(rate) if rate > 0.0 => CastState::Playing,
                Some(_) if ready => CastState::Paused,
                _ => CastState::Buffering,
            };
        }
        // No duration: nothing is loaded any more.
        None => r.state = if seen_playing { CastState::Ended } else { CastState::Loading },
    }
    r
}

/// `POST /play` on an open link; the receiver then fetches `url`.
async fn send_play(link: &mut Link, session: &str, url: &url::Url, fraction: f64, encrypted: bool) -> Result<()> {
    let resp = link
        .request("POST", "/play", &[("Content-Type", "application/x-apple-binary-plist"), ("X-Apple-Session-ID", session), ("User-Agent", USER_AGENT)], &play_body(url.as_str(), fraction)?)
        .await
        .map_err(link_err)?;
    tracing::info!(target: "cast", status = resp.status, encrypted, location = %url, "AirPlay /play answered");
    if !(200..300).contains(&resp.status) {
        // What the device says explains a refusal (an error plist, or text).
        let said: String = String::from_utf8_lossy(&resp.body).chars().filter(|c| !c.is_control() || *c == ' ').take(300).collect();
        tracing::warn!(target: "cast", status = resp.status, "AirPlay /play refused: {said}");
    }
    match resp.status {
        s if (200..300).contains(&s) => Ok(()),
        // 470: "connection authorization required".
        401 | 403 | 470 => Err(pin_wanted()),
        s => Err(Error::Playback(codes::CAST_AIRPLAY.tag(format!("The AirPlay device refused the stream (HTTP {s}).")))),
    }
}

impl AirPlay {
    /// Connects (verified, when `creds` are known) and starts the stream.
    pub async fn start(addr: SocketAddr, url: &url::Url, media: &CastMedia, creds: Option<&Credentials>) -> Result<Self> {
        let mut link = match creds {
            Some(c) => verified_link(addr, c).await?,
            None => Link::connect(addr).await.map_err(link_err)?,
        };
        let session = uuid::Uuid::new_v4().to_string();
        let fraction = match media.duration_ms {
            Some(d) if d > 0 => media.start_ms as f64 / d as f64,
            _ => 0.0,
        };
        send_play(&mut link, &session, url, fraction, creds.is_some()).await?;
        let remote = Remote { state: CastState::Loading, position_ms: media.start_ms, duration_ms: media.duration_ms, ..Default::default() };
        Ok(Self { link: tokio::sync::Mutex::new(link), session, last: Mutex::new(remote), seen_playing: Mutex::new(false) })
    }

    /// Plays another stream on the same connection (the conversion restarted somewhere else).
    pub async fn load(&self, url: &url::Url, start_ms: u64, duration_ms: Option<u64>) -> Result<()> {
        let fraction = match duration_ms {
            Some(d) if d > 0 => start_ms as f64 / d as f64,
            _ => 0.0,
        };
        send_play(&mut *self.link.lock().await, &self.session, url, fraction, true).await?;
        *self.seen_playing.lock() = false;
        *self.last.lock() = Remote { state: CastState::Loading, position_ms: start_ms, duration_ms, ..Default::default() };
        Ok(())
    }

    async fn call(&self, method: &str, path: &str) -> Result<Response> {
        self.link.lock().await.request(method, path, &[("X-Apple-Session-ID", &self.session), ("User-Agent", USER_AGENT)], b"").await.map_err(link_err)
    }

    async fn post(&self, path: &str) -> Result<()> {
        let resp = self.call("POST", path).await?;
        (200..300).contains(&resp.status).then_some(()).ok_or_else(|| Error::Playback(codes::CAST_AIRPLAY.tag(format!("The AirPlay device answered an error (HTTP {}).", resp.status))))
    }
}

#[async_trait]
impl Receiver for AirPlay {
    async fn pause(&self) -> Result<()> {
        self.post("/rate?value=0.000000").await
    }

    async fn resume(&self) -> Result<()> {
        self.post("/rate?value=1.000000").await
    }

    async fn seek(&self, ms: u64) -> Result<()> {
        self.last.lock().position_ms = ms;
        self.post(&format!("/scrub?position={:.3}", ms as f64 / 1000.0)).await
    }

    async fn set_volume(&self, volume: f32) -> Result<()> {
        self.last.lock().volume = Some(volume);
        self.post(&format!("/volume?volume={volume:.3}")).await
    }

    async fn status(&self) -> Remote {
        let previous = self.last.lock().clone();
        let next = match self.call("GET", "/playback-info").await {
            Ok(r) if (200..300).contains(&r.status) => parse_info(&r.body, &previous, *self.seen_playing.lock()),
            // Gone after having played: the receiver let go of the session.
            Ok(r) if r.status == 404 && *self.seen_playing.lock() => Remote { state: CastState::Ended, ..previous },
            Ok(_) => previous,
            Err(e) => Remote { state: CastState::Error, error: Some(format!("{e} ({})", e.code())), ..previous },
        };
        if next.state == CastState::Playing {
            *self.seen_playing.lock() = true;
        }
        *self.last.lock() = next.clone();
        next
    }

    async fn stop(&self) {
        let _ = self.post("/stop").await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(pairs: &[(&str, Value)]) -> Vec<u8> {
        let mut d = Dictionary::new();
        for (k, v) in pairs {
            d.insert((*k).into(), v.clone());
        }
        let mut out = Vec::new();
        plist::to_writer_xml(&mut out, &Value::Dictionary(d)).unwrap();
        out
    }

    #[test]
    fn play_body_is_a_binary_plist_with_the_location_and_start() {
        let body = play_body("http://192.168.1.2:5000/c/t/a.m3u8", 0.25).unwrap();
        assert!(body.starts_with(b"bplist00"));
        let Value::Dictionary(d) = Value::from_reader(std::io::Cursor::new(body)).unwrap() else { panic!("not a dictionary") };
        assert_eq!(d.get("Content-Location").and_then(Value::as_string), Some("http://192.168.1.2:5000/c/t/a.m3u8"));
        assert_eq!(d.get("Start-Position").and_then(Value::as_real), Some(0.25));
    }

    #[test]
    fn playback_info_gives_state_and_position() {
        let prev = Remote::default();
        let playing = parse_info(&info(&[("duration", Value::Real(100.0)), ("position", Value::Real(12.5)), ("rate", Value::Real(1.0))]), &prev, false);
        assert_eq!((playing.state, playing.position_ms, playing.duration_ms), (CastState::Playing, 12_500, Some(100_000)));
        let paused = parse_info(&info(&[("duration", Value::Real(100.0)), ("position", Value::Real(12.5)), ("rate", Value::Real(0.0)), ("readyToPlay", Value::Boolean(true))]), &playing, true);
        assert_eq!(paused.state, CastState::Paused);
        // An empty answer after playing: it is over; before playing: still loading.
        assert_eq!(parse_info(&info(&[]), &paused, true).state, CastState::Ended);
        assert_eq!(parse_info(&info(&[]), &prev, false).state, CastState::Loading);
    }
}
