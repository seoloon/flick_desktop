//! Offline downloads in the app: the seam between the FlickDD engine and the
//! rest of Flick. The engine does the transfer; this finds the server (the
//! Flick Server invitation link), signs in, keeps what is needed to show a
//! download without its server (metadata and artwork, see [`crate::offline`]),
//! and tells the UI what changed.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use oneshot_core::ids::ItemRef;
use oneshot_core::media::{ImageKind, ItemKind, MediaItem};
use oneshot_core::server::ProviderKind;
use oneshot_core::{Error, Result};
use oneshot_flickdd::api::{Failure, Link};
use oneshot_flickdd::{Backend, Connection, Event, Item, Kind, Manager, NewDownload, Source};
use oneshot_flickserver::{mint_token, key::now_unix};
use oneshot_storage::secrets;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager as _};

use crate::flickserver::stored_invitation;
use crate::flicksync::Hub;
use crate::offline::{self, IMAGE_KINDS, LocalLibrary};
use crate::state::AppState;

const EVENT: &str = "downloads";
/// Episodes queued by one "Download" on a series, so a mistaken click cannot flood the queue.
const MAX_EPISODES: usize = 400;
/// Titles whose metadata and artwork are fetched at once.
const CAPTURE_PARALLEL: usize = 4;

/// Downloads live in the app's own data folder.
pub fn default_directory(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir().unwrap_or_else(|_| PathBuf::from(".")).join("downloads")
}

struct AppSource {
    app: AppHandle,
    dir: PathBuf,
}

#[async_trait]
impl Source for AppSource {
    async fn connect(&self) -> std::result::Result<Connection, Failure> {
        let not_configured = || Failure::NotConfigured("Downloads need a Flick Server invitation link: add one in Settings › Flick Server.".into());
        let state = self.app.state::<Arc<AppState>>();
        let invitation = stored_invitation().ok().flatten().ok_or_else(not_configured)?;
        let who = Hub::identity(&state);
        let base = invitation.base_url();
        let jwt = mint_token(&invitation.key, &who, 3600, now_unix());
        Ok(Connection { link: Link { http: state.http(), base }, jwt })
    }

    fn directory(&self) -> PathBuf {
        self.dir.clone()
    }

    fn seal_key(&self) -> Option<[u8; 32]> {
        match master_key() {
            Ok(key) => Some(key),
            Err(e) => {
                tracing::warn!(target: "downloads", "downloads cannot be encrypted: {e}");
                None
            }
        }
    }
}

/// Entry of the downloads' master key in the credential vault.
const KEY_ENTRY: &str = "downloads:key";

/// The key every download's own key comes from. It lives in the same vault
/// entry as the other secrets, so the system asks for the password once for
/// all of them. Created on first use; the key is lost with the vault, and
/// then so are the files it sealed (they could not be played anyway).
fn master_key() -> Result<[u8; 32]> {
    if let Some(hex) = secrets::load_secret(KEY_ENTRY)? {
        if let Some(key) = parse_key(&hex) {
            return Ok(key);
        }
        tracing::warn!(target: "downloads", "the stored downloads key is damaged: a new one replaces it");
    }
    let mut key = [0u8; 32];
    getrandom::fill(&mut key).map_err(|e| Error::Storage(format!("no randomness for the downloads key: {e}")))?;
    secrets::store_secret(KEY_ENTRY, &key.iter().map(|b| format!("{b:02x}")).collect::<String>())?;
    Ok(key)
}

fn parse_key(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.is_ascii() {
        return None;
    }
    let mut key = [0u8; 32];
    for (i, b) in key.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(key)
}

