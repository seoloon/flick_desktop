//! The FlickDD REST calls and the ways they fail.

use std::time::Duration;

use oneshot_net::reqwest::{Client, Response, StatusCode};
use serde::Deserialize;
use url::Url;

use crate::item::{Backend, Kind};

/// No byte for this long: the request is dropped and retried.
pub const STALL: Duration = Duration::from_secs(20);
/// Nothing in a FlickDD exchange legitimately lasts this long; it replaces the
/// client's total timeout, which a slow segment would otherwise trip.
const REQUEST_CAP: Duration = Duration::from_secs(3600);

/// Everything that can stop a step. The engine classifies these (section 5 of the guide).
#[derive(Debug, Clone, thiserror::Error)]
pub enum Failure {
    #[error("HTTP {status} {code}")]
    Http { status: u16, code: String, retry_after: Option<u64> },
    #[error("network: {0}")]
    Network(String),
    #[error("no data for {} s", STALL.as_secs())]
    Stalled,
    #[error("the body ended early")]
    CutShort,
    /// Downloads are not set up (no invitation link).
    #[error("{0}")]
    NotConfigured(String),
    /// The partial file cannot be written.
    #[error("{0}")]
    Disk(String),
    /// The key that protects the files cannot be read.
    #[error("{0}")]
    Key(String),
}

impl Failure {
    pub fn http(status: u16, code: &str) -> Self {
        Self::Http { status, code: code.into(), retry_after: None }
    }

    pub fn status(&self) -> u16 {
        match self {
            Self::Http { status, .. } => *status,
            _ => 0,
        }
    }

    pub fn code(&self) -> &str {
        match self {
            Self::Http { code, .. } => code,
            _ => "",
        }
    }
}

fn net(e: oneshot_net::reqwest::Error) -> Failure {
    // `without_url`: the download id must not end up in logs, nor the token.
    Failure::Network(e.without_url().to_string())
}

#[derive(Deserialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    code: String,
}

async fn failure_from(resp: Response) -> Failure {
    let status = resp.status();
    let retry_after = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok());
    let code = resp.json::<ErrorBody>().await.map(|b| b.error.code).unwrap_or_default();
    Failure::Http { status: status.as_u16(), code, retry_after }
}

/// A grant: the server-side record that lets one file be fetched. The token is
/// a secret: it is never printed nor persisted.
#[derive(Clone, Deserialize)]
pub struct Grant {
    /// Relative to the server address (`/api/v1/downloads/{id}/file`).
    pub url: String,
    pub download_id: String,
    pub token: String,
    pub size: u64,
    pub filename: String,
    #[serde(default)]
    pub mime: String,
    pub etag: String,
    pub chunk_bytes: u64,
}

impl std::fmt::Debug for Grant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grant").field("size", &self.size).field("filename", &self.filename).finish_non_exhaustive()
    }
}

/// Where a grant lives: enough to talk to its server.
#[derive(Debug, Clone)]
pub struct Link {
    pub http: Client,
    pub base: Url,
}

impl Link {
    /// `path` comes from the server: only plain paths under the downloads API are followed.
    fn resolve(&self, path: &str) -> Result<Url, Failure> {
        if !path.starts_with("/api/v1/downloads/") || path.contains("//") || path.contains("..") || path.contains('?') || path.contains('#') {
            return Err(Failure::Network("unexpected download path".into()));
        }
        self.base.join(path.trim_start_matches('/')).map_err(|_| Failure::NotConfigured("The Flick Server address is not valid: add the invitation link again in Settings › Flick Server.".into()))
    }
}

/// `POST /api/v1/downloads`.
pub async fn create(link: &Link, jwt: &str, backend: Backend, item_id: &str, title: &str, kind: Option<Kind>) -> Result<Grant, Failure> {
    let url = link.base.join("api/v1/downloads").map_err(|_| Failure::NotConfigured("The Flick Server address is not valid: add the invitation link again in Settings › Flick Server.".into()))?;
    let title: String = title.chars().take(200).collect();
    let body = serde_json::json!({ "backend": backend.as_str(), "item_id": item_id, "title": title, "kind": kind.map(Kind::as_str) });
    let resp = link.http.post(url).bearer_auth(jwt).timeout(Duration::from_secs(30)).json(&body).send().await.map_err(net)?;
    if !resp.status().is_success() {
        return Err(failure_from(resp).await);
    }
    let grant: Grant = resp.json().await.map_err(|_| Failure::Network("malformed create response".into()))?;
    if grant.size == 0 || grant.chunk_bytes == 0 || !grant.url.ends_with("/file") {
        return Err(Failure::Network("unusable grant".into()));
    }
    Ok(grant)
}

