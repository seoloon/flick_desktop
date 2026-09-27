//! plex.tv authentication (PIN flow) and server discovery.
//!
//! The user never types a Plex password into this app: we create a PIN,
//! open `app.plex.tv/auth` in the system browser, and poll until the PIN is
//! linked to an account token.

use std::time::Duration;

use oneshot_core::{Error, Result};
use oneshot_net::reqwest::{Client, RequestBuilder};
use url::Url;

use crate::dto::{Pin, PlexUser, Resource};

const PLEX_TV: &str = "https://plex.tv/";

/// Identity headers (`X-Plex-*`) sent with every request.
#[derive(Debug, Clone)]
pub struct PlexIdentity {
    pub product: String,
    pub version: String,
    /// Stable per installation.
    pub client_identifier: String,
    pub device_name: String,
    pub platform: String,
}

impl PlexIdentity {
    pub fn apply(&self, rb: RequestBuilder, token: Option<&str>) -> RequestBuilder {
        let rb = rb
            .header("Accept", "application/json")
            .header("X-Plex-Product", &self.product)
            .header("X-Plex-Version", &self.version)
            .header("X-Plex-Client-Identifier", &self.client_identifier)
            .header("X-Plex-Device-Name", &self.device_name)
            .header("X-Plex-Device", "PC")
            .header("X-Plex-Platform", &self.platform)
            // Base client profile for unknown desktop clients; we extend it
            // per request with X-Plex-Client-Profile-Extra.
            .header("X-Plex-Client-Profile-Name", "Generic");
        match token {
            Some(t) => rb.header("X-Plex-Token", t),
            None => rb,
        }
    }

    /// Headers for the player's own requests. The transcoder answers 400 to a
    /// request without `X-Plex-Platform` (it picks the client profile from it).
    pub fn headers(&self, token: &str) -> Vec<(String, String)> {
        vec![
            ("X-Plex-Token".into(), token.into()),
            ("X-Plex-Client-Identifier".into(), self.client_identifier.clone()),
            ("X-Plex-Product".into(), self.product.clone()),
            ("X-Plex-Version".into(), self.version.clone()),
            ("X-Plex-Platform".into(), self.platform.clone()),
            ("X-Plex-Device-Name".into(), self.device_name.clone()),
        ]
    }
}

#[derive(Debug, Clone)]
pub struct PinChallenge {
    pub id: i64,
    pub code: String,
    /// URL to open in the system browser.
    pub auth_url: Url,
}

/// A server the account can reach, with its connections ranked best-first.
#[derive(Debug, Clone)]
pub struct DiscoveredServer {
    pub name: String,
    pub machine_id: String,
    pub version: Option<String>,
    pub access_token: String,
    pub owned: bool,
    /// Reachable URIs, best first (local HTTPS > local HTTP > remote > relay).
    pub reachable: Vec<Url>,
}

#[derive(Debug, Clone)]
pub struct PlexAccount {
    pub token: String,
    pub user_id: String,
    pub username: String,
    pub avatar: Option<Url>,
}

#[derive(Debug, Clone)]
pub struct PlexAuth {
    http: Client,
    identity: PlexIdentity,
    base: Url,
}

impl PlexAuth {
    pub fn new(http: Client, identity: PlexIdentity) -> Self {
        Self { http, identity, base: Url::parse(PLEX_TV).expect("static url") }
    }

    /// For tests: point at a mock plex.tv.
    pub fn with_base(mut self, base: Url) -> Self {
        self.base = base;
        self
    }

    fn url(&self, path: &str) -> Result<Url> {
        oneshot_net::join(&self.base, path)
    }

    pub async fn start_pin(&self) -> Result<PinChallenge> {
        let rb = self.http.post(self.url("api/v2/pins?strong=true")?);
        let pin: Pin = oneshot_net::json(self.identity.apply(rb, None).send().await.map_err(oneshot_net::map_err)?).await?;
        let mut auth_url = Url::parse("https://app.plex.tv/auth").expect("static url");
        auth_url.set_fragment(Some(&format!(
            "?clientID={}&code={}&context%5Bdevice%5D%5Bproduct%5D={}",
            self.identity.client_identifier, pin.code, self.identity.product
        )));
        Ok(PinChallenge { id: pin.id, code: pin.code, auth_url })
    }

    /// `Ok(None)` while the user has not approved the PIN yet.
    pub async fn poll_pin(&self, id: i64) -> Result<Option<PlexAccount>> {
        let rb = self.http.get(self.url(&format!("api/v2/pins/{id}"))?);
        let pin: Pin = oneshot_net::json(self.identity.apply(rb, None).send().await.map_err(oneshot_net::map_err)?).await?;
        let Some(token) = pin.auth_token.filter(|t| !t.is_empty()) else { return Ok(None) };
        self.account(&token).await.map(Some)
    }

    pub async fn account(&self, token: &str) -> Result<PlexAccount> {
        let rb = self.http.get(self.url("api/v2/user")?);
        let user: PlexUser = oneshot_net::json(self.identity.apply(rb, Some(token)).send().await.map_err(oneshot_net::map_err)?).await?;
        Ok(PlexAccount {
            token: token.to_owned(),
            user_id: user.id.to_string(),
            username: user.title.or(user.username).unwrap_or_else(|| user.uuid.clone()),
            avatar: user.thumb.and_then(|t| Url::parse(&t).ok()),
        })
    }

    /// Lists servers and probes every advertised connection concurrently.
    pub async fn discover(&self, account_token: &str) -> Result<Vec<DiscoveredServer>> {
        let rb = self.http.get(self.url("api/v2/resources?includeHttps=1&includeRelay=1&includeIPv6=1")?);
        let resources: Vec<Resource> =
            oneshot_net::json(self.identity.apply(rb, Some(account_token)).send().await.map_err(oneshot_net::map_err)?).await?;
        let servers = resources.into_iter().filter(|r| r.provides.split(',').any(|p| p == "server"));
        let probes = servers.map(|r| async move {
            let token = r.access_token.clone().unwrap_or_else(|| account_token.to_owned());
            let reachable = self.rank_connections(&r, &token).await;
            DiscoveredServer {
                name: r.name,
                machine_id: r.client_identifier,
                version: r.product_version,
                access_token: token,
                owned: r.owned,
                reachable,
            }
        });
        Ok(futures::future::join_all(probes).await)
    }

    async fn rank_connections(&self, r: &Resource, token: &str) -> Vec<Url> {
        let mut conns = r.connections.clone();
        // Preference order before latency: local > remote > relay, https first.
        conns.sort_by_key(|c| (c.relay, !c.local, !c.uri.starts_with("https"), c.ipv6));
        let checks = conns.iter().map(|c| async move {
            let url = Url::parse(&c.uri).ok()?;
            let rb = self.http.get(oneshot_net::join(&url, "identity").ok()?).timeout(Duration::from_secs(4));
            let ok = self.identity.apply(rb, Some(token)).send().await.is_ok_and(|r| r.status().is_success());
            ok.then_some(url)
        });
        futures::future::join_all(checks).await.into_iter().flatten().collect()
    }
}

/// Maps a relay-only situation to an honest error for the UI.
pub fn require_reachable(server: &DiscoveredServer) -> Result<Url> {
    server
        .reachable
        .first()
        .cloned()
        .ok_or_else(|| Error::Network(format!("no connection to {} is reachable from this network", server.name)))
}
