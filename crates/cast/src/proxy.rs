//! The relay a receiver pulls the stream from.
//!
//! A Chromecast or an AirPlay device cannot send the server's credentials and
//! may not reach it at all. The relay listens on this machine's LAN address
//! and forwards each request (with its `Range`) to the real URL, adding the
//! headers the server wants. Everything lives under a secret path so only the
//! device handed that address can use it.
//!
//! HLS playlists refer to their segments relatively, so the relay maps a
//! request path onto the *directory* of the original URL.

use std::net::{IpAddr, SocketAddr};

use oneshot_core::{Error, Result};
use parking_lot::Mutex;
use reqwest::Client;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use url::Url;

struct Route {
    token: String,
    /// The original URL with its file name and query removed.
    dir: Url,
    /// The original URL's query (often the server's credentials): kept here,
    /// never shown to the receiver, and added to every request relayed.
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    http: Client,
}

#[derive(Default)]
pub struct Proxy {
    running: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for Proxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Proxy").field("running", &self.running.lock().is_some()).finish()
    }
}

impl Proxy {
    /// Starts relaying `upstream`; returns the URL to give the receiver. Any
    /// earlier relay stops.
    pub async fn serve(&self, upstream: &Url, headers: Vec<(String, String)>, local: IpAddr, http: Client) -> Result<Url> {
        self.stop();
        let listener = TcpListener::bind(SocketAddr::new(local, 0)).await.map_err(|e| Error::Network(format!("cannot open the relay: {e}")))?;
        let addr = listener.local_addr().map_err(|e| Error::Network(e.to_string()))?;
        let token = uuid::Uuid::new_v4().simple().to_string();
        let mut dir = upstream.clone();
        dir.set_query(None);
        dir.set_fragment(None);
        let file = dir.path_segments().and_then(|mut s| s.next_back().map(str::to_owned)).unwrap_or_default();
        if let Ok(mut segments) = dir.path_segments_mut() {
            segments.pop();
            segments.push("");
        }
        let relayed = Url::parse(&format!("http://{addr}/c/{token}/{file}")).map_err(|e| Error::Invalid(e.to_string()))?;
        let query = upstream.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
        let route = std::sync::Arc::new(Route { token, dir, query, headers, http });
        let task = tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else { break };
                let route = std::sync::Arc::clone(&route);
                tokio::spawn(async move {
                    if let Err(e) = handle(socket, &route).await {
                        tracing::debug!(target: "cast", "relay request ended: {e}");
                    }
                });
            }
        });
        *self.running.lock() = Some(task);
        Ok(relayed)
    }

    pub fn stop(&self) {
        if let Some(task) = self.running.lock().take() {
            task.abort();
        }
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Head {
    method: String,
    path: String,
    query: Option<String>,
    range: Option<String>,
}

fn parse_head(raw: &str) -> Option<Head> {
    let mut lines = raw.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let target = first.next()?;
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_owned(), Some(q.to_owned())),
        None => (target.to_owned(), None),
    };
    let range = lines.filter_map(|l| l.split_once(':')).find(|(k, _)| k.eq_ignore_ascii_case("range")).map(|(_, v)| v.trim().to_owned());
    Some(Head { method, path, query, range })
}

/// The server URL a relay path stands for, if it carries the secret.
fn upstream_for(route: &Route, head: &Head) -> Option<Url> {
    let rest = head.path.strip_prefix("/c/")?.strip_prefix(route.token.as_str())?.strip_prefix('/')?;
    let mut url = route.dir.join(rest).ok()?;
    // Only the original URL's own directory: no `..` out of it.
    if url.host_str() != route.dir.host_str() || !url.path().starts_with(route.dir.path()) {
        return None;
    }
    // The receiver's own parameters (a playlist's segment links), plus the
    // original ones it was never given.
    url.set_query(head.query.as_deref());
    let present: Vec<String> = url.query_pairs().map(|(k, _)| k.into_owned()).collect();
    for (k, v) in route.query.iter().filter(|(k, _)| !present.contains(k)) {
        url.query_pairs_mut().append_pair(k, v);
    }
    Some(url)
}

const CORS: &str = "Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: Range, Content-Type\r\nAccess-Control-Expose-Headers: Content-Length, Content-Range, Accept-Ranges\r\n";

