//! Chromecast: the Cast v2 protocol, spoken directly.
//!
//! TLS (the device's certificate is self-signed, so it is not checked: the
//! address comes from the user's own network) carrying length-prefixed
//! protobuf `CastMessage`s whose payload is JSON. Only the few messages
//! needed to start the Default Media Receiver and drive it are handled.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use oneshot_core::{Error, Result};
use parking_lot::Mutex;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::client::TlsStream;

use crate::{CastMedia, CastState, Receiver, Remote};

const SENDER: &str = "sender-0";
const PLATFORM: &str = "receiver-0";
const NS_CONNECTION: &str = "urn:x-cast:com.google.cast.tp.connection";
const NS_HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
const NS_RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";
const NS_MEDIA: &str = "urn:x-cast:com.google.cast.media";
const DEFAULT_MEDIA_RECEIVER: &str = "CC1AD845";
const STEP_TIMEOUT: Duration = Duration::from_secs(15);

// ---------------------------------------------------------------- wire format

#[derive(Debug, Default, PartialEq)]
struct Message {
    source: String,
    destination: String,
    namespace: String,
    payload: String,
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8 & 0x7F) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn put_str(out: &mut Vec<u8>, field: u8, s: &str) {
    out.push(field << 3 | 2);
    put_varint(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

/// `CastMessage`: protocol_version=0 (1), source (2), destination (3),
/// namespace (4), payload_type=STRING (5), payload_utf8 (6).
fn encode(m: &Message) -> Vec<u8> {
    let mut body = vec![1 << 3, 0];
    put_str(&mut body, 2, &m.source);
    put_str(&mut body, 3, &m.destination);
    put_str(&mut body, 4, &m.namespace);
    body.extend_from_slice(&[5 << 3, 0]);
    put_str(&mut body, 6, &m.payload);
    let mut framed = (body.len() as u32).to_be_bytes().to_vec();
    framed.extend(body);
    framed
}

fn read_varint(b: &[u8], at: &mut usize) -> Option<u64> {
    let (mut v, mut shift) = (0u64, 0);
    loop {
        let byte = *b.get(*at)?;
        *at += 1;
        v |= u64::from(byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

fn decode(b: &[u8]) -> Option<Message> {
    let mut m = Message::default();
    let mut at = 0;
    while at < b.len() {
        let tag = read_varint(b, &mut at)?;
        match (tag >> 3, tag & 7) {
            (field, 2) => {
                let len = read_varint(b, &mut at)? as usize;
                let bytes = b.get(at..at.checked_add(len)?)?;
                at += len;
                let s = String::from_utf8_lossy(bytes).into_owned();
                match field {
                    2 => m.source = s,
                    3 => m.destination = s,
                    4 => m.namespace = s,
                    6 => m.payload = s,
                    _ => {}
                }
            }
            (_, 0) => {
                read_varint(b, &mut at)?;
            }
            (_, 1) => at = at.checked_add(8)?,
            (_, 5) => at = at.checked_add(4)?,
            _ => return None,
        }
    }
    Some(m)
}

// ----------------------------------------------------------------- connection

#[derive(Debug)]
struct AnyCertificate;

impl ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(&self, _: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(&self, _: &[u8], _: &CertificateDer<'_>, _: &DigitallySignedStruct) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(&self, _: &[u8], _: &CertificateDer<'_>, _: &DigitallySignedStruct) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider().signature_verification_algorithms.supported_schemes()
    }
}

struct Wire {
    reader: ReadHalf<TlsStream<TcpStream>>,
    writer: WriteHalf<TlsStream<TcpStream>>,
    request_id: u32,
}

/// Any trouble talking to the Chromecast.
fn cc(e: impl std::fmt::Display) -> Error {
    Error::Network(oneshot_core::codes::CAST_CHROMECAST.tag(format!("The Chromecast connection failed ({e}). Check it is on the same network, restart it, and retry.")))
}

impl Wire {
    async fn connect(addr: SocketAddr) -> Result<Self> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| cc(e))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AnyCertificate))
            .with_no_client_auth();
        let tcp = tokio::time::timeout(STEP_TIMEOUT, TcpStream::connect(addr)).await.map_err(|_| Error::Network(oneshot_core::codes::CAST_CHROMECAST.tag("The Chromecast did not answer. Check it is on the same network, restart it, and retry.")))?.map_err(|e| cc(e))?;
        let name = ServerName::try_from(addr.ip().to_string()).map_err(|e| cc(e))?;
        let tls = tokio_rustls::TlsConnector::from(Arc::new(config)).connect(name, tcp).await.map_err(|e| cc(e))?;
        let (reader, writer) = tokio::io::split(tls);
        Ok(Self { reader, writer, request_id: 0 })
    }

    async fn send(&mut self, destination: &str, namespace: &str, payload: &Value) -> Result<()> {
        let m = Message { source: SENDER.into(), destination: destination.into(), namespace: namespace.into(), payload: payload.to_string() };
        self.writer.write_all(&encode(&m)).await.map_err(|e| cc(e))?;
        self.writer.flush().await.map_err(|e| cc(e))
    }

    /// Sends a request carrying a fresh `requestId`.
    async fn request(&mut self, destination: &str, namespace: &str, mut payload: Value) -> Result<u32> {
        self.request_id += 1;
        payload["requestId"] = json!(self.request_id);
        self.send(destination, namespace, &payload).await?;
        Ok(self.request_id)
    }

    async fn next(&mut self) -> Result<(Message, Value)> {
        loop {
            let mut len = [0u8; 4];
            self.reader.read_exact(&mut len).await.map_err(|e| cc(e))?;
            let len = u32::from_be_bytes(len) as usize;
            if len > 1 << 20 {
                return Err(Error::Protocol(oneshot_core::codes::CAST_PROTOCOL.tag("The Chromecast sent something Flick cannot read. Restart the Chromecast.")));
            }
            let mut body = vec![0u8; len];
            self.reader.read_exact(&mut body).await.map_err(|e| cc(e))?;
            let Some(m) = decode(&body) else { continue };
            let json: Value = serde_json::from_str(&m.payload).unwrap_or(Value::Null);
            if m.namespace == NS_HEARTBEAT && json["type"] == "PING" {
                self.send(&m.source, NS_HEARTBEAT, &json!({"type": "PONG"})).await?;
                continue;
            }
            if m.namespace == NS_CONNECTION && json["type"] == "CLOSE" {
                return Err(Error::Network(oneshot_core::codes::CAST_CHROMECAST.tag("The Chromecast closed the session.")));
            }
            return Ok((m, json));
        }
    }

    /// The first message `pick` accepts.
    async fn wait<T>(&mut self, what: &str, mut pick: impl FnMut(&Message, &Value) -> Option<Result<T>>) -> Result<T> {
        let waiting = async {
            loop {
                let (m, json) = self.next().await?;
                if let Some(found) = pick(&m, &json) {
                    return found;
                }
            }
        };
        tokio::time::timeout(STEP_TIMEOUT, waiting).await.map_err(|_| Error::Network(oneshot_core::codes::CAST_CHROMECAST.tag(format!("The Chromecast did not {what} in time. Restart it and retry."))))?
    }
}

