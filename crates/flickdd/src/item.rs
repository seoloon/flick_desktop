//! One download, as it is persisted and shown.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Jellyfin,
    Plex,
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jellyfin => "jellyfin",
            Self::Plex => "plex",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Movie,
    Episode,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Episode => "episode",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Waiting for a local slot.
    Queued,
    /// A worker is on it (it may be waiting to retry: see `note`).
    Active,
    /// Stopped: by the user, or after repeated failures (see `error`).
    Paused,
    Done,
    /// Cannot be downloaded at all (see `error`).
    Failed,
}

/// What a caller gives to start a download.
#[derive(Debug, Clone)]
pub struct NewDownload {
    pub backend: Backend,
    /// The backend's own id of the movie or episode.
    pub item_id: String,
    /// The app's reference to the same item, to link a download back to the library.
    pub item_ref: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub kind: Option<Kind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub backend: Backend,
    pub item_id: String,
    pub item_ref: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub kind: Option<Kind>,
    pub state: State,
    /// Bytes safely written to `temp_path` (never more than are on disk).
    pub offset: u64,
    /// Identity of the last grant: a new grant with another size or ETag means the file changed.
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub mime: Option<String>,
    pub temp_path: PathBuf,
    #[serde(default)]
    pub final_path: Option<PathBuf>,
    /// The file on disk is encrypted (see [`crate::seal`]). Fixed when the
    /// download is created: files from before stay as they are.
    #[serde(default)]
    pub sealed: bool,
    /// Why it is paused or failed, in a sentence for the user.
    #[serde(default)]
    pub error: Option<String>,
    /// What it is doing when that is not "downloading" (waiting, retrying).
    #[serde(default)]
    pub note: Option<String>,
    /// Bytes per second, while downloading.
    #[serde(default)]
    pub speed: Option<u64>,
    /// A finished download whose file is gone.
    #[serde(default, skip_deserializing)]
    pub missing: bool,
    pub created_ms: i64,
}

impl Item {
    pub fn new(req: &NewDownload, temp_path: PathBuf, now_ms: i64) -> Self {
        Self {
            id: uuid::Uuid::new_v4().simple().to_string(),
            backend: req.backend,
            item_id: req.item_id.clone(),
            item_ref: req.item_ref.clone(),
            title: req.title.clone(),
            subtitle: req.subtitle.clone(),
            kind: req.kind,
            state: State::Queued,
            offset: 0,
            size: None,
            etag: None,
            filename: None,
            mime: None,
            temp_path,
            final_path: None,
            sealed: false,
            error: None,
            note: None,
            speed: None,
            missing: false,
            created_ms: now_ms,
        }
    }
}
