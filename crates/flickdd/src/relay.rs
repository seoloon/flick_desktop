//! Plays a sealed file: a relay on this computer's loopback address decrypts
//! the file for the player, with `Range` support so seeking works.
//!
//! Only this machine can connect, and every address carries a random secret,
//! so another program needs it to read anything. The decrypted bytes exist
//! only in flight, never on disk.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use crate::seal::Seal;

/// A sealed file to serve.
#[derive(Debug)]
pub struct Target {
    pub path: PathBuf,
    pub seal: Seal,
}

/// Finds the file of a download by id.
pub type Resolver = Arc<dyn Fn(&str) -> Option<Target> + Send + Sync>;

const CHUNK: usize = 256 * 1024;

pub struct Relay {
    addr: SocketAddr,
    token: String,
    task: JoinHandle<()>,
}

impl std::fmt::Debug for Relay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Relay").field("addr", &self.addr).finish_non_exhaustive()
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Relay {
    pub async fn start(resolve: Resolver) -> std::io::Result<Self> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await?;
        let addr = listener.local_addr()?;
        let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
        let secret = token.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else { break };
                let (resolve, secret) = (Arc::clone(&resolve), secret.clone());
                tokio::spawn(async move {
                    if let Err(e) = handle(socket, &secret, &resolve).await {
                        tracing::debug!(target: "flickdd", "relay request ended: {e}");
                    }
                });
            }
        });
        Ok(Self { addr, token, task })
    }

    /// The address the player opens for a download. The `.mkv` only helps format detection.
    pub fn url(&self, download_id: &str) -> String {
        format!("http://{}/s/{}/{download_id}.mkv", self.addr, self.token)
    }
}

/// Which bytes a request wants, out of `len`.
#[derive(Debug, PartialEq)]
enum Wanted {
    Whole,
    Bytes(u64, u64),
    Unsatisfiable,
}

/// A single `Range: bytes=…` header (several ranges are served as the whole file).
fn parse_range(header: Option<&str>, len: u64) -> Wanted {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else { return Wanted::Whole };
    if spec.contains(',') {
        return Wanted::Whole;
    }
    let Some((a, b)) = spec.split_once('-') else { return Wanted::Whole };
    let (start, end) = match (a.trim(), b.trim()) {
        ("", n) => match n.parse::<u64>() {
            Ok(n) if n > 0 => (len.saturating_sub(n), len.saturating_sub(1)),
            _ => return Wanted::Unsatisfiable,
        },
        (s, "") => match s.parse::<u64>() {
            Ok(s) => (s, len.saturating_sub(1)),
            Err(_) => return Wanted::Whole,
        },
        (s, e) => match (s.parse::<u64>(), e.parse::<u64>()) {
            (Ok(s), Ok(e)) => (s, e.min(len.saturating_sub(1))),
            _ => return Wanted::Whole,
        },
    };
    if len == 0 || start >= len || start > end { Wanted::Unsatisfiable } else { Wanted::Bytes(start, end) }
}

struct Head {
    method: String,
    path: String,
    range: Option<String>,
}

fn parse_head(raw: &str) -> Option<Head> {
    let mut lines = raw.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let path = first.next()?.split('?').next()?.to_owned();
    let range = lines.filter_map(|l| l.split_once(':')).find(|(k, _)| k.eq_ignore_ascii_case("range")).map(|(_, v)| v.trim().to_owned());
    Some(Head { method, path, range })
}

/// The download id in `/s/<token>/<id>.mkv`, if the secret is right.
fn id_for<'a>(path: &'a str, token: &str) -> Option<&'a str> {
    let rest = path.strip_prefix("/s/")?.strip_prefix(token)?.strip_prefix('/')?;
    let id = rest.rsplit_once('.').map_or(rest, |(id, _)| id);
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric())).then_some(id)
}