// --------------------------------------------------------------------- state

fn media_remote(status: &Value, previous: &Remote) -> Remote {
    let mut r = previous.clone();
    r.error = None;
    let player = status["playerState"].as_str().unwrap_or("");
    r.state = match player {
        "PLAYING" => CastState::Playing,
        "PAUSED" => CastState::Paused,
        "BUFFERING" => CastState::Buffering,
        "IDLE" => match status["idleReason"].as_str() {
            Some("FINISHED") => CastState::Ended,
            Some("ERROR") => {
                r.error = Some("the Chromecast could not play this stream".into());
                CastState::Error
            }
            // Idle before it has loaded anything, or after being told to stop.
            Some(_) => CastState::Idle,
            None => CastState::Loading,
        },
        _ => r.state,
    };
    if let Some(t) = status["currentTime"].as_f64() {
        r.position_ms = (t.max(0.0) * 1000.0) as u64;
    }
    if let Some(d) = status["media"]["duration"].as_f64() {
        r.duration_ms = Some((d.max(0.0) * 1000.0) as u64);
    }
    r
}

enum Cmd {
    Send { destination: String, namespace: &'static str, payload: Value },
    Stop,
}

pub(crate) struct Chromecast {
    commands: mpsc::UnboundedSender<Cmd>,
    remote: Arc<Mutex<Remote>>,
    transport: String,
    media_session: Arc<Mutex<Option<i64>>>,
}

impl Chromecast {
    /// Connects, launches the Default Media Receiver and loads `media` from `url`.
    pub async fn start(addr: SocketAddr, url: &url::Url, media: &CastMedia) -> Result<Self> {
        let mut wire = Wire::connect(addr).await?;
        wire.send(PLATFORM, NS_CONNECTION, &json!({"type": "CONNECT"})).await?;
        wire.request(PLATFORM, NS_RECEIVER, json!({"type": "LAUNCH", "appId": DEFAULT_MEDIA_RECEIVER})).await?;
        let (transport, session) = wire
            .wait("start its media player", |m, json| {
                if m.namespace != NS_RECEIVER {
                    return None;
                }
                if json["type"] == "LAUNCH_ERROR" {
                    return Some(Err(Error::Playback(oneshot_core::codes::CAST_LOAD.tag(format!("The Chromecast refused to start its player ({}). Restart it and retry.", json["reason"].as_str().unwrap_or("unknown"))))));
                }
                let app = json["status"]["applications"].as_array()?.iter().find(|a| a["appId"] == DEFAULT_MEDIA_RECEIVER)?;
                Some(Ok((app["transportId"].as_str()?.to_owned(), app["sessionId"].as_str()?.to_owned())))
            })
            .await?;
        wire.send(&transport, NS_CONNECTION, &json!({"type": "CONNECT"})).await?;
        let load = json!({
            "type": "LOAD",
            "sessionId": session,
            "autoplay": true,
            "currentTime": media.start_ms as f64 / 1000.0,
            "media": {
                "contentId": url.as_str(),
                "contentType": media.content_type,
                "streamType": "BUFFERED",
                "metadata": {"metadataType": 0, "title": media.title},
            },
        });
        let load_id = wire.request(&transport, NS_MEDIA, load).await?;
        let (media_session, first) = wire
            .wait("load the stream", |m, json| {
                if m.namespace != NS_MEDIA || json["requestId"] != load_id && json["type"] != "MEDIA_STATUS" {
                    return None;
                }
                match json["type"].as_str()? {
                    "LOAD_FAILED" | "INVALID_REQUEST" => Some(Err(Error::Playback(oneshot_core::codes::CAST_LOAD.tag("The Chromecast could not load this stream. Try another version of the title.")))),
                    "MEDIA_STATUS" => {
                        let s = json["status"].as_array()?.first()?;
                        Some(Ok((s["mediaSessionId"].as_i64()?, s.clone())))
                    }
                    _ => None,
                }
            })
            .await?;

        let initial = Remote { duration_ms: media.duration_ms, position_ms: media.start_ms, ..Default::default() };
        let remote = Arc::new(Mutex::new(media_remote(&first, &initial)));
        let media_session = Arc::new(Mutex::new(Some(media_session)));
        let (commands, rx) = mpsc::unbounded_channel();
        tokio::spawn(run(wire, rx, transport.clone(), session, Arc::clone(&remote), Arc::clone(&media_session)));
        Ok(Self { commands, remote, transport, media_session })
    }