/// `DELETE /api/v1/downloads/{id}`: frees the slot. Best effort, idempotent: a
/// grant that is already gone is a success.
pub async fn release(link: &Link, grant: &Grant) {
    let Ok(url) = link.resolve(grant.url.strip_suffix("/file").unwrap_or(&grant.url)) else { return };
    let _ = link.http.delete(url).bearer_auth(&grant.token).timeout(Duration::from_secs(15)).send().await;
}

/// An open `206` response.
#[derive(Debug)]
pub struct Segment {
    pub resp: Response,
    /// `Content-Length` of this response.
    pub want: u64,
}

/// `(start, end, size)` of a `Content-Range: bytes S-E/size` value.
pub fn parse_content_range(v: &str) -> Option<(u64, u64, u64)> {
    let rest = v.trim().strip_prefix("bytes ")?;
    let (range, size) = rest.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?, size.parse().ok()?))
}

/// Requests `bytes=offset-end` with the grant's ETag as validator. A `200`
/// (validator mismatch) or a `Content-Range` that does not start at `offset`
/// is reported as `SOURCE_CHANGED`.
pub async fn open_segment(link: &Link, grant: &Grant, offset: u64, end: u64) -> Result<Segment, Failure> {
    let url = link.resolve(&grant.url)?;
    let send = link
        .http
        .get(url)
        .bearer_auth(&grant.token)
        .header("Range", format!("bytes={offset}-{end}"))
        .header("If-Range", &grant.etag)
        .timeout(REQUEST_CAP)
        .send();
    let resp = tokio::time::timeout(STALL, send).await.map_err(|_| Failure::Stalled)?.map_err(net)?;
    match resp.status() {
        StatusCode::PARTIAL_CONTENT => {}
        // The whole file: the validator did not match. Dropping the response closes it unread.
        StatusCode::OK => return Err(Failure::http(409, "SOURCE_CHANGED")),
        _ => return Err(failure_from(resp).await),
    }
    let range = resp.headers().get("content-range").and_then(|v| v.to_str().ok()).and_then(parse_content_range);
    match range {
        Some((start, _, size)) if start == offset && size == grant.size => {}
        _ => return Err(Failure::http(409, "SOURCE_CHANGED")),
    }
    let want = resp.content_length().unwrap_or(0);
    Ok(Segment { resp, want })
}

/// Next chunk of a segment, under the stall rule.
pub async fn next_chunk(seg: &mut Segment) -> Result<Option<bytes::Bytes>, Failure> {
    match tokio::time::timeout(STALL, seg.resp.chunk()).await {
        Err(_) => Err(Failure::Stalled),
        Ok(Err(e)) => Err(net(e)),
        Ok(Ok(chunk)) => Ok(chunk),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range_is_parsed() {
        assert_eq!(parse_content_range("bytes 8388608-16777215/4831838208"), Some((8_388_608, 16_777_215, 4_831_838_208)));
        assert_eq!(parse_content_range("bytes */100"), None);
        assert_eq!(parse_content_range("items 0-1/2"), None);
        assert_eq!(parse_content_range("bytes 5-x/9"), None);
    }

    #[test]
    fn only_download_paths_are_followed() {
        let link = Link { http: Client::new(), base: Url::parse("https://flick.example.com/sync/").unwrap() };
        assert_eq!(link.resolve("/api/v1/downloads/abc/file").unwrap().as_str(), "https://flick.example.com/sync/api/v1/downloads/abc/file");
        assert!(link.resolve("/api/v1/rooms/x").is_err());
        assert!(link.resolve("//evil.example/api/v1/downloads/x").is_err());
        assert!(link.resolve("/api/v1/downloads/../x").is_err());
        assert!(link.resolve("/api/v1/downloads/x?token=1").is_err());
    }
}
