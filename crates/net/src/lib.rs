//! One HTTP policy for every provider and for the image cache: timeouts,
//! proxy, connection pooling, user agent, error mapping and log redaction.

use std::time::Duration;

use oneshot_core::{Error, codes};
use oneshot_core::settings::NetworkSettings;
use reqwest::{Response, StatusCode};
use serde::de::DeserializeOwned;
use url::Url;

pub use reqwest;

pub const USER_AGENT: &str = concat!("Flick/", env!("CARGO_PKG_VERSION"));

pub fn client(settings: &NetworkSettings) -> Result<reqwest::Client, Error> {
    let mut b = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(u64::from(settings.timeout_secs.min(10))))
        .timeout(Duration::from_secs(u64::from(settings.timeout_secs)))
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(usize::from(settings.concurrent_requests));
    if let Some(proxy) = &settings.proxy {
        b = b.proxy(reqwest::Proxy::all(proxy).map_err(|e| Error::Invalid(codes::NET_PROXY.tag(format!("The proxy address in the settings is not valid ({e})."))))?);
    }
    b = match settings.ip_family {
        oneshot_core::settings::IpFamily::V4Only => b.local_address(std::net::IpAddr::from([0, 0, 0, 0])),
        oneshot_core::settings::IpFamily::V6Only => b.local_address(std::net::IpAddr::from([0u16; 8])),
        oneshot_core::settings::IpFamily::Any => b,
    };
    if settings.allow_invalid_certificates {
        tracing::warn!(target: "net", "TLS certificate validation disabled by user setting");
        b = b.danger_accept_invalid_certs(true);
    }
    b.build().map_err(|e| Error::Network(codes::NET_CLIENT.tag(format!("Flick could not set up its network connection ({e})."))))
}

/// Maps transport errors to domain errors.
pub fn map_err(e: reqwest::Error) -> Error {
    if let Some(status) = e.status() {
        return status_error(status, "");
    }
    let raw = redact(&e.to_string());
    let lower = raw.to_ascii_lowercase();
    if e.is_timeout() {
        Error::Network(codes::NET_TIMEOUT.tag("The server took too long to answer."))
    } else if lower.contains("certificate") || lower.contains("tls") || lower.contains("ssl") {
        Error::Network(codes::NET_TLS.tag("The secure connection to the server could not be trusted (HTTPS certificate)."))
    } else if e.is_connect() {
        Error::Network(codes::NET_UNREACHABLE.tag("Flick could not reach the server. Check that it is on and that the address is right."))
    } else {
        Error::Network(codes::NET_OTHER.tag(format!("The connection to the server failed ({raw}).")))
    }
}

fn status_error(status: StatusCode, body: &str) -> Error {
    // The body is for the log: it may be a page of HTML, never something to show.
    tracing::debug!(target: "net", %status, body = %body.chars().take(300).collect::<String>(), "HTTP error");
    match status {
        StatusCode::UNAUTHORIZED => Error::Unauthorized,
        StatusCode::FORBIDDEN => Error::Forbidden(codes::AUTH_FORBIDDEN.tag("The server does not allow this for your account.")),
        StatusCode::NOT_FOUND => Error::NotFound(codes::SRV_NOT_FOUND.tag("The server does not have what was asked for. It may have been removed: refresh the library.")),
        StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => Error::Network(codes::NET_TIMEOUT.tag("The server took too long to answer.")),
        s if s.is_server_error() => Error::Protocol(codes::NET_SERVER_ERROR.tag(format!("The server had a problem (HTTP {}). Try again in a moment.", s.as_u16()))),
        s => Error::Protocol(codes::NET_BAD_ANSWER.tag(format!("The server refused the request (HTTP {}).", s.as_u16()))),
    }
}

/// Checks the status and returns the response for further processing.
pub async fn ensure_ok(resp: Response) -> Result<Response, Error> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    Err(status_error(status, &body))
}

pub async fn json<T: DeserializeOwned>(resp: Response) -> Result<T, Error> {
    let resp = ensure_ok(resp).await?;
    let url = redact(resp.url().as_str());
    let bytes = resp.bytes().await.map_err(map_err)?;
    serde_json::from_slice(&bytes).map_err(|e| {
        tracing::warn!(target: "provider", %url, error = %e, "unexpected JSON");
        Error::Protocol(codes::NET_BAD_ANSWER.tag("The server's answer could not be understood. Is the server up to date?"))
    })
}

/// Removes credentials from URLs/strings before logging.
/// PINs are credentials too and must not appear in logs.
pub fn redact(s: &str) -> String {
    const KEYS: [&str; 5] = ["api_key=", "X-Plex-Token=", "ApiKey=", "token=", "pin="];
    let mut out = s.to_owned();
    for key in KEYS {
        let mut from = 0;
        while let Some(pos) = out[from..].find(key) {
            let start = from + pos + key.len();
            let end = out[start..].find(['&', ' ', '"', '\'']).map_or(out.len(), |i| start + i);
            out.replace_range(start..end, "***");
            from = start + 3;
        }
    }
    out
}

/// Joins a relative API path to a server base URL that may itself contain a
/// path prefix (reverse proxies: `https://host/jellyfin/`).
pub fn join(base: &Url, path: &str) -> Result<Url, Error> {
    let mut base = base.clone();
    if !base.path().ends_with('/') {
        base.set_path(&format!("{}/", base.path()));
    }
    base.join(path.trim_start_matches('/')).map_err(|e| Error::Invalid(codes::NET_ADDRESS.tag(format!("The server address is not valid ({path}: {e})."))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_tokens() {
        assert_eq!(redact("http://h/a?api_key=SECRET&x=1"), "http://h/a?api_key=***&x=1");
        assert_eq!(redact("u?X-Plex-Token=abc"), "u?X-Plex-Token=***");
    }

    #[test]
    fn redacts_pins() {
        assert_eq!(redact("https://plex.tv/api/v2/home/users/u/switch?pin=1234"), "https://plex.tv/api/v2/home/users/u/switch?pin=***");
        assert_eq!(redact("x?a=1&pin=0000&b=2"), "x?a=1&pin=***&b=2");
    }

    #[test]
    fn join_keeps_reverse_proxy_prefix() {
        let base = Url::parse("https://host/jellyfin").unwrap();
        assert_eq!(join(&base, "/Items").unwrap().as_str(), "https://host/jellyfin/Items");
    }
}
