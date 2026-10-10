//! Connection and authentication (password or Quick Connect).

use oneshot_core::{Error, Result};
use oneshot_net::reqwest::Client;
use serde_json::json;
use url::Url;

use crate::dto::{AuthenticationResult, PublicSystemInfo, PublicUserDto, QuickConnectResult};

/// How this installation identifies itself to Jellyfin. `device_id` must be
/// stable per installation: Jellyfin keys sessions and tokens on it.
#[derive(Debug, Clone)]
pub struct ClientIdentity {
    pub client: String,
    pub device_name: String,
    pub device_id: String,
    pub version: String,
}

impl ClientIdentity {
    /// `Authorization: MediaBrowser …` header value (Jellyfin ≥ 10.8 scheme).
    pub fn header(&self, token: Option<&str>) -> String {
        let esc = |s: &str| s.replace('"', "");
        let mut h = format!(
            r#"MediaBrowser Client="{}", Device="{}", DeviceId="{}", Version="{}""#,
            esc(&self.client),
            esc(&self.device_name),
            esc(&self.device_id),
            esc(&self.version)
        );
        if let Some(t) = token {
            h.push_str(&format!(r#", Token="{}""#, esc(t)));
        }
        h
    }
}

/// Result of a successful login; the token goes to the OS keychain.
#[derive(Debug, Clone)]
pub struct Session {
    pub base_url: Url,
    pub token: String,
    pub user_id: String,
    pub user_name: String,
    pub is_admin: bool,
    pub server_id: String,
    pub server_name: String,
    pub version: Option<String>,
}

/// A user listed on the server's sign-in screen.
#[derive(Debug, Clone)]
pub struct PublicUser {
    pub id: String,
    pub name: String,
    /// Public picture URL (no token needed).
    pub avatar: Option<Url>,
    pub has_password: bool,
}

#[derive(Debug, Clone)]
pub struct Connector {
    http: Client,
    identity: ClientIdentity,
}

impl Connector {
    pub fn new(http: Client, identity: ClientIdentity) -> Self {
        Self { http, identity }
    }

    /// Normalises user input ("192.168.1.10:8096", "https://jf.example/jellyfin")
    /// and checks that a Jellyfin server answers there.
    pub async fn probe(&self, input: &str) -> Result<(Url, PublicSystemInfo)> {
        let input = input.trim().trim_end_matches('/');
        let candidates: Vec<String> = if input.starts_with("http://") || input.starts_with("https://") {
            vec![input.to_owned()]
        } else {
            vec![format!("https://{input}"), format!("http://{input}")]
        };
        let mut last = Error::Invalid(oneshot_core::codes::NET_ADDRESS.tag(format!("\"{input}\" is not a server address. Use something like https://my-server:8920.")));
        for c in candidates {
            let Ok(url) = Url::parse(&c) else { continue };
            match self.public_info(&url).await {
                Ok(info) => return Ok((url, info)),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    async fn public_info(&self, base: &Url) -> Result<PublicSystemInfo> {
        let resp = self
            .http
            .get(oneshot_net::join(base, "System/Info/Public")?)
            .send()
            .await
            .map_err(oneshot_net::map_err)?;
        let info: PublicSystemInfo = oneshot_net::json(resp).await?;
        if info.product_name.as_deref().is_some_and(|p| !p.contains("Jellyfin")) {
            return Err(Error::Protocol(oneshot_core::codes::SRV_NOT_JELLYFIN.tag(format!("{base} answered, but it is not a Jellyfin server."))));
        }
        Ok(info)
    }

    pub async fn login(&self, base: &Url, username: &str, password: &str) -> Result<Session> {
        let resp = self
            .http
            .post(oneshot_net::join(base, "Users/AuthenticateByName")?)
            .header("Authorization", self.identity.header(None))
            .json(&json!({ "Username": username, "Pw": password }))
            .send()
            .await
            .map_err(oneshot_net::map_err)?;
        let auth: AuthenticationResult = oneshot_net::json(resp).await?;
        self.session(base, auth).await
    }

    /// Users the server shows on its sign-in screen (no authentication).
    /// Users hidden from that screen are not listed.
    pub async fn public_users(&self, base: &Url) -> Result<Vec<PublicUser>> {
        let resp = self
            .http
            .get(oneshot_net::join(base, "Users/Public")?)
            .header("Authorization", self.identity.header(None))
            .send()
            .await
            .map_err(oneshot_net::map_err)?;
        let users: Vec<PublicUserDto> = oneshot_net::json(resp).await?;
        Ok(users
            .into_iter()
            .map(|u| {
                let avatar = u.primary_image_tag.as_deref().and_then(|tag| {
                    let mut url = oneshot_net::join(base, &format!("Users/{}/Images/Primary", u.id)).ok()?;
                    url.query_pairs_mut().append_pair("tag", tag).append_pair("maxHeight", "256");
                    Some(url)
                });
                PublicUser { avatar, id: u.id, name: u.name, has_password: u.has_password }
            })
            .collect())
    }

    /// Starts Quick Connect: show `code` to the user, then poll with `secret`.
    pub async fn quick_connect_start(&self, base: &Url) -> Result<QuickConnectResult> {
        let resp = self
            .http
            .post(oneshot_net::join(base, "QuickConnect/Initiate")?)
            .header("Authorization", self.identity.header(None))
            .send()
            .await
            .map_err(oneshot_net::map_err)?;
        match oneshot_net::json(resp).await {
            Err(Error::Unauthorized | Error::Forbidden(_)) => Err(Error::Unsupported(oneshot_core::codes::AUTH_QUICK_CONNECT.tag("Quick Connect is turned off on this Jellyfin server. Sign in with a user name and password."))),
            other => other,
        }
    }

    /// Returns `Some(session)` once the user approved the code on another device.
    pub async fn quick_connect_poll(&self, base: &Url, secret: &str) -> Result<Option<Session>> {
        let mut url = oneshot_net::join(base, "QuickConnect/Connect")?;
        url.query_pairs_mut().append_pair("secret", secret);
        let state: QuickConnectResult =
            oneshot_net::json(self.http.get(url).send().await.map_err(oneshot_net::map_err)?).await?;
        if !state.authenticated {
            return Ok(None);
        }
        let resp = self
            .http
            .post(oneshot_net::join(base, "Users/AuthenticateWithQuickConnect")?)
            .header("Authorization", self.identity.header(None))
            .json(&json!({ "Secret": secret }))
            .send()
            .await
            .map_err(oneshot_net::map_err)?;
        let auth: AuthenticationResult = oneshot_net::json(resp).await?;
        self.session(base, auth).await.map(Some)
    }

    async fn session(&self, base: &Url, auth: AuthenticationResult) -> Result<Session> {
        let info = self.public_info(base).await?;
        Ok(Session {
            base_url: base.clone(),
            token: auth.access_token,
            user_id: auth.user.id,
            user_name: auth.user.name,
            is_admin: auth.user.policy.is_some_and(|p| p.is_administrator),
            server_id: auth.server_id,
            server_name: info.server_name.unwrap_or_else(|| "Jellyfin".into()),
            version: info.version,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_format() {
        let id = ClientIdentity {
            client: "Flick".into(),
            device_name: "Desk\"top".into(),
            device_id: "abc".into(),
            version: "0.1.0".into(),
        };
        assert_eq!(
            id.header(Some("tok")),
            r#"MediaBrowser Client="Flick", Device="Desktop", DeviceId="abc", Version="0.1.0", Token="tok""#
        );
    }
}