pub struct Downloads {
    manager: Arc<Manager>,
    dir: PathBuf,
    library: Arc<LocalLibrary>,
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
        let dir = default_directory(&app);
        let emitter = app.clone();
        let sink: oneshot_flickdd::EventSink = Arc::new(move |e| {
            let ui = match e {
                Event::Changed(item) => UiEvent::Changed { item },
                Event::Removed(id) => UiEvent::Removed { id },
            };
            let _ = emitter.emit(EVENT, &ui);
        });
        let tauri::async_runtime::RuntimeHandle::Tokio(rt) = tauri::async_runtime::handle();
        let source = Arc::new(AppSource { app, dir: dir.clone() });
        let manager = Manager::open(source, config_dir.join("downloads.json"), sink, rt);
        let library = Arc::new(LocalLibrary::new(Arc::clone(&manager), dir.clone()));
        Self { manager, dir, library }
    }

    /// Resumes what was left unfinished.
    pub fn start(&self) {
        self.manager.start();
    }

    pub fn manager(&self) -> &Arc<Manager> {
        &self.manager
    }

    pub fn library(&self) -> Arc<LocalLibrary> {
        Arc::clone(&self.library)
    }

    pub fn directory(&self) -> &Path {
        &self.dir
    }

    /// The stored picture of a download.
    pub async fn local_image(&self, id: &str, kind: ImageKind) -> Result<Vec<u8>> {
        offline::local_image(&self.dir, id, kind).await
    }

    /// Queues a movie or an episode; a season or a series queues each of its episodes.
    pub async fn enqueue(&self, st: &Arc<AppState>, id: &ItemRef) -> Result<Vec<Item>> {
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
        let mut captures = Vec::with_capacity(episodes.len());
        for e in &episodes {
            let download = self.manager.enqueue(&request(st, e)?);
            captures.push((download.id.clone(), e.id.clone()));
            queued.push(download);
        }
        // What the library needs to show them offline is fetched while they download.
        let (st, dir) = (Arc::clone(st), self.dir.clone());
        tauri::async_runtime::spawn(async move {
            futures::stream::iter(captures)
                .for_each_concurrent(CAPTURE_PARALLEL, |(download, item)| {
                    let (st, dir) = (Arc::clone(&st), dir.clone());
                    async move {
                        if let Err(e) = capture(&st, &dir, &download, &item).await {
                            tracing::warn!(target: "downloads", "could not keep the details of a download: {e}");
                        }
                    }
                })
                .await;
        });
        Ok(queued)
    }

    /// Cancels a download or forgets a finished one, with what was kept to show it.
    pub fn remove(&self, st: &AppState, id: &str, delete_file: bool) {
        self.manager.remove(id, delete_file);
        forget(&self.dir, id);
        st.catalog.invalidate_item(&ItemRef::new(offline::LOCAL_SERVER, id));
    }

    /// Everything: queue, files, details and artwork.
    pub fn clear(&self, st: &AppState) {
        let ids: Vec<String> = self.manager.list().into_iter().map(|d| d.id).collect();
        self.manager.clear();
        for id in &ids {
            st.catalog.invalidate_item(&ItemRef::new(offline::LOCAL_SERVER, id));
        }
        // Stray files too (partials of an interrupted run, pictures of a download that was never recorded).
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
            }
        }
    }
}

/// Deletes the snapshot and pictures of a download.
fn forget(dir: &Path, id: &str) {
    let _ = std::fs::remove_file(offline::snapshot_path(dir, id));
    for (kind, _, _) in IMAGE_KINDS {
        let _ = std::fs::remove_file(offline::image_path(dir, id, kind));
    }
}

/// Keeps the title's details and artwork next to its download, so the library can show it offline.
async fn capture(st: &AppState, dir: &Path, download: &str, item: &ItemRef) -> Result<()> {
    // The detail fetch (the list of a season has no technical sources).
    let media = st.catalog.item(item).await?;
    tokio::fs::create_dir_all(offline::meta_dir(dir)).await.map_err(|e| Error::Storage(e.to_string()))?;
    for (kind, _, size) in IMAGE_KINDS {
        let reference = match kind {
            ImageKind::Poster => &media.images.poster,
            ImageKind::Backdrop => &media.images.backdrop,
            ImageKind::Thumb => &media.images.thumb,
            ImageKind::Logo => &media.images.logo,
            ImageKind::Banner => &None,
        };
        let Some(reference) = reference else { continue };
        match crate::images::load(st, reference, size).await {
            Ok(bytes) => {
                let _ = tokio::fs::write(offline::image_path(dir, download, kind), bytes).await;
            }
            Err(e) => tracing::debug!(target: "downloads", "no {kind:?} kept: {e}"),
        }
    }
    let json = serde_json::to_vec(&media).map_err(|e| Error::Storage(e.to_string()))?;
    tokio::fs::write(offline::snapshot_path(dir, download), json).await.map_err(|e| Error::Storage(e.to_string()))
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

#[cfg(test)]
mod tests {
    use super::parse_key;

    #[test]
    fn a_key_round_trips_through_hex_and_damage_is_refused() {
        let key: [u8; 32] = std::array::from_fn(|i| (i * 9) as u8);
        let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(parse_key(&hex), Some(key));
        assert_eq!(parse_key(&hex[..62]), None);
        assert_eq!(parse_key(&"zz".repeat(32)), None);
        assert_eq!(parse_key(&"é".repeat(32)), None);
    }
}