    fn media_command(&self, mut payload: Value) -> Result<()> {
        let id = (*self.media_session.lock()).ok_or_else(|| Error::Playback(oneshot_core::codes::CAST_NOTHING.tag("Nothing is loaded on the Chromecast.")))?;
        payload["mediaSessionId"] = json!(id);
        self.commands.send(Cmd::Send { destination: self.transport.clone(), namespace: NS_MEDIA, payload }).map_err(|_| Error::Network(oneshot_core::codes::CAST_CHROMECAST.tag("The Chromecast connection is closed. Pick the device again.")))
    }
}

/// Keeps the connection alive and the state fresh until told to stop.
async fn run(mut wire: Wire, mut commands: mpsc::UnboundedReceiver<Cmd>, transport: String, session: String, remote: Arc<Mutex<Remote>>, media_session: Arc<Mutex<Option<i64>>>) {
    let mut ping = tokio::time::interval(Duration::from_secs(5));
    let mut poll = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = ping.tick() => {
                if wire.send(PLATFORM, NS_HEARTBEAT, &json!({"type": "PING"})).await.is_err() { break }
            }
            _ = poll.tick() => {
                if wire.request(&transport, NS_MEDIA, json!({"type": "GET_STATUS"})).await.is_err() { break }
            }
            cmd = commands.recv() => match cmd {
                Some(Cmd::Send { destination, namespace, payload }) => {
                    if wire.request(&destination, namespace, payload).await.is_err() { break }
                }
                Some(Cmd::Stop) | None => break,
            },
            msg = wire.next() => match msg {
                Ok((m, json)) if m.namespace == NS_MEDIA && json["type"] == "MEDIA_STATUS" => {
                    if let Some(s) = json["status"].as_array().and_then(|a| a.first()) {
                        if let Some(id) = s["mediaSessionId"].as_i64() { *media_session.lock() = Some(id); }
                        let mut r = remote.lock();
                        *r = media_remote(s, &r);
                    }
                }
                Ok((m, json)) if m.namespace == NS_RECEIVER && json["type"] == "RECEIVER_STATUS" => {
                    if let Some(level) = json["status"]["volume"]["level"].as_f64() {
                        remote.lock().volume = Some(level as f32);
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    let mut r = remote.lock();
                    r.state = CastState::Error;
                    r.error = Some(e.to_string());
                    return;
                }
            },
        }
    }
    // Leaving the receiver's app returns the TV to its home screen.
    let _ = wire.send(PLATFORM, NS_RECEIVER, &json!({"type": "STOP", "requestId": 9999, "sessionId": session})).await;
}

