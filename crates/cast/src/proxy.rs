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

enum Kind {
    Upstream {
        /// The original URL with its file name and query removed.
        dir: Url,
        /// The original URL's query (often the server's credentials): kept here,
        /// never shown to the receiver, and added to every request relayed.
        query: Vec<(String, String)>,
        headers: Vec<(String, String)>,
        http: Client,
    },
    /// The files of a folder (a conversion's playlist and segments).
    Files(std::path::PathBuf),
}

struct Route {
    token: String,
    kind: Kind,
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
        let listener = TcpListener::bind(SocketAddr::new(local, 0)).await.map_err(|e| Error::Network(oneshot_core::codes::CAST_RELAY.tag(format!("Flick could not open the local relay the device pulls the video from ({e}). Check no firewall blocks Flick on the local network."))))?;
        let addr = listener.local_addr().map_err(|e| Error::Network(oneshot_core::codes::CAST_RELAY.tag(format!("Flick could not open the local relay ({e})."))))?;
        let token = uuid::Uuid::new_v4().simple().to_string();
        let mut dir = upstream.clone();
        dir.set_query(None);
        dir.set_fragment(None);
        let file = dir.path_segments().and_then(|mut s| s.next_back().map(str::to_owned)).unwrap_or_default();
        if let Ok(mut segments) = dir.path_segments_mut() {
            segments.pop();
            segments.push("");
        }
        let relayed = Url::parse(&format!("http://{addr}/c/{token}/{file}")).map_err(|e| Error::Invalid(oneshot_core::codes::CAST_RELAY.tag(format!("The relay address could not be built ({e})."))))?;
        let query = upstream.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
        self.run(listener, Route { token, kind: Kind::Upstream { dir, query, headers, http } });
        Ok(relayed)
    }

    fn run(&self, listener: TcpListener, route: Route) {
        let route = std::sync::Arc::new(route);
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
    }

    /// Serves the files of `root` under a secret path; the URL to give out ends with `/c/<token>/`.
    pub async fn serve_dir(&self, root: std::path::PathBuf, local: IpAddr) -> Result<Url> {
        self.stop();
        let listener = TcpListener::bind(SocketAddr::new(local, 0)).await.map_err(|e| Error::Network(oneshot_core::codes::CAST_RELAY.tag(format!("Flick could not open the local relay the device pulls the video from ({e}). Check no firewall blocks Flick on the local network."))))?;
        let addr = listener.local_addr().map_err(|e| Error::Network(oneshot_core::codes::CAST_RELAY.tag(format!("Flick could not open the local relay ({e})."))))?;
        let token = uuid::Uuid::new_v4().simple().to_string();
        let base = Url::parse(&format!("http://{addr}/c/{token}/")).map_err(|e| Error::Invalid(oneshot_core::codes::CAST_RELAY.tag(format!("The relay address could not be built ({e})."))))?;
        self.run(listener, Route { token, kind: Kind::Files(root) });
        Ok(base)
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
fn upstream_for(token: &str, dir: &Url, query: &[(String, String)], head: &Head) -> Option<Url> {
    let rest = head.path.strip_prefix("/c/")?.strip_prefix(token)?.strip_prefix('/')?;
    let mut url = dir.join(rest).ok()?;
    // Only the original URL's own directory: no `..` out of it.
    if url.host_str() != dir.host_str() || !url.path().starts_with(dir.path()) {
        return None;
    }
    // The receiver's own parameters (a playlist's segment links), plus the
    // original ones it was never given.
    url.set_query(head.query.as_deref());
    let present: Vec<String> = url.query_pairs().map(|(k, _)| k.into_owned()).collect();
    for (k, v) in query.iter().filter(|(k, _)| !present.contains(k)) {
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
    let (dir, query, headers, http) = match &route.kind {
        Kind::Files(root) => return serve_file(&mut socket, &route.token, root, &head).await,
        Kind::Upstream { dir, query, headers, http } => (dir, query, headers, http),
    };
    let Some(url) = upstream_for(&route.token, dir, query, &head) else { return respond(&mut socket, "404 Not Found", "").await };

    // One request to the server, with `range` when there is one. A link that drops it is tried again: the
    // server is often far away, and a reader behind the relay (ffmpeg) gives up at the first error.
    let send = |range: Option<String>| {
        let mut req = if head.method == "HEAD" { http.head(url.clone()) } else { http.get(url.clone()) };
        for (k, v) in headers {
            req = req.header(k, v);
        }
        if let Some(range) = range {
            req = req.header("Range", range);
        }
        async move {
            let mut last = None;
            for attempt in 0..ATTEMPTS {
                if attempt > 0 {
                    tokio::time::sleep(RETRY_PAUSE * attempt).await;
                }
                match req.try_clone().expect("a body-less request").send().await {
                    Ok(r) => return Ok(r),
                    Err(e) => {
                        tracing::debug!(target: "cast", attempt, "relay upstream: {e}");
                        last = Some(e);
                    }
                }
            }
            Err(last.expect("at least one attempt"))
        }
    };
    let mut resp = match send(head.range.clone()).await {
        Ok(r) => r,
        Err(_) => return respond(&mut socket, "502 Bad Gateway", "").await,
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
        // Where the answer began in the file, and how far the end was asked: what a resume needs.
        let first = if status.as_u16() == 206 { content_range_start(resp.headers()) } else if status.as_u16() == 200 { Some(0) } else { None };
        let last = head.range.as_deref().and_then(|r| r.strip_prefix("bytes=")?.split_once('-')?.1.parse::<u64>().ok());
        let mut sent = 0u64;
        let mut resumed = 0;
        loop {
            match resp.chunk().await {
                Ok(Some(bytes)) => {
                    socket.write_all(&bytes).await?;
                    sent += bytes.len() as u64;
                }
                Ok(None) => break,
                Err(e) => {
                    // The link broke mid-body: ask the server for the rest, so the reader sees one whole body.
                    tracing::debug!(target: "cast", sent, "relay upstream body: {e}");
                    let Some(first) = first.filter(|_| resumed < ATTEMPTS) else { break };
                    resumed += 1;
                    tokio::time::sleep(RETRY_PAUSE * resumed).await;
                    let from = first + sent;
                    let range = format!("bytes={from}-{}", last.map(|l| l.to_string()).unwrap_or_default());
                    match send(Some(range)).await {
                        Ok(next) if next.status().as_u16() == 206 => resp = next,
                        _ => break,
                    }
                }
            }
        }
    }
    socket.shutdown().await
}

/// How many times a request, or the rest of a body, is asked of the server before the relay gives up.
const ATTEMPTS: u32 = 3;
const RETRY_PAUSE: std::time::Duration = std::time::Duration::from_millis(400);

/// The first byte of a `Content-Range: bytes a-b/total` answer.
fn content_range_start(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers.get("content-range")?.to_str().ok()?.strip_prefix("bytes ")?.split('-').next()?.trim().parse().ok()
}

/// The file a relay path stands for, if it carries the secret and stays inside `root`.
fn file_for(token: &str, root: &std::path::Path, head: &Head) -> Option<std::path::PathBuf> {
    let rest = head.path.strip_prefix("/c/")?.strip_prefix(token)?.strip_prefix('/')?;
    let mut path = root.to_path_buf();
    for part in rest.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains(['\\', ':', '%', '\0']) {
            return None;
        }
        path.push(part);
    }
    Some(path)
}

fn content_type(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("m3u8") => "application/vnd.apple.mpegurl",
        Some("m4s" | "mp4") => "video/mp4",
        _ => "application/octet-stream",
    }
}

