//! Debug builds only: connects local test servers from environment variables
//! so the app can be exercised end-to-end without typing credentials.
//!
//! * `ONESHOT_DEV_JELLYFIN=http://localhost:18096|user|password`
//! * `ONESHOT_DEV_PLEX=http://localhost:32401` (unclaimed server, no token)
//!
//! See `tools/dev-jellyfin.sh` and `tools/dev-plex.sh`.

use std::sync::Arc;

use oneshot_core::ServerId;
use oneshot_core::server::{ProviderKind, ServerDescriptor, UserProfile};
use url::Url;

use crate::state::AppState;

pub async fn bootstrap(state: Arc<AppState>) {
    let known = |kind: ProviderKind, url: &Url| state.servers.read().iter().any(|s| s.kind == kind && &s.base_url == url);

    if let Ok(spec) = std::env::var("ONESHOT_DEV_JELLYFIN") {
        let mut parts = spec.splitn(3, '|');
        if let (Some(url), Some(user), Some(pass)) = (parts.next(), parts.next(), parts.next())
            && let Ok(url) = Url::parse(url)
            && !known(ProviderKind::Jellyfin, &url)
        {
            let connector = oneshot_jellyfin::Connector::new(state.http(), state.jellyfin_identity());
            match connector.login(&url, user, pass).await {
                Ok(s) => {
                    let d = ServerDescriptor {
                        id: ServerId::new(),
                        kind: ProviderKind::Jellyfin,
                        name: s.server_name.clone(),
                        remote_id: s.server_id.clone(),
                        base_url: s.base_url.clone(),
                        alternate_urls: vec![],
                        version: s.version.clone(),
                        user: UserProfile { id: s.user_id.clone(), name: s.user_name.clone(), avatar: None, is_admin: s.is_admin },
                    };
                    if let Err(e) = state.register_server(d, &s.token) {
                        tracing::error!("dev bootstrap (jellyfin): {e}");
                    }
                }
                Err(e) => tracing::error!("dev bootstrap (jellyfin): {e}"),
            }
        }
    }

    if let Ok(url) = std::env::var("ONESHOT_DEV_PLEX")
        && let Ok(url) = Url::parse(&url)
        && !known(ProviderKind::Plex, &url)
    {
        let d = ServerDescriptor {
            id: ServerId::new(),
            kind: ProviderKind::Plex,
            name: "Plex (dev)".into(),
            remote_id: "dev".into(),
            base_url: url,
            alternate_urls: vec![],
            version: None,
            user: UserProfile { id: "1".into(), name: "owner".into(), avatar: None, is_admin: true },
        };
        if let Err(e) = state.register_server(d, "") {
            tracing::error!("dev bootstrap (plex): {e}");
        }
    }
}
