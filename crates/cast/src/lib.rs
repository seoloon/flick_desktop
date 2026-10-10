//! Casting: sends what is playing to a Chromecast or an AirPlay receiver.
//!
//! The receiver pulls the stream itself, so the app hands it a URL on a small
//! relay of its own ([`proxy`]): the server's address and credentials stay
//! here, and the receiver sees a plain LAN URL.

mod airplay;
mod chromecast;
mod discovery;
pub mod proxy;

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
        Self { discovery: Discovery::default(), proxy: proxy::Proxy::default(), active: tokio::sync::Mutex::new(None), last: Mutex::new(CastStatus::default()) }
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
        let relayed = self.proxy.serve(&media.url, media.headers.clone(), local, http).await?;
        *self.last.lock() = CastStatus { device: Some(device.clone()), state: CastState::Loading, position_ms: media.start_ms, duration_ms: media.duration_ms, ..Default::default() };
        let loaded = match device.kind {
            CastKind::Chromecast => chromecast::Chromecast::start(addr, &relayed, &media).await.map(|r| Box::new(r) as Box<dyn Receiver>),
            CastKind::AirPlay => airplay::AirPlay::start(addr, &relayed, &media).await.map(|r| Box::new(r) as Box<dyn Receiver>),
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
