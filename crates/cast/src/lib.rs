//! Casting: sends what is playing to a Chromecast or an AirPlay receiver.
//!
//! The receiver pulls the stream itself, so the app hands it a URL on a small
//! relay of its own ([`proxy`]): the server's address and credentials stay
//! here, and the receiver sees a plain LAN URL.

mod airplay;
mod chromecast;
mod discovery;
pub mod hap;
mod link;
pub mod proxy;
pub mod transcode;

use std::net::{IpAddr, SocketAddr, UdpSocket};

use async_trait::async_trait;
use oneshot_core::{Error, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use url::Url;

pub use discovery::Discovery;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum CastKind {
    Chromecast,
    AirPlay,
}

/// A receiver found on the network.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CastDevice {
    pub id: String,
    pub name: String,
    pub kind: CastKind,
    pub model: Option<String>,
    pub host: String,
    pub port: u16,
}

impl CastDevice {
    fn addr(&self) -> Result<SocketAddr> {
        let ip: IpAddr = self.host.parse().map_err(|_| Error::Invalid(oneshot_core::codes::CAST_ADDRESS.tag(format!("The address of the device is not valid ({}). Refresh the device list.", self.host))))?;
        Ok(SocketAddr::new(ip, self.port))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum CastState {
    /// Nothing is being cast.
    #[default]
    Idle,
    Loading,
    Playing,
    Paused,
    Buffering,
    /// The title reached its end.
    Ended,
    Error,
}

/// What the receiver reports.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CastStatus {
    pub device: Option<CastDevice>,
    pub state: CastState,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    /// 0..=1, when the receiver tells.
    pub volume: Option<f32>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "type")]
pub enum CastCommand {
    Pause,
    Resume,
    Seek {
        ms: u64,
    },
    /// 0..=1.
    SetVolume {
        volume: f32,
    },
}

/// What to play.
#[derive(Debug, Clone)]
pub struct CastMedia {
    pub url: Url,
    /// Sent to the server by the relay, never to the receiver.
    pub headers: Vec<(String, String)>,
    pub content_type: String,
    pub title: String,
    pub start_ms: u64,
    pub duration_ms: Option<u64>,
    /// Set when the original file is not playable as it is: the app converts it for the receiver.
    pub convert: Option<transcode::Convert>,
    /// With `convert`: the text subtitle to burn in, as a file on the server (in the title's folder).
    pub subtitle_file: Option<Url>,
}

/// What the receiver last said, as the protocols give it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Remote {
    pub state: CastState,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: Option<f32>,
    pub error: Option<String>,
}

#[async_trait]
pub(crate) trait Receiver: Send + Sync {
    async fn pause(&self) -> Result<()>;
    async fn resume(&self) -> Result<()>;
    async fn seek(&self, ms: u64) -> Result<()>;
    async fn set_volume(&self, volume: f32) -> Result<()>;
    async fn status(&self) -> Remote;
    async fn stop(&self);
}

struct Active {
    device: CastDevice,
    receiver: Box<dyn Receiver>,
}

/// Finds receivers and drives the one the user picked.
pub struct Caster {
    discovery: Discovery,
    proxy: proxy::Proxy,
    active: tokio::sync::Mutex<Option<Active>>,
    last: Mutex<CastStatus>,
    /// Where pairings are kept between runs.
    store: Option<std::sync::Arc<dyn PairingStore>>,
    /// A pairing waiting for its PIN: the device and the open connection.
    pairing: tokio::sync::Mutex<Option<(String, link::Link)>>,
    /// Where ffmpeg is looked for after `ONESHOT_FFMPEG`.
    ffmpeg_dirs: Vec<std::path::PathBuf>,
}

/// Where the credentials of paired receivers live (the app keeps them in its credential vault).
pub trait PairingStore: Send + Sync {
    fn load(&self, device_id: &str) -> Option<String>;
    fn save(&self, device_id: &str, credentials: &str) -> Result<()>;
    fn forget(&self, device_id: &str);
}

impl std::fmt::Debug for Caster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Caster").finish_non_exhaustive()
    }
}

impl Default for Caster {
    fn default() -> Self {
        Self::new()
    }
}

