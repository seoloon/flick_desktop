//! Admin surface for servers owned by the signed-in account. Only endpoints
//! exposed by PMS itself are used; anything else is reported unsupported.

use async_trait::async_trait;
use oneshot_core::ids::ItemRef;
use oneshot_core::provider::{AdminProvider, AdminServerInfo, AdminSession, AdminTask, AdminUser};
use oneshot_core::{Error, Result};
use oneshot_net::reqwest::Method;
use serde::Deserialize;

use crate::dto::{ButlerContainer, Envelope, SessionsContainer};
use crate::provider::PlexProvider;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Root {
    friendly_name: Option<String>,
    version: Option<String>,
    platform: Option<String>,
    platform_version: Option<String>,
    transcoder_active_video_sessions: Option<u32>,
    #[serde(default)]
    my_plex_subscription: bool,
}

#[derive(Debug, Deserialize)]
struct Accounts {
    #[serde(rename = "Account", default)]
    accounts: Vec<Account>,
}

#[derive(Debug, Deserialize)]
struct Account {
    id: i64,
    name: Option<String>,
}

#[async_trait]
impl AdminProvider for PlexProvider {
    async fn server_info(&self) -> Result<AdminServerInfo> {
        let r: Envelope<Root> = self.get("", &[]).await?;
        let r = r.container;
        let mut extra = vec![("Plex Pass".into(), r.my_plex_subscription.to_string())];
        if let Some(n) = r.transcoder_active_video_sessions {
            extra.push(("Active video transcodes".into(), n.to_string()));
        }
        Ok(AdminServerInfo {
            name: r.friendly_name.unwrap_or_default(),
            version: r.version.unwrap_or_default(),
            os: r.platform.map(|p| format!("{p} {}", r.platform_version.unwrap_or_default())),
            update_available: None,
            transcode_hw_accel: None,
            extra,
        })
    }

    async fn sessions(&self) -> Result<Vec<AdminSession>> {
        let s: Envelope<SessionsContainer> = self.get("status/sessions", &[]).await?;
        Ok(s.container
            .metadata
            .into_iter()
            .map(|m| AdminSession {
                id: m.session.map(|s| s.id).unwrap_or_default(),
                user: m.user.map(|u| u.tag).unwrap_or_default(),
                client: m.player.as_ref().and_then(|p| p.product.clone()).unwrap_or_default(),
                device: m.player.as_ref().and_then(|p| p.title.clone()).unwrap_or_default(),
                title: Some(match m.grandparent_title {
                    Some(g) => format!("{g} — {}", m.title.unwrap_or_default()),
                    None => m.title.unwrap_or_default(),
                }),
                state: m.player.and_then(|p| p.state),
                transcoding: Some(m.transcode.is_some()),
            })
            .collect())
    }

    async fn users(&self) -> Result<Vec<AdminUser>> {
        let a: Envelope<Accounts> = self.get("accounts", &[]).await?;
        Ok(a.container
            .accounts
            .into_iter()
            .filter(|a| a.id > 0)
            .map(|a| AdminUser {
                id: a.id.to_string(),
                name: a.name.unwrap_or_default(),
                // PMS's local account list does not expose roles; the owner is id 1.
                is_admin: a.id == 1,
                is_disabled: false,
                last_activity: None,
            })
            .collect())
    }

    async fn tasks(&self) -> Result<Vec<AdminTask>> {
        let b: Envelope<ButlerContainer> = self.get("butler", &[]).await?;
        Ok(b.container
            .tasks
            .map(|t| t.tasks)
            .unwrap_or_default()
            .into_iter()
            .map(|t| AdminTask {
                id: t.name.clone(),
                name: t.title.unwrap_or(t.name),
                category: Some("Butler".into()),
                state: if t.enabled.unwrap_or(false) { "Scheduled".into() } else { "Disabled".into() },
                progress: None,
                last_result: None,
            })
            .collect())
    }

    async fn run_task(&self, id: &str) -> Result<()> {
        self.send_empty(Method::POST, &format!("butler/{id}"), &[]).await
    }

    async fn scan_library(&self, library: &ItemRef) -> Result<()> {
        let section = library
            .key
            .strip_prefix("section:")
            .ok_or_else(|| Error::Invalid("not a Plex library section".into()))?;
        self.send_empty(Method::GET, &format!("library/sections/{section}/refresh"), &[]).await
    }

    async fn logs(&self) -> Result<Vec<String>> {
        Err(Error::Unsupported("log browsing (PMS only offers a zipped diagnostics download)".into()))
    }
}
