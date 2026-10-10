//! A small HTTP/1.1 client on one TCP connection, which can switch to the
//! encrypted framing AirPlay receivers use after `pair-verify`.
//!
//! `reqwest` cannot do this: pairing and the playback calls that follow must
//! share one connection, and once verified, every byte on it is sealed.
//!
//! Framing (HAP): up to 1024 bytes of plain text per frame, sent as
//! `length (2 bytes, little-endian) | ciphertext | tag (16 bytes)`; the length
//! is the additional data, the nonce is 4 zero bytes then a frame counter
//! (8 bytes, little-endian), counted separately in each direction.

use std::net::SocketAddr;
use std::time::Duration;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::hap::SessionKeys;

const FRAME: usize = 1024;
const TAG: usize = 16;
const STEP: Duration = Duration::from_secs(10);
/// Nothing AirPlay answers is anywhere near this large.
const MAX_BODY: usize = 8 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum LinkError {
    #[error("{0}")]
    Io(String),
    #[error("the receiver did not answer in time")]
    Timeout,
    #[error("the receiver sent something that is not HTTP")]
    Malformed,
    #[error("the encrypted channel broke")]
    Crypto,
}

impl From<std::io::Error> for LinkError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

/// One direction's sealing state.
struct Sealer {
    cipher: ChaCha20Poly1305,
    counter: u64,
}

impl Sealer {
    fn new(key: &[u8; 32]) -> Self {
        Self { cipher: ChaCha20Poly1305::new(Key::from_slice(key)), counter: 0 }
    }

    fn nonce(&mut self) -> [u8; 12] {
        let mut n = [0u8; 12];
        n[4..].copy_from_slice(&self.counter.to_le_bytes());
        self.counter += 1;
        n
    }

    /// `data` as frames.
    fn seal(&mut self, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len() + (data.len() / FRAME + 1) * (2 + TAG));
        for chunk in data.chunks(FRAME) {
            let len = (chunk.len() as u16).to_le_bytes();
            let nonce = self.nonce();
            let sealed = self.cipher.encrypt(Nonce::from_slice(&nonce), Payload { msg: chunk, aad: &len }).expect("encryption does not fail");
            out.extend(len);
            out.extend(sealed);
        }
        out
    }

    /// Opens the complete frames at the start of `raw`, removing them from it.
    fn open(&mut self, raw: &mut Vec<u8>) -> Result<Vec<u8>, LinkError> {
        let mut plain = Vec::new();
        let mut used = 0;
        while raw.len() - used >= 2 {
            let len = u16::from_le_bytes([raw[used], raw[used + 1]]) as usize;
            if len > FRAME {
                return Err(LinkError::Crypto);
            }
            let end = used + 2 + len + TAG;
            if raw.len() < end {
                break;
            }
            let nonce = self.nonce();
            let aad = [raw[used], raw[used + 1]];
            let opened = self.cipher.decrypt(Nonce::from_slice(&nonce), Payload { msg: &raw[used + 2..end], aad: &aad }).map_err(|_| LinkError::Crypto)?;
            plain.extend(opened);
            used = end;
        }
        raw.drain(..used);
        Ok(plain)
    }
}

struct Channel {
    write: Sealer,
    read: Sealer,
}

pub(crate) struct Link {
    stream: TcpStream,
    host: String,
    channel: Option<Channel>,
    /// Bytes read from the socket, not yet opened.
    raw: Vec<u8>,
    /// Opened (or plain) bytes, not yet consumed.
    ready: Vec<u8>,
}

impl Link {
    pub async fn connect(addr: SocketAddr) -> Result<Self, LinkError> {
        let stream = tokio::time::timeout(STEP, TcpStream::connect(addr)).await.map_err(|_| LinkError::Timeout)??;
        stream.set_nodelay(true).ok();
        Ok(Self { stream, host: addr.to_string(), channel: None, raw: Vec::new(), ready: Vec::new() })
    }

    /// From now on everything is sealed with these keys.
    pub fn encrypt_with(&mut self, keys: &SessionKeys) {
        self.channel = Some(Channel { write: Sealer::new(&keys.write), read: Sealer::new(&keys.read) });
    }

    #[cfg(test)]
    pub fn is_encrypted(&self) -> bool {
        self.channel.is_some()
    }

    async fn fill(&mut self) -> Result<(), LinkError> {
        let mut buf = [0u8; 8192];
        let n = tokio::time::timeout(STEP, self.stream.read(&mut buf)).await.map_err(|_| LinkError::Timeout)??;
        if n == 0 {
            return Err(LinkError::Io("the receiver closed the connection".into()));
        }
        match &mut self.channel {
            Some(c) => {
                self.raw.extend(&buf[..n]);
                let opened = c.read.open(&mut self.raw)?;
                self.ready.extend(opened);
            }
            None => self.ready.extend(&buf[..n]),
        }
        Ok(())
    }

