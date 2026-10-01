//! Networking: REST room calls, WebSocket connection, reconnection back-off.
//! No room or playback logic lives here.

use std::hash::{BuildHasher, Hasher};
use std::time::Duration;

use oneshot_net::reqwest::{Client, StatusCode};
use serde::Deserialize;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use url::Url;

use crate::errors::{Error, Result};
use crate::protocol::ControlMode;

pub type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Back-off between reconnection attempts: 0.5 s, 1 s, 2 s, 4 s, 8 s, then
/// every 10 s, each ± 20 % so a restarted server is not hit by everybody at once.
pub fn backoff_delay(attempt: u32, jitter: f64) -> Duration {
    const STEPS_MS: [f64; 5] = [500.0, 1000.0, 2000.0, 4000.0, 8000.0];
    let base = STEPS_MS.get(attempt as usize).copied().unwrap_or(10_000.0);
    let factor = 1.0 + (jitter.clamp(0.0, 1.0) - 0.5) * 0.4;
    Duration::from_millis((base * factor) as u64)
}

/// Stop retrying after this long without a connection: the server dropped
/// our seat (30 s grace) well before, so the UI offers "rejoin" instead.
pub const GIVE_UP_AFTER: Duration = Duration::from_secs(60);

/// Cheap jitter in `[0, 1)` without pulling a RNG crate.
pub fn jitter() -> f64 {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u8(0);
    (h.finish() >> 11) as f64 / (1u64 << 53) as f64
}

/// `https://sync.example.com` + `/api/v1/rooms/X/ws` → `wss://sync.example.com/api/v1/rooms/X/ws`.
/// `ws_path` comes from the server, so it is only accepted as a plain
/// absolute path under `/api/v1/rooms/`: never another host or scheme.
pub fn ws_url(base: &Url, ws_path: &str) -> Result<Url> {
    if !ws_path.starts_with("/api/v1/rooms/")
        || ws_path.contains("//")
        || ws_path.contains("..")
        || ws_path.contains('?')
        || ws_path.contains('#')
    {
        return Err(Error::Protocol("unexpected websocket path".into()));
    }
    let mut url = base.clone();
    let scheme = match base.scheme() {
        "https" | "wss" => "wss",
        "http" | "ws" => "ws",
        _ => return Err(Error::NotConfigured),
    };
    url.set_scheme(scheme).map_err(|()| Error::NotConfigured)?;
    url.set_path(ws_path);
    url.set_query(None);
    Ok(url)
}

#[derive(Debug, Clone, Deserialize)]
pub struct JoinedRoom {
    pub room_id: String,
    #[serde(default)]
    pub share_code: String,
    pub participant_id: String,
    pub ws_path: String,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    code: String,
}

async fn error_from(resp: oneshot_net::reqwest::Response) -> Error {
    let status = resp.status();
    let code = resp.json::<ErrorBody>().await.map(|b| b.error.code).ok();
    match (status, code) {
        (StatusCode::UNAUTHORIZED, _) => Error::Unauthenticated,
        (_, Some(code)) => Error::Server { code },
        (StatusCode::NOT_FOUND, None) => Error::Server { code: "ROOM_NOT_FOUND".into() },
        (s, None) if s.is_server_error() => Error::Network(format!("FlickSync answered {}", s.as_u16())),
        (s, None) => Error::Server { code: format!("HTTP_{}", s.as_u16()) },
    }
}

fn net(e: oneshot_net::reqwest::Error) -> Error {
    // `without_url`: the room id and tokens must not end up in logs.
    Error::Network(e.without_url().to_string())
}

pub async fn create_room(http: &Client, base: &Url, token: &str, control_mode: ControlMode, chat: bool) -> Result<JoinedRoom> {
    let url = base.join("api/v1/rooms").map_err(|_| Error::NotConfigured)?;
    let body = serde_json::json!({ "control_mode": control_mode, "chat_enabled": chat });
    let resp = http.post(url).bearer_auth(token).json(&body).send().await.map_err(net)?;
    if !resp.status().is_success() {
        return Err(error_from(resp).await);
    }
    resp.json().await.map_err(|_| Error::Protocol("malformed create response".into()))
}