impl Caster {
    pub fn new() -> Self {
        Self { discovery: Discovery::default(), proxy: proxy::Proxy::default(), active: tokio::sync::Mutex::new(None), last: Mutex::new(CastStatus::default()), store: None, pairing: tokio::sync::Mutex::new(None), ffmpeg_dirs: Vec::new() }
    }

    /// Where ffmpeg is looked for (the folders libmpv is searched in), after `ONESHOT_FFMPEG`.
    pub fn with_ffmpeg_dirs(mut self, dirs: Vec<std::path::PathBuf>) -> Self {
        self.ffmpeg_dirs = dirs;
        self
    }

    /// Whether the ffmpeg found can draw a text subtitle into the picture (it needs libass).
    pub fn can_burn_text_subtitles(&self) -> bool {
        transcode::locate(&self.ffmpeg_dirs).is_some_and(|f| transcode::has_filter(&f, "subtitles"))
    }

    /// Whether ffmpeg is there to convert a title.
    pub fn can_convert(&self) -> bool {
        transcode::locate(&self.ffmpeg_dirs).is_some()
    }

    /// Remembers pairings in `store`.
    pub fn with_store(store: std::sync::Arc<dyn PairingStore>) -> Self {
        Self { store: Some(store), ..Self::new() }
    }

    /// Asks an AirPlay receiver to show its PIN. [`Caster::pair_finish`] follows.
    pub async fn pair_begin(&self, device_id: &str) -> Result<()> {
        let device = self.discovery.find(device_id).ok_or_else(|| Error::NotFound(oneshot_core::codes::CAST_DEVICE_GONE.tag("That device is no longer on the network. Check it is on, then pick it again.")))?;
        let link = airplay::begin_pairing(device.addr()?).await?;
        *self.pairing.lock().await = Some((device.id, link));
        Ok(())
    }

    /// Checks the PIN read on the screen and remembers the pairing.
    pub async fn pair_finish(&self, device_id: &str, pin: &str) -> Result<()> {
        let (id, link) = self.pairing.lock().await.take().filter(|(id, _)| id == device_id).ok_or_else(|| Error::Invalid(oneshot_core::codes::CAST_PAIR_FAILED.tag("The PIN request has expired. Ask the device for a new code.")))?;
        let creds = airplay::finish_pairing(link, pin.trim()).await?;
        let store = self.store.as_ref().ok_or_else(|| Error::Storage(oneshot_core::codes::STO_SECRET_SAVE.tag("There is nowhere to keep the pairing.")))?;
        store.save(&id, &creds.encode())
    }

    /// Receivers seen so far. The first call starts looking (nothing touches
    /// the network until the user opens the cast menu).
    pub fn devices(&self) -> Vec<CastDevice> {
        self.discovery.devices()
    }

    pub fn is_casting(&self) -> bool {
        self.last.lock().device.is_some()
    }

    /// Starts casting `media` to `device_id`, replacing what was cast before.
    pub async fn start(&self, device_id: &str, media: CastMedia, http: reqwest::Client) -> Result<()> {
        let device = self.discovery.find(device_id).ok_or_else(|| Error::NotFound(oneshot_core::codes::CAST_DEVICE_GONE.tag("That device is no longer on the network. Check it is on, then pick it again.")))?;
        self.stop().await;
        let addr = device.addr()?;
        let local = local_address_towards(addr)?;
        let relayed = if media.convert.is_none() { Some(self.proxy.serve(&media.url, media.headers.clone(), local, http.clone()).await?) } else { None };
        *self.last.lock() = CastStatus { device: Some(device.clone()), state: CastState::Loading, position_ms: media.start_ms, duration_ms: media.duration_ms, ..Default::default() };
        let loaded = match device.kind {
            CastKind::Chromecast => match relayed.as_ref() {
                Some(relayed) => chromecast::Chromecast::start(addr, relayed, &media).await.map(|r| Box::new(r) as Box<dyn Receiver>),
                None => Err(Error::Invalid(oneshot_core::codes::CAST_OTHER.tag("A Chromecast does not take a converted stream."))),
            },
            CastKind::AirPlay => {
                let creds = self.store.as_ref().and_then(|s| s.load(&device.id)).and_then(|c| hap::Credentials::decode(&c));
                let started = match (&media.convert, &relayed) {
                    (Some(convert), _) => self.start_converted(addr, local, convert.clone(), &media, http, creds.as_ref()).await,
                    (None, Some(relayed)) => airplay::AirPlay::start(addr, relayed, &media, creds.as_ref()).await.map(|r| Box::new(r) as Box<dyn Receiver>),
                    (None, None) => unreachable!("a direct cast always has its relay"),
                };
                // A pairing the receiver no longer honours is forgotten: the PIN is asked again.
                if started.as_ref().is_err_and(|e| e.code() == oneshot_core::codes::AUTH_AIRPLAY_PAIRING.id)
                    && creds.is_some()
                    && let Some(store) = &self.store
                {
                    store.forget(&device.id);
                }
                started
            }
        };
        match loaded {
            Ok(receiver) => {
                *self.active.lock().await = Some(Active { device, receiver });
                Ok(())
            }
            Err(e) => {
                *self.last.lock() = CastStatus::default();
                self.proxy.stop();
                Err(e)
            }
        }
    }