    pub async fn request(&mut self, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Result<Response, LinkError> {
        let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {}\r\nContent-Length: {}\r\n", self.host, body.len());
        for (k, v) in headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str("\r\n");
        let mut data = head.into_bytes();
        data.extend(body);
        let out = match &mut self.channel {
            Some(c) => c.write.seal(&data),
            None => data,
        };
        tokio::time::timeout(STEP, self.stream.write_all(&out)).await.map_err(|_| LinkError::Timeout)??;

        let head_end = loop {
            if let Some(i) = self.ready.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
            if self.ready.len() > 64 * 1024 {
                return Err(LinkError::Malformed);
            }
            self.fill().await?;
        };
        let (status, headers) = parse_head(&self.ready[..head_end])?;
        let len = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse::<usize>().ok()).unwrap_or(0);
        if len > MAX_BODY {
            return Err(LinkError::Malformed);
        }
        while self.ready.len() < head_end + len {
            self.fill().await?;
        }
        let body = self.ready[head_end..head_end + len].to_vec();
        self.ready.drain(..head_end + len);
        Ok(Response { status, body })
    }
}

fn parse_head(head: &[u8]) -> Result<(u16, Vec<(String, String)>), LinkError> {
    let text = std::str::from_utf8(head).map_err(|_| LinkError::Malformed)?;
    let mut lines = text.split("\r\n");
    let status = lines.next().and_then(|l| l.split(' ').nth(1)).and_then(|s| s.parse().ok()).ok_or(LinkError::Malformed)?;
    let headers = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned())).collect();
    Ok((status, headers))
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpListener;

    use super::*;

    #[test]
    fn frames_round_trip_across_sizes_and_split_reads() {
        let key = [5u8; 32];
        for size in [0usize, 1, 1023, 1024, 1025, 5000] {
            let data: Vec<u8> = (0..size).map(|i| (i * 7) as u8).collect();
            let sealed = Sealer::new(&key).seal(&data);
            let mut opener = Sealer::new(&key);
            // The bytes arrive in awkward pieces.
            let (mut raw, mut got) = (Vec::new(), Vec::new());
            for piece in sealed.chunks(37) {
                raw.extend(piece);
                got.extend(opener.open(&mut raw).unwrap());
            }
            assert_eq!(got, data, "size {size}");
            assert!(raw.is_empty());
        }
    }

    #[test]
    fn a_tampered_or_replayed_frame_is_refused() {
        let key = [5u8; 32];
        let mut sealed = Sealer::new(&key).seal(b"hello");
        let last = sealed.len() - 1;
        sealed[last] ^= 1;
        assert!(matches!(Sealer::new(&key).open(&mut sealed), Err(LinkError::Crypto)));
        // The same frame twice: the second counter does not match.
        let one = Sealer::new(&key).seal(b"hello");
        let mut twice = [one.clone(), one].concat();
        assert!(matches!(Sealer::new(&key).open(&mut twice), Err(LinkError::Crypto)));
    }

    #[test]
    fn a_head_gives_status_and_headers() {
        let (status, headers) = parse_head(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nServer: AirTunes/625\r\n\r\n").unwrap();
        assert_eq!(status, 200);
        assert_eq!(headers[0], ("Content-Length".to_owned(), "3".to_owned()));
        assert!(parse_head(b"nonsense\r\n\r\n").is_err());
    }

    /// A request and its answer, plain then sealed, against a model receiver.
    #[tokio::test]
    async fn requests_work_plain_then_encrypted_on_one_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let keys = SessionKeys { write: [1; 32], read: [2; 32] };
        let server_keys = SessionKeys { write: keys.read, read: keys.write };
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            // Plain exchange.
            let n = s.read(&mut buf).await.unwrap();
            assert!(buf[..n].starts_with(b"POST /pair-verify HTTP/1.1"));
            s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").await.unwrap();
            // Then sealed in both directions.
            let (mut r, mut w) = (Sealer::new(&server_keys.read), Sealer::new(&server_keys.write));
            let mut raw = Vec::new();
            let mut plain = Vec::new();
            while !plain.windows(4).any(|x| x == b"\r\n\r\n") {
                let n = s.read(&mut buf).await.unwrap();
                raw.extend(&buf[..n]);
                plain.extend(r.open(&mut raw).unwrap());
            }
            assert!(plain.starts_with(b"GET /playback-info HTTP/1.1"));
            let body = vec![b'x'; 3000];
            let reply = [format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes(), body].concat();
            s.write_all(&w.seal(&reply)).await.unwrap();
        });
        let mut link = Link::connect(addr).await.unwrap();
        let first = link.request("POST", "/pair-verify", &[], b"abc").await.unwrap();
        assert_eq!((first.status, first.body.as_slice()), (200, &b"ok"[..]));
        link.encrypt_with(&keys);
        assert!(link.is_encrypted());
        let second = link.request("GET", "/playback-info", &[], b"").await.unwrap();
        assert_eq!((second.status, second.body.len()), (200, 3000));
        server.await.unwrap();
    }
}