async fn handle(mut socket: TcpStream, route: &Route) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 2048];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        if buf.len() > 16 * 1024 {
            return Ok(());
        }
        let n = socket.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let raw = String::from_utf8_lossy(&buf);
    let Some(head) = parse_head(&raw) else { return respond(&mut socket, "400 Bad Request", "").await };

    if head.method == "OPTIONS" {
        return respond(&mut socket, "204 No Content", "").await;
    }
    if head.method != "GET" && head.method != "HEAD" {
        return respond(&mut socket, "405 Method Not Allowed", "").await;
    }
    let Some(url) = upstream_for(route, &head) else { return respond(&mut socket, "404 Not Found", "").await };

    let mut req = if head.method == "HEAD" { route.http.head(url) } else { route.http.get(url) };
    for (k, v) in &route.headers {
        req = req.header(k, v);
    }
    if let Some(range) = &head.range {
        req = req.header("Range", range);
    }
    let mut resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!(target: "cast", "relay upstream: {e}");
            return respond(&mut socket, "502 Bad Gateway", "").await;
        }
    };
    let status = resp.status();
    let mut out = format!("HTTP/1.1 {} {}\r\n{CORS}Connection: close\r\n", status.as_u16(), status.canonical_reason().unwrap_or(""));
    for name in ["content-type", "content-length", "content-range", "accept-ranges"] {
        if let Some(v) = resp.headers().get(name).and_then(|v| v.to_str().ok()) {
            out.push_str(&format!("{name}: {v}\r\n"));
        }
    }
    out.push_str("\r\n");
    socket.write_all(out.as_bytes()).await?;
    if head.method == "GET" {
        while let Ok(Some(bytes)) = resp.chunk().await {
            socket.write_all(&bytes).await?;
        }
    }
    socket.shutdown().await
}

async fn respond(socket: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    let out = format!("HTTP/1.1 {status}\r\n{CORS}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    socket.write_all(out.as_bytes()).await?;
    socket.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn get(addr: &str, target: &str, extra: &str) -> String {
        let mut s = TcpStream::connect(addr).await.unwrap();
        s.write_all(format!("GET {target} HTTP/1.1\r\nHost: x\r\n{extra}\r\n").as_bytes()).await.unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).await.unwrap();
        String::from_utf8_lossy(&out).into_owned()
    }

    #[tokio::test]
    async fn forwards_range_and_credentials_and_maps_relative_paths() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/videos/7/master.m3u8"))
            .and(query_param("api_key", "k"))
            .respond_with(ResponseTemplate::new(200).insert_header("content-type", "application/x-mpegURL").set_body_string("#EXTM3U\nmain.m3u8?x=1\n"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/videos/7/seg0.ts"))
            .and(header("x-token", "secret"))
            .and(header("range", "bytes=0-3"))
            .respond_with(ResponseTemplate::new(206).insert_header("content-range", "bytes 0-3/10").set_body_bytes(b"abcd".to_vec()))
            .mount(&server)
            .await;

        let proxy = Proxy::default();
        let upstream = Url::parse(&format!("{}/videos/7/master.m3u8?api_key=k", server.uri())).unwrap();
        let relayed = proxy.serve(&upstream, vec![("x-token".into(), "secret".into())], "127.0.0.1".parse().unwrap(), Client::new()).await.unwrap();
        let addr = format!("{}:{}", relayed.host_str().unwrap(), relayed.port().unwrap());
        assert!(relayed.path().ends_with("/master.m3u8"));
        // The credential stays here: the receiver's URL carries none.
        assert_eq!(relayed.query(), None);

        let playlist = get(&addr, relayed.path(), "").await;
        assert!(playlist.starts_with("HTTP/1.1 200"), "{playlist}");
        assert!(playlist.contains("access-control-allow-origin: *") || playlist.contains("Access-Control-Allow-Origin: *"));
        assert!(playlist.ends_with("#EXTM3U\nmain.m3u8?x=1\n"));

        // A segment, as the playlist names it: next to the playlist.
        let dir = relayed.path().rsplit_once('/').unwrap().0;
        let seg = get(&addr, &format!("{dir}/seg0.ts"), "Range: bytes=0-3\r\n").await;
        assert!(seg.starts_with("HTTP/1.1 206"), "{seg}");
        assert!(seg.ends_with("abcd"));

        // Without the secret path the relay gives nothing.
        let denied = get(&addr, "/videos/7/seg0.ts", "").await;
        assert!(denied.starts_with("HTTP/1.1 404"), "{denied}");
        let escape = get(&addr, &format!("{dir}/../../other"), "").await;
        assert!(escape.starts_with("HTTP/1.1 404") || escape.starts_with("HTTP/1.1 502"), "{escape}");
        proxy.stop();
    }
}
