//! FlickSync invitation links: one string that carries the server's address
//! and the key its users sign in with.
//!
//! ```text
//! flicksync://<host>[:<port>][/<prefix>]/?v=1&tls=<0|1>#k=<base64url(kid:server_id:secret)>
//! ```
//!
//! `<prefix>` is the path under which a reverse proxy serves FlickSync
//! (`/sync`); every API path is appended to the base URL, prefix included.
//!
//! The key sits in the fragment so it is never sent over HTTP nor seen by a
//! proxy. The link **is** a secret: it is parsed here, kept in the OS keychain
//! by the app, and nothing in this module prints it. The format is the
//! server's (`flick-integration.md#invitation-link`); this parser is written
//! by hand because a generic URL parser treats an unknown scheme's fragment
//! differently from one library to the next.

use std::net::{Ipv4Addr, Ipv6Addr};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use url::Url;

use crate::auth::SigningKey;

const SCHEME: &str = "flicksync://";
const VERSION: &str = "1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InviteError {
    #[error("not an invitation link")]
    NotAnInvitation,
    #[error("the link has no key")]
    MissingKey,
    #[error("the link's key is unreadable")]
    BadKey,
    #[error("the link has no usable address")]
    BadAddress,
    #[error("the link has an invalid path")]
    BadPath,
    #[error("the link has no valid version")]
    BadVersion,
    #[error("unsupported link version")]
    UnsupportedVersion,
    #[error("the link has no valid tls setting")]
    BadTls,
}

impl InviteError {
    /// What to tell the person who pasted the link.
    pub fn message(self) -> &'static str {
        match self {
            Self::NotAnInvitation => "This isn't a FlickSync invitation link.",
            Self::UnsupportedVersion => "This link comes from a newer version of FlickSync. Update Flick.",
            Self::MissingKey | Self::BadKey => "The link is incomplete: copy it again in full.",
            Self::BadAddress | Self::BadPath | Self::BadVersion | Self::BadTls => "The link is damaged: copy it again in full, or ask for a new one.",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Invitation {
    /// Lower-case `host[:port]`; an IPv6 host keeps its brackets.
    host: String,
    /// Path prefix of a reverse proxy: empty, or `/seg[/seg…]` without a trailing slash.
    prefix: String,
    tls: bool,
    pub key: SigningKey,
}

impl Invitation {
    pub fn parse(s: &str) -> Result<Self, InviteError> {
        // Copy-paste brings spaces and line breaks along.
        let rest = s.trim().strip_prefix(SCHEME).ok_or(InviteError::NotAnInvitation)?;
        let (before, fragment) = rest.split_once('#').ok_or(InviteError::MissingKey)?;
        let (location, query) = before.split_once('?').unwrap_or((before, ""));
        let (authority, prefix) = match location.split_once('/') {
            Some((a, path)) => (a, normalize_prefix(path)?),
            None => (location, String::new()),
        };
        let host = normalize_authority(authority)?;

        match param(query, "v") {
            Some(VERSION) => {}
            Some(_) => return Err(InviteError::UnsupportedVersion),
            None => return Err(InviteError::BadVersion),
        }
        let tls = match param(query, "tls") {
            Some("1") => true,
            Some("0") => false,
            _ => return Err(InviteError::BadTls),
        };
        let raw = URL_SAFE_NO_PAD.decode(param(fragment, "k").ok_or(InviteError::MissingKey)?).map_err(|_| InviteError::BadKey)?;
        let key = String::from_utf8(raw).map_err(|_| InviteError::BadKey)?;
        let key = SigningKey::parse(&key).map_err(|_| InviteError::BadKey)?;
        Ok(Self { host, prefix, tls, key })
    }

    /// The canonical link, to store or compare. A secret.
    pub fn link(&self) -> String {
        format!("{SCHEME}{}{}/?v={VERSION}&tls={}#k={}", self.host, self.prefix, u8::from(self.tls), URL_SAFE_NO_PAD.encode(self.key.expose()))
    }

    /// `host[:port]`, without the prefix.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// `host[:port][/prefix]`: what the server is called, safe to show.
    pub fn address(&self) -> String {
        format!("{}{}", self.host, self.prefix)
    }

    pub fn tls(&self) -> bool {
        self.tls
    }

    /// REST base, prefix included. It ends with `/`, so that joining `api/v1/…`
    /// to it keeps the prefix (`https://flick.example.com/sync/`).
    pub fn base_url(&self) -> Url {
        // The authority and the prefix were validated at parse time, so this cannot fail.
        Url::parse(&format!("{}://{}{}/", if self.tls { "https" } else { "http" }, self.host, self.prefix))
            .unwrap_or_else(|_| Url::parse("http://invalid/").expect("static url"))
    }

    /// Plain HTTP to a host that is not on a local network: tokens would cross
    /// the Internet in the clear.
    pub fn is_insecure_remote(&self) -> bool {
        !self.tls && !is_local(&self.host)
    }
}

/// Value of `name` in an `a=1&b=2` list.
fn param<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.split('&').filter_map(|kv| kv.split_once('=')).find(|(k, _)| *k == name).map(|(_, v)| v)
}

fn valid_port(p: &str) -> bool {
    !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u32>().is_ok_and(|n| (1..=65535).contains(&n))
}

/// The path after the authority (`""`, `"sync/"`, `"sync"`, `"a/b/"`) as a
/// prefix: empty, or `/a/b`. Segments are `A-Za-z0-9 - . _ ~`, never empty,
/// `.` or `..`; at most one trailing slash.
fn normalize_prefix(path: &str) -> Result<String, InviteError> {
    // `path` follows the first slash: `"/"` would be an empty segment (`host//`).
    if path == "/" {
        return Err(InviteError::BadPath);
    }
    let path = path.strip_suffix('/').unwrap_or(path);
    if path.is_empty() {
        return Ok(String::new());
    }
    let segment_ok = |s: &str| !s.is_empty() && s != "." && s != ".." && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'));
    if path.split('/').all(segment_ok) { Ok(format!("/{path}")) } else { Err(InviteError::BadPath) }
}

/// Lower-cases and checks `host[:port]` (`[v6]` in brackets).
fn normalize_authority(authority: &str) -> Result<String, InviteError> {
    let authority = authority.to_lowercase();
    let bad = InviteError::BadAddress;
    if let Some(rest) = authority.strip_prefix('[') {
        let (inner, after) = rest.split_once(']').ok_or(bad)?;
        inner.parse::<Ipv6Addr>().map_err(|_| bad)?;
        match after {
            "" => {}
            _ if after.strip_prefix(':').is_some_and(valid_port) => {}
            _ => return Err(bad),
        }
        return Ok(authority);
    }
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (authority.as_str(), None),
    };
    let name_ok = !host.is_empty()
        && host.len() <= 253
        && !host.starts_with(['.', '-'])
        && !host.ends_with(['.', '-'])
        && host.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'));
    if !name_ok || port.is_some_and(|p| !valid_port(p)) {
        return Err(bad);
    }
    Ok(authority)
}