    /// Converts the title with ffmpeg and plays the result on the receiver.
    async fn start_converted(&self, addr: SocketAddr, local: IpAddr, convert: transcode::Convert, media: &CastMedia, http: reqwest::Client, creds: Option<&hap::Credentials>) -> Result<Box<dyn Receiver>> {
        let ffmpeg = transcode::locate(&self.ffmpeg_dirs).ok_or_else(transcode::missing_error)?;
        let session = transcode::Session::start(ffmpeg, convert, &media.url, media.subtitle_file.as_ref(), media.headers.clone(), local, http, media.start_ms).await?;
        // The receiver's own timeline begins where the conversion does.
        let hls = CastMedia { url: session.playlist_url(), content_type: "application/x-mpegURL".into(), start_ms: 0, duration_ms: None, convert: None, subtitle_file: None, headers: Vec::new(), title: media.title.clone() };
        let airplay = airplay::AirPlay::start(addr, &hls.url, &hls, creds).await?;
        Ok(Box::new(transcode::Converting::new(airplay, session, media.duration_ms)))
    }

    pub async fn command(&self, command: CastCommand) -> Result<()> {
        let active = self.active.lock().await;
        let a = active.as_ref().ok_or_else(|| Error::Invalid(oneshot_core::codes::CAST_NOTHING.tag("Nothing is being cast.")))?;
        match command {
            CastCommand::Pause => a.receiver.pause().await,
            CastCommand::Resume => a.receiver.resume().await,
            CastCommand::Seek { ms } => a.receiver.seek(ms).await,
            CastCommand::SetVolume { volume } => a.receiver.set_volume(volume.clamp(0.0, 1.0)).await,
        }
    }

    /// The receiver's state now (asks it when the protocol has no push).
    pub async fn status(&self) -> CastStatus {
        let active = self.active.lock().await;
        let Some(a) = active.as_ref() else { return self.last.lock().clone() };
        let r = a.receiver.status().await;
        let status = CastStatus { device: Some(a.device.clone()), state: r.state, position_ms: r.position_ms, duration_ms: r.duration_ms, volume: r.volume, error: r.error };
        *self.last.lock() = status.clone();
        status
    }

    /// Ends the cast and returns where it was, to carry on locally.
    pub async fn stop(&self) -> Option<u64> {
        let active = self.active.lock().await.take();
        let position = if let Some(a) = active {
            let p = a.receiver.status().await.position_ms;
            a.receiver.stop().await;
            Some(p)
        } else {
            None
        };
        self.proxy.stop();
        *self.last.lock() = CastStatus::default();
        position
    }
}

/// The address of this machine on the network that reaches `device`.
fn local_address_towards(device: SocketAddr) -> Result<IpAddr> {
    let socket = UdpSocket::bind(if device.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }).map_err(|e| Error::Network(oneshot_core::codes::CAST_OTHER.tag(format!("Flick could not find this computer's address on the local network ({e})."))))?;
    socket.connect(device).map_err(|e| Error::Network(oneshot_core::codes::CAST_OTHER.tag(format!("Flick could not find this computer's address on the local network ({e})."))))?;
    Ok(socket.local_addr().map_err(|e| Error::Network(oneshot_core::codes::CAST_OTHER.tag(format!("Flick could not find this computer's address on the local network ({e})."))))?.ip())
}
