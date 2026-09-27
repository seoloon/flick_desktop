//! Administration surface. Exposed only when the user is a server
//! administrator; every call is still authorised by the server.

use async_trait::async_trait;
use oneshot_core::Result;
use oneshot_core::ids::ItemRef;
use oneshot_core::provider::{AdminProvider, AdminServerInfo, AdminSession, AdminTask, AdminUser};
use oneshot_net::reqwest::Method;

use crate::dto::{LogFile, SessionInfo, SystemInfo, TaskInfo, UserDto};
use crate::provider::JellyfinProvider;

#[async_trait]
impl AdminProvider for JellyfinProvider {
    async fn server_info(&self) -> Result<AdminServerInfo> {
        let i: SystemInfo = self.get("System/Info", &[]).await?;
        let mut extra = Vec::new();
        if let Some(a) = i.local_address {
            extra.push(("Local address".into(), a));
        }
        if let Some(r) = i.has_pending_restart {
            extra.push(("Restart pending".into(), r.to_string()));
        }
        if let Some(e) = i.encoder_location {
            extra.push(("FFmpeg".into(), e));
        }
        Ok(AdminServerInfo {
            name: i.server_name.unwrap_or_default(),
            version: i.version.unwrap_or_default(),
            os: i.operating_system_display_name,
            update_available: i.has_update_available,
            transcode_hw_accel: None,
            extra,
        })
    }

    async fn sessions(&self) -> Result<Vec<AdminSession>> {
        let s: Vec<SessionInfo> = self.get("Sessions", &[("activeWithinSeconds", "960".into())]).await?;
        Ok(s.into_iter()
            .map(|s| AdminSession {
                id: s.id,
                user: s.user_name.unwrap_or_default(),
                client: s.client.unwrap_or_default(),
                device: s.device_name.unwrap_or_default(),
                title: s.now_playing_item.and_then(|n| n.name),
                state: s.play_state.as_ref().map(|p| if p.is_paused { "Paused".into() } else { p.play_method.clone().unwrap_or_else(|| "Idle".into()) }),
                transcoding: Some(s.transcoding_info.is_some()),
            })
            .collect())
    }

    async fn users(&self) -> Result<Vec<AdminUser>> {
        let u: Vec<UserDto> = self.get("Users", &[]).await?;
        Ok(u.into_iter()
            .map(|u| {
                let policy = u.policy.unwrap_or_default();
                AdminUser {
                    id: u.id,
                    name: u.name,
                    is_admin: policy.is_administrator,
                    is_disabled: policy.is_disabled,
                    last_activity: u.last_activity_date,
                }
            })
            .collect())
    }

    async fn tasks(&self) -> Result<Vec<AdminTask>> {
        let t: Vec<TaskInfo> = self.get("ScheduledTasks", &[("isHidden", "false".into())]).await?;
        Ok(t.into_iter()
            .map(|t| AdminTask {
                id: t.id,
                name: t.name,
                category: t.category,
                state: t.state,
                progress: t.current_progress_percentage,
                last_result: t.last_execution_result.and_then(|r| r.status),
            })
            .collect())
    }

    async fn run_task(&self, id: &str) -> Result<()> {
        self.send_empty(Method::POST, &format!("ScheduledTasks/Running/{id}"), &[]).await
    }

    async fn scan_library(&self, library: &ItemRef) -> Result<()> {
        self.send_empty(
            Method::POST,
            &format!("Items/{}/Refresh", library.key),
            &[("Recursive", "true".into()), ("MetadataRefreshMode", "Default".into())],
        )
        .await
    }

    async fn logs(&self) -> Result<Vec<String>> {
        let files: Vec<LogFile> = self.get("System/Logs", &[]).await?;
        Ok(files.into_iter().map(|f| f.name).collect())
    }
}
