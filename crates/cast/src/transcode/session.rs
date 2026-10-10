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

/// Where a file that sits in the title's folder on the server (a subtitle the server extracted) is on the
/// relay that serves the title, which adds the server's credentials. `None` for a file anywhere else.
pub(crate) fn relay_file(relayed: &Url, upstream: &Url, file: &Url) -> Option<Url> {
    let folder = |path: &str| path.rfind('/').map_or(String::new(), |i| path[..=i].to_owned());
    if (file.scheme(), file.host_str(), file.port_or_known_default()) != (upstream.scheme(), upstream.host_str(), upstream.port_or_known_default()) {
        return None;
    }
    let rest = file.path().strip_prefix(&folder(upstream.path()))?;
    let mut url = relayed.clone();
    url.set_path(&format!("{}{rest}", folder(relayed.path())));
    url.set_query(file.query());
    Some(url)
}

impl Session {
    /// `subtitle_file`: a text subtitle on the server, in the title's folder, for the conversion to burn in.
    #[allow(clippy::too_many_arguments)]
    pub async fn start(ffmpeg: PathBuf, mut convert: Convert, upstream: &Url, subtitle_file: Option<&Url>, headers: Vec<(String, String)>, local: IpAddr, http: reqwest::Client, start_ms: u64) -> Result<Self> {
        let source = Proxy::default();
        let input = source.serve(upstream, headers, IpAddr::from([127, 0, 0, 1]), http).await?;
        if let Some(burn) = convert.burn.as_mut().filter(|b| !b.bitmap) {
            burn.file = subtitle_file.and_then(|file| relay_file(&input, upstream, file)).map(String::from);
        }
        // No file for a text subtitle: none is burnt, rather than reading the whole title to find it.
        if convert.burn.as_ref().is_some_and(|b| !b.bitmap && b.file.is_none()) {
            convert.burn = None;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn a_file_next_to_the_title_on_the_server_is_reached_through_the_same_relay() {
        let upstream = url("https://media.example/jf/Videos/abc/stream?static=true&api_key=K");
        let relayed = url("http://127.0.0.1:5000/c/tok/stream");
        let subtitle = url("https://media.example/jf/Videos/abc/src1/Subtitles/3/0/Stream.srt");
        assert_eq!(relay_file(&relayed, &upstream, &subtitle).unwrap().as_str(), "http://127.0.0.1:5000/c/tok/src1/Subtitles/3/0/Stream.srt");
    }

    #[test]
    fn a_file_elsewhere_or_on_another_host_is_not_relayed() {
        let upstream = url("https://media.example/jf/Videos/abc/stream");
        let relayed = url("http://127.0.0.1:5000/c/tok/stream");
        assert!(relay_file(&relayed, &upstream, &url("https://media.example/jf/Other/x.srt")).is_none());
        assert!(relay_file(&relayed, &upstream, &url("https://elsewhere.example/jf/Videos/abc/x.srt")).is_none());
    }
}