#[async_trait]
impl Receiver for Chromecast {
    async fn pause(&self) -> Result<()> {
        self.media_command(json!({"type": "PAUSE"}))
    }

    async fn resume(&self) -> Result<()> {
        self.media_command(json!({"type": "PLAY"}))
    }

    async fn seek(&self, ms: u64) -> Result<()> {
        self.remote.lock().position_ms = ms;
        self.media_command(json!({"type": "SEEK", "currentTime": ms as f64 / 1000.0, "resumeState": "PLAYBACK_START"}))
    }

    async fn set_volume(&self, volume: f32) -> Result<()> {
        self.remote.lock().volume = Some(volume);
        self.commands
            .send(Cmd::Send { destination: PLATFORM.into(), namespace: NS_RECEIVER, payload: json!({"type": "SET_VOLUME", "volume": {"level": volume}}) })
            .map_err(|_| Error::Network(oneshot_core::codes::CAST_CHROMECAST.tag("The Chromecast connection is closed. Pick the device again.")))
    }

    async fn status(&self) -> Remote {
        self.remote.lock().clone()
    }

    async fn stop(&self) {
        let _ = self.commands.send(Cmd::Stop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip_through_the_wire_format() {
        let m = Message { source: "sender-0".into(), destination: "receiver-0".into(), namespace: NS_CONNECTION.into(), payload: r#"{"type":"CONNECT"}"#.into() };
        let framed = encode(&m);
        let len = u32::from_be_bytes(framed[..4].try_into().unwrap()) as usize;
        assert_eq!(len, framed.len() - 4);
        assert_eq!(decode(&framed[4..]), Some(m));
    }

    #[test]
    fn long_payloads_use_multi_byte_lengths() {
        let m = Message { source: "a".into(), destination: "b".into(), namespace: "c".into(), payload: "x".repeat(300) };
        assert_eq!(decode(&encode(&m)[4..]).unwrap().payload.len(), 300);
    }

    #[test]
    fn status_maps_player_states() {
        let before = Remote { duration_ms: Some(5000), ..Default::default() };
        let playing = media_remote(&json!({"playerState": "PLAYING", "currentTime": 12.5, "media": {"duration": 3600.0}}), &before);
        assert_eq!((playing.state, playing.position_ms, playing.duration_ms), (CastState::Playing, 12_500, Some(3_600_000)));
        let ended = media_remote(&json!({"playerState": "IDLE", "idleReason": "FINISHED"}), &playing);
        assert_eq!(ended.state, CastState::Ended);
        let failed = media_remote(&json!({"playerState": "IDLE", "idleReason": "ERROR"}), &playing);
        assert_eq!(failed.state, CastState::Error);
        assert!(failed.error.is_some());
        assert_eq!(media_remote(&json!({"playerState": "IDLE"}), &before).state, CastState::Loading);
    }
}