/// `code`: a share code or room id; the server normalizes case and hyphens.
pub async fn join_room(http: &Client, base: &Url, token: &str, code: &str) -> Result<JoinedRoom> {
    let code: String = code.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    if code.is_empty() || code.len() > 64 {
        return Err(Error::Server { code: "ROOM_NOT_FOUND".into() });
    }
    let url = base.join(&format!("api/v1/rooms/{code}/join")).map_err(|_| Error::NotConfigured)?;
    let resp = http.post(url).bearer_auth(token).send().await.map_err(net)?;
    if !resp.status().is_success() {
        return Err(error_from(resp).await);
    }
    resp.json().await.map_err(|_| Error::Protocol("malformed join response".into()))
}

pub async fn health(http: &Client, base: &Url) -> bool {
    let Ok(url) = base.join("health") else { return false };
    http.get(url).timeout(Duration::from_secs(5)).send().await.is_ok_and(|r| r.status().is_success())
}

/// Why a WebSocket attempt failed, in the terms the session cares about.
#[derive(Debug)]
pub enum ConnectError {
    /// 401/403: ask for a fresh token once, then give up.
    Unauthenticated,
    /// 404/410: the room is gone.
    RoomGone,
    /// Anything else: network, 5xx, TLS… retry with back-off.
    Retry(String),
}

pub async fn connect(url: &Url, token: &str) -> std::result::Result<Socket, ConnectError> {
    let mut request = url.as_str().into_client_request().map_err(|e| ConnectError::Retry(e.to_string()))?;
    let value = HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| ConnectError::Unauthenticated)?;
    request.headers_mut().insert("Authorization", value);
    let attempt = tokio::time::timeout(Duration::from_secs(10), tokio_tungstenite::connect_async(request)).await;
    match attempt {
        Err(_) => Err(ConnectError::Retry("connection timed out".into())),
        Ok(Ok((socket, _))) => Ok(socket),
        Ok(Err(tokio_tungstenite::tungstenite::Error::Http(resp))) => Err(match resp.status().as_u16() {
            401 | 403 => ConnectError::Unauthenticated,
            404 | 410 => ConnectError::RoomGone,
            s => ConnectError::Retry(format!("handshake refused ({s})")),
        }),
        // The error text can embed the URL (room id): keep only its kind.
        Ok(Err(e)) => Err(ConnectError::Retry(format!("{:?}", std::mem::discriminant(&e)))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_plateaus() {
        let secs = |a| backoff_delay(a, 0.5).as_millis();
        assert_eq!([secs(0), secs(1), secs(2), secs(3), secs(4), secs(5), secs(40)], [500, 1000, 2000, 4000, 8000, 10_000, 10_000]);
    }

    #[test]
    fn backoff_jitter_stays_within_twenty_percent() {
        for a in 0..8 {
            let mid = backoff_delay(a, 0.5).as_millis() as f64;
            for j in [0.0, 0.25, 0.75, 1.0] {
                let d = backoff_delay(a, j).as_millis() as f64;
                assert!((d / mid - 1.0).abs() <= 0.21, "{a} {j}");
            }
        }
        assert!((0.0..1.0).contains(&jitter()));
    }

    #[test]
    fn websocket_url_follows_the_base_scheme() {
        let base = Url::parse("https://sync.example.com/").unwrap();
        assert_eq!(ws_url(&base, "/api/v1/rooms/ABC/ws").unwrap().as_str(), "wss://sync.example.com/api/v1/rooms/ABC/ws");
        let local = Url::parse("http://localhost:8787").unwrap();
        assert_eq!(ws_url(&local, "/api/v1/rooms/ABC/ws").unwrap().as_str(), "ws://localhost:8787/api/v1/rooms/ABC/ws");
    }

    #[test]
    fn a_hostile_ws_path_is_refused() {
        let base = Url::parse("https://sync.example.com/").unwrap();
        for bad in [
            "//evil.com/x",
            "/other",
            "/api/v1/rooms/../x",
            "/api/v1/rooms/a?token=1",
            "https://evil.com/api/v1/rooms/x",
            "/api/v1/rooms//x",
        ] {
            assert!(ws_url(&base, bad).is_err(), "{bad}");
        }
    }
}
