//! Offline downloads in the app: the seam between the FlickDD engine and the
//! rest of Flick. The engine does the transfer; this finds the server (the
//! FlickSync invitation link), signs in, and tells the UI what changed.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use oneshot_core::ids::ItemRef;
use oneshot_core::media::{ItemKind, MediaItem};
use oneshot_core::server::ProviderKind;
use oneshot_core::{Error, Result};
use oneshot_flickdd::api::{Failure, Link};
use oneshot_flickdd::{Backend, Connection, Event, Item, Kind, Manager, NewDownload, Source};
use oneshot_flicksync::auth::{LocalKeyTokenProvider, TokenProvider};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager as _};

use crate::flicksync::{Hub, stored_invitation};
use crate::state::AppState;

const EVENT: &str = "downloads";
/// Episodes queued by one "Download" on a series, so a mistaken click cannot flood the queue.
const MAX_EPISODES: usize = 400;

pub fn default_directory(app: &AppHandle) -> PathBuf {
    app.path().download_dir().or_else(|_| app.path().document_dir()).unwrap_or_else(|_| PathBuf::from(".")).join("Flick")
}

struct AppSource {
    app: AppHandle,
}

#[async_trait]
impl Source for AppSource {
    async fn connect(&self) -> std::result::Result<Connection, Failure> {
        let not_configured = || {
            Failure::NotConfigured("Downloads need a Flick Server invitation link: add one in Settings › Watch Together.".into())
        };
        let state = self.app.state::<Arc<AppState>>();
        let invitation = stored_invitation().ok().flatten().ok_or_else(not_configured)?;
        let who = Hub::identity(&state);
        let base = invitation.base_url();
        let jwt = LocalKeyTokenProvider::new(invitation.key, who).token().await.map_err(|_| not_configured())?;
        Ok(Connection { link: Link { http: state.http(), base }, jwt })
    }

    fn directory(&self) -> PathBuf {
        default_directory(&self.app)
    }
}

pub struct Downloads {
    manager: Arc<Manager>,
}

impl std::fmt::Debug for Downloads {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Downloads").finish_non_exhaustive()
    }
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum UiEvent {
    Changed { item: Box<Item> },
    Removed { id: String },
}

impl Downloads {
    pub fn new(app: AppHandle, config_dir: PathBuf) -> Self {
        let emitter = app.clone();
        let sink: oneshot_flickdd::EventSink = Arc::new(move |e| {
            let ui = match e {
                Event::Changed(item) => UiEvent::Changed { item },
                Event::Removed(id) => UiEvent::Removed { id },
            };
            let _ = emitter.emit(EVENT, &ui);
        });
        let tauri::async_runtime::RuntimeHandle::Tokio(rt) = tauri::async_runtime::handle();
        let source = Arc::new(AppSource { app });
        Self { manager: Manager::open(source, config_dir.join("downloads.json"), sink, rt) }
    }

    /// Resumes what was left unfinished.
    pub fn start(&self) {
        self.manager.start();
    }

    pub fn manager(&self) -> &Arc<Manager> {
        &self.manager
    }

    /// Queues a movie or an episode; a season or a series queues each of its episodes.
    pub async fn enqueue(&self, st: &AppState, id: &ItemRef) -> Result<Vec<Item>> {
        let item = st.catalog.item(id).await?;
        let mut episodes: Vec<MediaItem> = Vec::new();
        match item.kind {
            ItemKind::Movie | ItemKind::Episode => episodes.push(item),
            ItemKind::Season => episodes = st.catalog.children(id, ItemKind::Season).await?,
            ItemKind::Series => {
                for season in st.catalog.children(id, ItemKind::Series).await? {
                    episodes.extend(st.catalog.children(&season.id, ItemKind::Season).await?);
                }
            }
            _ => return Err(Error::Invalid("Only movies and episodes can be downloaded.".into())),
        }
        episodes.retain(|e| matches!(e.kind, ItemKind::Movie | ItemKind::Episode));
        episodes.truncate(MAX_EPISODES);
        if episodes.is_empty() {
            return Err(Error::Invalid("There is nothing to download here.".into()));
        }
        let mut queued = Vec::with_capacity(episodes.len());
        for e in &episodes {
            queued.push(self.manager.enqueue(&request(st, e)?));
        }
        Ok(queued)
    }
}

fn request(st: &AppState, item: &MediaItem) -> Result<NewDownload> {
    let desc = st.servers.read().iter().find(|s| s.id == item.id.server).cloned().ok_or_else(|| Error::NotFound("server".into()))?;
    let backend = match desc.kind {
        ProviderKind::Jellyfin => Backend::Jellyfin,
        ProviderKind::Plex => Backend::Plex,
    };
    let (title, subtitle, kind) = match item.kind {
        ItemKind::Episode => {
            let e = item.episode.as_ref();
            let label = match (e.and_then(|e| e.season_number), e.and_then(|e| e.episode_number)) {
                (Some(s), Some(n)) => format!("S{s:02}E{n:02} · {}", item.title),
                _ => item.title.clone(),
            };
            let series = e.and_then(|e| e.series_title.clone()).unwrap_or_else(|| item.title.clone());
            (series, Some(label), Kind::Episode)
        }
        _ => (item.title.clone(), item.year.map(|y| y.to_string()), Kind::Movie),
    };
    Ok(NewDownload { backend, item_id: item.id.key.clone(), item_ref: item.id.to_string(), title, subtitle, kind: Some(kind) })
}