/// `bytes=a-b`, `bytes=a-` and `bytes=-n` over a body of `len` bytes: the first and last byte.
fn byte_range(header: &str, len: u64) -> Option<(u64, u64)> {
    let spec = header.strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    let (first, last) = match (a.parse::<u64>().ok(), b.parse::<u64>().ok()) {
        (Some(a), Some(b)) => (a, b.min(len.saturating_sub(1))),
        (Some(a), None) => (a, len.saturating_sub(1)),
        (None, Some(n)) if n > 0 => (len.saturating_sub(n), len.saturating_sub(1)),
        _ => return None,
    };
    (first <= last && first < len).then_some((first, last))
}

async fn serve_file(socket: &mut TcpStream, route_token: &str, root: &std::path::Path, head: &Head) -> std::io::Result<()> {
    let Some(path) = file_for(route_token, root, head) else { return respond(socket, "404 Not Found", "").await };
    let Ok(body) = tokio::fs::read(&path).await else { return respond(socket, "404 Not Found", "").await };
    let len = body.len() as u64;
    let (status, slice, range) = match head.range.as_deref().and_then(|r| byte_range(r, len)) {
        Some((a, b)) => ("206 Partial Content", &body[a as usize..=b as usize], format!("content-range: bytes {a}-{b}/{len}\r\n")),
        None => ("200 OK", &body[..], String::new()),
    };
    let out = format!("HTTP/1.1 {status}\r\n{CORS}content-type: {}\r\ncontent-length: {}\r\naccept-ranges: bytes\r\n{range}Cache-Control: no-cache\r\nConnection: close\r\n\r\n", content_type(&path), slice.len());
    socket.write_all(out.as_bytes()).await?;
    if head.method == "GET" {
        socket.write_all(slice).await?;
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

    /// An upstream that drops the connection halfway through its first answer (a flaky server on a bad link),
    /// and serves what is asked with `Range` afterwards.
    async fn flaky_upstream(drops: usize) -> SocketAddr {
        const BODY: &[u8] = b"0123456789";
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let mut served = 0;
            loop {
                let Ok((mut s, _)) = listener.accept().await else { return };
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                let from: usize = head.split("range: bytes=").nth(1).and_then(|r| r.split('-').next()).and_then(|r| r.trim().parse().ok()).unwrap_or(0);
                let (status, extra) = if from > 0 { ("206 Partial Content", format!("content-range: bytes {from}-9/10\r\n")) } else { ("200 OK", String::new()) };
                let body = &BODY[from..];
                s.write_all(format!("HTTP/1.1 {status}\r\ncontent-length: {}\r\n{extra}connection: close\r\n\r\n", body.len()).as_bytes()).await.ok();
                if served < drops {
                    s.write_all(&body[..4.min(body.len())]).await.ok();
                    s.flush().await.ok();
                    drop(s); // cut: the client sees a body shorter than content-length
                } else {
                    s.write_all(body).await.ok();
                }
                served += 1;
            }
        });
        addr
    }

    #[tokio::test]
    async fn a_body_cut_by_the_upstream_is_resumed_with_a_range() {
        let up = flaky_upstream(1).await;
        let proxy = Proxy::default();
        let relayed = proxy.serve(&Url::parse(&format!("http://{up}/v/file.mkv")).unwrap(), vec![], "127.0.0.1".parse().unwrap(), Client::new()).await.unwrap();
        let addr = format!("{}:{}", relayed.host_str().unwrap(), relayed.port().unwrap());
        let r = get(&addr, relayed.path(), "").await;
        assert!(r.starts_with("HTTP/1.1 200") && r.ends_with("0123456789"), "{r}");
        proxy.stop();
    }

    #[tokio::test]
    async fn a_request_the_upstream_drops_before_answering_is_tried_again() {
        // First connection: accepted and closed without a word.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let up = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (s, _) = listener.accept().await.unwrap();
            drop(s);
            loop {
                let Ok((mut s, _)) = listener.accept().await else { return };
                let mut buf = vec![0u8; 4096];
                let _ = s.read(&mut buf).await;
                s.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok").await.ok();
            }
        });
        let proxy = Proxy::default();
        let relayed = proxy.serve(&Url::parse(&format!("http://{up}/v/file.mkv")).unwrap(), vec![], "127.0.0.1".parse().unwrap(), Client::new()).await.unwrap();
        let addr = format!("{}:{}", relayed.host_str().unwrap(), relayed.port().unwrap());
        let r = get(&addr, relayed.path(), "").await;
        assert!(r.starts_with("HTTP/1.1 200") && r.ends_with("ok"), "{r}");
        proxy.stop();
    }

    #[test]
    fn byte_ranges() {
        assert_eq!(byte_range("bytes=0-3", 10), Some((0, 3)));
        assert_eq!(byte_range("bytes=4-", 10), Some((4, 9)));
        assert_eq!(byte_range("bytes=-3", 10), Some((7, 9)));
        assert_eq!(byte_range("bytes=2-99", 10), Some((2, 9)));
        assert_eq!(byte_range("bytes=10-", 10), None);
        assert_eq!(byte_range("items=0-1", 10), None);
    }

    #[tokio::test]
    async fn serves_a_folder_under_the_secret_and_refuses_to_leave_it() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("0")).unwrap();
        std::fs::write(root.path().join("0/index.m3u8"), "#EXTM3U\n").unwrap();
        std::fs::write(root.path().join("0/seg00000.m4s"), b"0123456789").unwrap();
        std::fs::write(root.path().parent().unwrap().join("outside.txt"), "secret").ok();

        let proxy = Proxy::default();
        let base = proxy.serve_dir(root.path().to_path_buf(), "127.0.0.1".parse().unwrap()).await.unwrap();
        let addr = format!("{}:{}", base.host_str().unwrap(), base.port().unwrap());
        let at = |rest: &str| format!("{}{rest}", base.path());

        let list = get(&addr, &at("0/index.m3u8"), "").await;
        assert!(list.starts_with("HTTP/1.1 200") && list.contains("application/vnd.apple.mpegurl") && list.ends_with("#EXTM3U\n"), "{list}");
        let seg = get(&addr, &at("0/seg00000.m4s"), "Range: bytes=2-4\r\n").await;
        assert!(seg.starts_with("HTTP/1.1 206") && seg.contains("video/mp4") && seg.contains("bytes 2-4/10") && seg.ends_with("234"), "{seg}");

        for bad in [at("../outside.txt"), at("0/../../outside.txt"), at("0/..%2f..%2foutside.txt"), at("0\\..\\..\\outside.txt"), at("/etc/passwd"), "/0/index.m3u8".to_owned(), at("0/missing.m4s")] {
            let r = get(&addr, &bad, "").await;
            assert!(r.starts_with("HTTP/1.1 404"), "{bad}: {r}");
        }
        proxy.stop();
    }
}
