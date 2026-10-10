//! One running ffmpeg: it writes the playlist and segments into a folder until the title ends or it is dropped.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use oneshot_core::codes;
use oneshot_core::{Error, Result};
use parking_lot::Mutex;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use super::{Convert, PLAYLIST, args};

/// How long ffmpeg has to open the title and finish its first segment.
const FIRST_SEGMENT: Duration = Duration::from_secs(45);

pub struct Job {
    child: Child,
    dir: PathBuf,
}

/// The length of the segments a playlist lists.
pub(crate) fn playlist_ms(playlist: &str) -> u64 {
    let secs: f64 = playlist.lines().filter_map(|l| l.strip_prefix("#EXTINF:")).filter_map(|v| v.trim_end_matches(',').split(',').next()?.parse::<f64>().ok()).sum();
    (secs * 1000.0) as u64
}

impl Job {
    /// Starts ffmpeg on `input` from `start_ms`, and returns once the first segment is on disk.
    pub async fn start(ffmpeg: &Path, convert: &Convert, input: &str, start_ms: u64, dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir).map_err(|e| Error::Storage(codes::CAST_CONVERT_FAILED.tag(format!("Flick could not prepare a folder for the conversion ({e})."))))?;
        let mut cmd = Command::new(ffmpeg);
        cmd.args(args(convert, input, start_ms, dir)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashing on screen
        let mut child = cmd.spawn().map_err(|e| Error::Playback(codes::CAST_FFMPEG_MISSING.tag(format!("ffmpeg could not be started ({e}). Reinstall Flick."))))?;

        // ffmpeg's own words go to the log; the last lines explain a failure.
        let tail = Arc::new(Mutex::new(String::new()));
        if let Some(stderr) = child.stderr.take() {
            let tail = Arc::clone(&tail);
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(target: "cast", "ffmpeg: {line}");
                    let mut t = tail.lock();
                    if t.len() > 600 {
                        t.clear();
                    }
                    t.push_str(&line);
                    t.push('\n');
                }
            });
        }

        let mut job = Self { child, dir: dir.to_path_buf() };
        let started = tokio::time::Instant::now();
        loop {
            if job.produced_ms() > 0 {
                return Ok(job);
            }
            if job.child.try_wait().ok().flatten().is_some() {
                // Let the stderr reader take the last lines.
                tokio::time::sleep(Duration::from_millis(100)).await;
                tracing::warn!(target: "cast", "ffmpeg stopped before its first segment: {}", tail.lock());
                return Err(Error::Playback(codes::CAST_CONVERT_FAILED.tag("The conversion of the video for AirPlay stopped before it began. Try another version of the title.")));
            }
            if started.elapsed() > FIRST_SEGMENT {
                return Err(Error::Playback(codes::CAST_CONVERT_SLOW.tag("The conversion of the video for AirPlay did not start in time. Try again, or watch it in Flick.")));
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }

    /// How much of the title is ready to be played (the playlist's length).
    pub fn produced_ms(&self) -> u64 {
        std::fs::read_to_string(self.dir.join(PLAYLIST)).map(|p| playlist_ms(&p)).unwrap_or(0)
    }

    /// ffmpeg has written the whole title.
    pub fn finished(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcode::{AudioPlan, Convert, VideoPlan};

    fn convert() -> Convert {
        Convert { video_pos: 0, audio_pos: None, video: VideoPlan::H264, hevc: false, audio: AudioPlan::Copy, burn: None, deinterlace: false, scale_down: false }
    }

    #[test]
    fn the_playlist_length_is_the_sum_of_its_segments() {
        let list = "#EXTM3U\n#EXT-X-VERSION:7\n#EXTINF:6.000000,\nseg00000.m4s\n#EXTINF:5.500,\nseg00001.m4s\n";
        assert_eq!(playlist_ms(list), 11_500);
        assert_eq!(playlist_ms("#EXTM3U\n"), 0);
    }

    #[tokio::test]
    async fn a_bad_input_fails_with_the_conversion_code_and_no_hang() {
        let Some(ffmpeg) = crate::transcode::locate(&[]) else { return };
        let dir = tempfile::tempdir().unwrap();
        let err = match Job::start(&ffmpeg, &convert(), "http://127.0.0.1:1/none.mkv", 0, dir.path()).await {
            Ok(_) => panic!("started on a missing input"),
            Err(e) => e,
        };
        assert_eq!(err.code(), oneshot_core::codes::CAST_CONVERT_FAILED.id);
    }

    #[tokio::test]
    async fn a_real_clip_gets_a_playlist_and_the_process_dies_with_the_job() {
        let Some(ffmpeg) = crate::transcode::locate(&[]) else { return };
        let work = tempfile::tempdir().unwrap();
        let clip = work.path().join("clip.mp4");
        let made = std::process::Command::new(&ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=24:duration=14", "-f", "lavfi", "-i", "sine=frequency=440:duration=14", "-c:v", "libx264", "-c:a", "aac", "-shortest"])
            .arg(&clip)
            .status();
        if !made.is_ok_and(|s| s.success()) {
            return; // an ffmpeg without libx264
        }
        let out = work.path().join("out");
        let c = Convert { video: VideoPlan::Copy, audio_pos: Some(0), ..convert() };
        let job = Job::start(&ffmpeg, &c, clip.to_str().unwrap(), 0, &out).await.unwrap();
        assert!(out.join("index.m3u8").exists() && out.join("init.mp4").exists());
        assert!(job.produced_ms() >= 5_000);
        drop(job);
    }
}
