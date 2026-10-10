//! AirPlay: the video-URL protocol (`/play`, `/rate`, `/scrub`,
//! `/playback-info`, `/stop`) over plain HTTP, with binary plists.
//!
//! The receiver fetches the stream itself. Receivers that insist on pairing
//! (a PIN or an Apple ID check) refuse the request: that is reported, there is
//! no pairing here.

use std::net::SocketAddr;
use std::time::Duration;

use async_trait::async_trait;
use oneshot_core::{Error, Result};
use parking_lot::Mutex;
use plist::{Dictionary, Value};
use reqwest::{Client, StatusCode};

use crate::{CastMedia, CastState, Receiver, Remote};

const USER_AGENT: &str = "MediaControl/1.0";

pub(crate) struct AirPlay {
    http: Client,
    base: String,
    session: String,
    last: Mutex<Remote>,
    /// A stream that was playing and whose info has gone: it ended.
    seen_playing: Mutex<bool>,
}

fn net(e: reqwest::Error) -> Error {
    Error::Network(oneshot_core::codes::CAST_AIRPLAY.tag(format!("The AirPlay device could not be reached ({e}). Check it is on the same network.")))
}

/// The `/play` body.
fn play_body(url: &str, start_fraction: f64) -> Result<Vec<u8>> {
    let mut d = Dictionary::new();
    d.insert("Content-Location".into(), Value::String(url.into()));
    d.insert("Start-Position".into(), Value::Real(start_fraction.clamp(0.0, 1.0)));
    let mut out = Vec::new();
    plist::to_writer_binary(&mut out, &Value::Dictionary(d)).map_err(|e| Error::Protocol(oneshot_core::codes::CAST_PROTOCOL.tag(format!("Flick could not prepare the stream for AirPlay ({e})."))))?;
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

impl AirPlay {
    pub async fn start(addr: SocketAddr, url: &url::Url, media: &CastMedia) -> Result<Self> {
        let http = Client::builder().timeout(Duration::from_secs(10)).user_agent(USER_AGENT).build().map_err(net)?;
        let session = uuid::Uuid::new_v4().to_string();
        let base = format!("http://{addr}");
        let fraction = match media.duration_ms {
            Some(d) if d > 0 => media.start_ms as f64 / d as f64,
            _ => 0.0,
        };
        let resp = http
            .post(format!("{base}/play"))
            .header("Content-Type", "application/x-apple-binary-plist")
            .header("X-Apple-Session-ID", &session)
            .body(play_body(url.as_str(), fraction)?)
            .send()
            .await
            .map_err(net)?;
        match resp.status() {
            s if s.is_success() => {}
            // 470: "connection authorization required".
            s if s == StatusCode::UNAUTHORIZED || s == StatusCode::FORBIDDEN || s.as_u16() == 470 => {
                return Err(Error::Forbidden(oneshot_core::codes::AUTH_AIRPLAY_PAIRING.tag("This AirPlay device asks for pairing, which Flick does not support yet. Set it to accept anyone on the same network, or use a Chromecast.")));
            }
            s => return Err(Error::Playback(oneshot_core::codes::CAST_AIRPLAY.tag(format!("The AirPlay device refused the stream (HTTP {}).", s.as_u16())))),
        }
        let remote = Remote { state: CastState::Loading, position_ms: media.start_ms, duration_ms: media.duration_ms, ..Default::default() };
        Ok(Self { http, base, session, last: Mutex::new(remote), seen_playing: Mutex::new(false) })
    }

    async fn post(&self, path: &str) -> Result<()> {
        let resp = self.http.post(format!("{}{path}", self.base)).header("X-Apple-Session-ID", &self.session).send().await.map_err(net)?;
        resp.status().is_success().then_some(()).ok_or_else(|| Error::Playback(oneshot_core::codes::CAST_AIRPLAY.tag(format!("The AirPlay device answered an error (HTTP {}).", resp.status().as_u16()))))
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
        let fetched = self.http.get(format!("{}/playback-info", self.base)).header("X-Apple-Session-ID", &self.session).send().await;
        let next = match fetched {
            Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                Ok(body) => parse_info(&body, &previous, *self.seen_playing.lock()),
                Err(_) => previous,
            },
            // Gone after having played: the receiver let go of the session.
            Ok(resp) if resp.status() == StatusCode::NOT_FOUND && *self.seen_playing.lock() => Remote { state: CastState::Ended, ..previous },
            Ok(_) => previous,
            Err(e) => Remote { state: CastState::Error, error: Some(format!("AirPlay: {e}")), ..previous },
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