async fn reply(socket: &mut TcpStream, status: &str, extra: &str) -> std::io::Result<()> {
    socket.write_all(format!("HTTP/1.1 {status}\r\n{extra}Content-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await
}

async fn handle(mut socket: TcpStream, token: &str, resolve: &Resolver) -> std::io::Result<()> {
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
    let Some(head) = parse_head(&String::from_utf8_lossy(&buf)) else { return reply(&mut socket, "400 Bad Request", "").await };
    if head.method != "GET" && head.method != "HEAD" {
        return reply(&mut socket, "405 Method Not Allowed", "").await;
    }
    let Some(target) = id_for(&head.path, token).and_then(|id| resolve(id)) else { return reply(&mut socket, "404 Not Found", "").await };
    let Ok(mut file) = File::open(&target.path).await else { return reply(&mut socket, "404 Not Found", "").await };
    let len = file.metadata().await?.len();
    let (status, start, end, range_line) = match parse_range(head.range.as_deref(), len) {
        Wanted::Whole => ("200 OK", 0, len.saturating_sub(1), String::new()),
        Wanted::Bytes(s, e) => ("206 Partial Content", s, e, format!("Content-Range: bytes {s}-{e}/{len}\r\n")),
        Wanted::Unsatisfiable => return reply(&mut socket, "416 Range Not Satisfiable", &format!("Content-Range: bytes */{len}\r\n")).await,
    };
    let size = if len == 0 { 0 } else { end - start + 1 };
    socket
        .write_all(
            format!("HTTP/1.1 {status}\r\nContent-Type: application/octet-stream\r\nAccept-Ranges: bytes\r\n{range_line}Content-Length: {size}\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .await?;
    if head.method == "HEAD" || size == 0 {
        return Ok(());
    }
    file.seek(std::io::SeekFrom::Start(start)).await?;
    let (mut at, mut left) = (start, size);
    let mut data = vec![0u8; CHUNK];
    while left > 0 {
        let want = (left as usize).min(CHUNK);
        let n = file.read(&mut data[..want]).await?;
        if n == 0 {
            break;
        }
        target.seal.apply(at, &mut data[..n]);
        socket.write_all(&data[..n]).await?;
        at += n as u64;
        left -= n as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range(None, 100), Wanted::Whole);
        assert_eq!(parse_range(Some("bytes=10-19"), 100), Wanted::Bytes(10, 19));
        assert_eq!(parse_range(Some("bytes=10-"), 100), Wanted::Bytes(10, 99));
        assert_eq!(parse_range(Some("bytes=-5"), 100), Wanted::Bytes(95, 99));
        assert_eq!(parse_range(Some("bytes=90-500"), 100), Wanted::Bytes(90, 99));
        assert_eq!(parse_range(Some("bytes=100-"), 100), Wanted::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=0-1,5-6"), 100), Wanted::Whole);
    }

    #[test]
    fn only_the_secret_path_names_a_download() {
        assert_eq!(id_for("/s/tok/abc123.mkv", "tok"), Some("abc123"));
        assert_eq!(id_for("/s/other/abc123.mkv", "tok"), None);
        assert_eq!(id_for("/s/tok/../etc.mkv", "tok"), None);
        assert_eq!(id_for("/s/tok/", "tok"), None);
    }

    #[tokio::test]
    async fn serves_the_plain_bytes_of_a_sealed_file() {
        let dir = tempfile::tempdir().unwrap();
        let plain: Vec<u8> = (0..5000u32).map(|i| (i % 253) as u8).collect();
        let seal = Seal::derive(&[3; 32], "dl1");
        let mut sealed = plain.clone();
        seal.apply(0, &mut sealed);
        let path = dir.path().join("f.mkv");
        std::fs::write(&path, &sealed).unwrap();
        let resolve: Resolver = Arc::new(move |id| (id == "dl1").then(|| Target { path: path.clone(), seal: seal.clone() }));
        let relay = Relay::start(resolve).await.unwrap();
        let url = relay.url("dl1");
        let (host, path) = url.strip_prefix("http://").unwrap().split_once('/').unwrap();
        for (range, want) in [(None, &plain[..]), (Some("bytes=1001-2999"), &plain[1001..3000])] {
            let mut s = TcpStream::connect(host).await.unwrap();
            let r = range.map(|r| format!("Range: {r}\r\n")).unwrap_or_default();
            s.write_all(format!("GET /{path} HTTP/1.1\r\nHost: x\r\n{r}\r\n").as_bytes()).await.unwrap();
            let mut got = Vec::new();
            s.read_to_end(&mut got).await.unwrap();
            let at = got.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
            assert_eq!(&got[at..], want);
        }
        let bad = url.replace("/s/", "/s/x");
        let mut s = TcpStream::connect(host).await.unwrap();
        s.write_all(format!("GET /{} HTTP/1.1\r\n\r\n", bad.split_once('/').map(|_| bad.strip_prefix("http://").unwrap().split_once('/').unwrap().1).unwrap()).as_bytes()).await.unwrap();
        let mut got = String::new();
        s.read_to_string(&mut got).await.unwrap();
        assert!(got.starts_with("HTTP/1.1 404"));
    }
}