/// A host on this machine or a private network (plain HTTP is fine there).
fn is_local(authority: &str) -> bool {
    if let Some(rest) = authority.strip_prefix('[') {
        let Some(v6) = rest.split_once(']').and_then(|(h, _)| h.parse::<Ipv6Addr>().ok()) else { return false };
        let first = v6.segments()[0];
        return v6.is_loopback() || first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80;
    }
    let host = authority.split_once(':').map_or(authority, |(h, _)| h);
    if let Ok(v4) = host.parse::<Ipv4Addr>() {
        return v4.is_private() || v4.is_loopback() || v4.is_link_local();
    }
    // A bare name ("nas") only resolves on the local network.
    !host.contains('.') || host.ends_with(".local") || host.ends_with(".lan") || host.ends_with(".home.arpa")
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "main:default:0123456789abcdef0123456789abcdef0123456789abcdef";
    const LINK: &str = "flicksync://sync.example.com/?v=1&tls=1#k=bWFpbjpkZWZhdWx0OjAxMjM0NTY3ODlhYmNkZWYwMTIzNDU2Nzg5YWJjZGVmMDEyMzQ1Njc4OWFiY2RlZg";

    fn link(authority: &str, query: &str, fragment: &str) -> String {
        format!("flicksync://{authority}{query}#{fragment}")
    }

    fn k() -> String {
        format!("k={}", URL_SAFE_NO_PAD.encode(KEY))
    }

    #[test]
    fn the_servers_example_is_parsed_and_reproduced_identically() {
        let inv = Invitation::parse(LINK).unwrap();
        assert_eq!(inv.host(), "sync.example.com");
        assert!(inv.tls());
        assert_eq!((inv.key.kid.as_str(), inv.key.server_id.as_str()), ("main", "default"));
        assert_eq!(inv.base_url().as_str(), "https://sync.example.com/");
        assert_eq!(inv.link(), LINK);
    }

    #[test]
    fn pasted_whitespace_is_tolerated() {
        assert!(Invitation::parse(&format!("  \n{LINK}\r\n")).is_ok());
    }

    #[test]
    fn an_ipv6_host_with_a_port_over_plain_http() {
        let inv = Invitation::parse(&link("[::1]:8443", "/?v=1&tls=0", &k())).unwrap();
        assert_eq!(inv.host(), "[::1]:8443");
        assert_eq!(inv.base_url().as_str(), "http://[::1]:8443/");
        assert!(!inv.is_insecure_remote(), "loopback is local");
    }

    #[test]
    fn unknown_parameters_are_ignored() {
        let inv = Invitation::parse(&link("a.example", "/?v=1&tls=1&futur=x", &format!("{}&autre=1", k()))).unwrap();
        assert_eq!(inv.host(), "a.example");
    }

    #[test]
    fn the_host_is_lowercased() {
        assert_eq!(Invitation::parse(&link("Sync.Example.COM", "/?v=1&tls=1", &k())).unwrap().host(), "sync.example.com");
    }

    #[test]
    fn refused_links_say_why() {
        let ok = |q: &str| Invitation::parse(&link("a.example", q, &k()));
        assert_eq!(Invitation::parse("https://a.example").unwrap_err(), InviteError::NotAnInvitation);
        assert_eq!(Invitation::parse("flicksync://a.example/?v=1&tls=1").unwrap_err(), InviteError::MissingKey);
        assert_eq!(Invitation::parse(&link("a.example", "/?v=1&tls=1", "other=1")).unwrap_err(), InviteError::MissingKey);
        assert_eq!(ok("/?v=2&tls=1").unwrap_err(), InviteError::UnsupportedVersion);
        assert_eq!(ok("/?tls=1").unwrap_err(), InviteError::BadVersion);
        assert_eq!(ok("/?v=1&tls=2").unwrap_err(), InviteError::BadTls);
        assert_eq!(ok("/?v=1").unwrap_err(), InviteError::BadTls);
    }

    #[test]
    fn a_proxy_prefix_is_kept_in_the_base_url_the_address_and_the_link() {
        let sync = format!("flicksync://flick.example.com/sync/?v=1&tls=1#{}", k());
        let inv = Invitation::parse(&sync).unwrap();
        assert_eq!(inv.host(), "flick.example.com");
        assert_eq!(inv.address(), "flick.example.com/sync");
        assert_eq!(inv.base_url().as_str(), "https://flick.example.com/sync/");
        assert_eq!(inv.base_url().join("api/v1/rooms").unwrap().as_str(), "https://flick.example.com/sync/api/v1/rooms", "joins stay under the prefix");
        assert_eq!(inv.link(), sync);
        // Several segments, a port, and a missing trailing slash (tolerated, written back with it).
        let deep = Invitation::parse(&link("a.example:8443", "/x/y.z_~-1?v=1&tls=0", &k())).unwrap();
        assert_eq!(deep.address(), "a.example:8443/x/y.z_~-1");
        assert_eq!(deep.base_url().as_str(), "http://a.example:8443/x/y.z_~-1/");
        let bare = Invitation::parse(&link("a.example", "/sync?v=1&tls=1", &k())).unwrap();
        assert_eq!(bare.address(), "a.example/sync");
        assert!(bare.link().contains("a.example/sync/?v=1"));
        // No prefix: the same as before.
        assert_eq!(Invitation::parse(LINK).unwrap().base_url().as_str(), "https://sync.example.com/");
    }

    #[test]
    fn invalid_prefixes_are_refused() {
        for path in ["//", "/sync//", "//sync/", "/./", "/../", "/a/../b/", "/a b/", "/sy%6ec/", "/é/"] {
            let r = Invitation::parse(&link("a.example", &format!("{path}?v=1&tls=1"), &k()));
            assert_eq!(r.unwrap_err(), InviteError::BadPath, "{path:?}");
        }
    }

    #[test]
    fn bad_ports_and_hosts_are_refused() {
        for authority in ["a.example:99999", "a.example:0", "a.example:", "a.example:x", "", "a b", "user@a.example", "a.example:1:2", "[::1", "[nope]:80", "-a.example"] {
            assert_eq!(Invitation::parse(&link(authority, "/?v=1&tls=1", &k())).unwrap_err(), InviteError::BadAddress, "{authority:?}");
        }
    }

    #[test]
    fn unreadable_keys_are_refused() {
        let with = |k: &str| Invitation::parse(&link("a.example", "/?v=1&tls=1", &format!("k={k}")));
        let enc = |s: &str| URL_SAFE_NO_PAD.encode(s);
        assert_eq!(with("not base64!").unwrap_err(), InviteError::BadKey);
        assert_eq!(with(&format!("{}=", enc(KEY))).unwrap_err(), InviteError::BadKey, "padding is not allowed");
        assert_eq!(with(&enc("main:default")).unwrap_err(), InviteError::BadKey, "two parts");
        assert_eq!(with(&enc("main:default:tooshort")).unwrap_err(), InviteError::BadKey, "secret under 32 characters");
        assert_eq!(with(&enc("ma in:default:0123456789abcdef0123456789abcdef")).unwrap_err(), InviteError::BadKey, "space in the kid");
    }

    #[test]
    fn plain_http_is_only_acceptable_on_a_local_network() {
        let insecure = |a: &str| Invitation::parse(&link(a, "/?v=1&tls=0", &k())).unwrap().is_insecure_remote();
        assert!(insecure("sync.example.com"));
        assert!(insecure("8.8.8.8:8787"));
        for local in ["localhost:8787", "192.168.1.20:8787", "10.0.0.5", "172.16.3.4", "nas", "box.local", "[fd00::1]:8787", "[fe80::1]"] {
            assert!(!insecure(local), "{local}");
        }
        let https = Invitation::parse(&link("sync.example.com", "/?v=1&tls=1", &k())).unwrap();
        assert!(!https.is_insecure_remote());
    }

    #[test]
    fn the_secret_never_shows_in_debug_output() {
        let shown = format!("{:?}", Invitation::parse(LINK).unwrap());
        assert!(!shown.contains("0123456789abcdef"));
    }
}
