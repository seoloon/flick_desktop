//! A conversion in progress: the job, the relays around it, and the folder it writes to.

use std::net::IpAddr;
use std::path::PathBuf;

use oneshot_core::Result;
use url::Url;

use super::{Convert, Job};
use crate::proxy::Proxy;

#[derive(Debug)]
pub(crate) struct Session {
    ffmpeg: PathBuf,
    convert: Convert,
    /// ffmpeg reads the title here (loopback, the server's credentials stay in the relay).
    input: Url,
    _source: Proxy,
    /// The receiver pulls the playlist and segments here.
    files: Proxy,
    base: Url,
    dir: tempfile::TempDir,
    job: Option<Job>,
    generation: u32,
    base_ms: u64,
}

impl Session {
    pub async fn start(ffmpeg: PathBuf, convert: Convert, upstream: &Url, headers: Vec<(String, String)>, local: IpAddr, http: reqwest::Client, start_ms: u64) -> Result<Self> {
        let source = Proxy::default();
        let input = source.serve(upstream, headers, IpAddr::from([127, 0, 0, 1]), http).await?;
        let dir = tempfile::Builder::new().prefix("flick-airplay-").tempdir().map_err(|e| oneshot_core::Error::Storage(oneshot_core::codes::CAST_CONVERT_FAILED.tag(format!("Flick could not prepare a folder for the conversion ({e})."))))?;
        let files = Proxy::default();
        let base = files.serve_dir(dir.path().to_path_buf(), local).await?;
        let mut session = Self { ffmpeg, convert, input, _source: source, files, base, dir, job: None, generation: 0, base_ms: 0 };
        session.begin(start_ms).await?;
        Ok(session)
    }

    async fn begin(&mut self, ms: u64) -> Result<()> {
        self.job = None; // the previous ffmpeg is killed
        self.generation += 1;
        let out = self.dir.path().join(self.generation.to_string());
        self.base_ms = ms;
        self.job = Some(Job::start(&self.ffmpeg, &self.convert, self.input.as_str(), ms, &out).await?);
        Ok(())
    }

    /// Converts again from `ms`: a new folder, a new playlist.
    pub async fn restart_at(&mut self, ms: u64) -> Result<()> {
        let old = self.dir.path().join(self.generation.to_string());
        let result = self.begin(ms).await;
        let _ = std::fs::remove_dir_all(old);
        result
    }

    pub fn playlist_url(&self) -> Url {
        self.base.join(&format!("{}/{}", self.generation, super::PLAYLIST)).unwrap_or_else(|_| self.base.clone())
    }

    /// Where in the title the current playlist begins.
    pub fn base_ms(&self) -> u64 {
        self.base_ms
    }

    pub fn produced_ms(&self) -> u64 {
        self.job.as_ref().map_or(0, Job::produced_ms)
    }

    pub fn finished(&mut self) -> bool {
        self.job.as_mut().is_none_or(Job::finished)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.job = None;
        self.files.stop();
    }
}
