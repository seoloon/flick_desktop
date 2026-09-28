//! mpv core lifecycle and the event thread.

use std::sync::Arc;
use std::time::{Duration, Instant};

use oneshot_mpv::{Api, EndReason, Event, Mpv, Node};
use tokio::sync::mpsc::UnboundedSender;

use crate::presenter::Presenter;

/// Properties mirrored into the player state.
pub(crate) const OBSERVED: &[&str] = &[
    "time-pos",
    "duration",
    "pause",
    "paused-for-cache",
    "seeking",
    "eof-reached",
    "volume",
    "mute",
    "track-list",
    "demuxer-cache-time",
    "video-params",
    "hwdec-current",
    "audio-out-params",
    "chapter-list",
];

/// Minimum interval between forwarded `time-pos` updates. mpv reports it
/// every frame; the UI interpolates between updates.
const POSITION_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone)]
pub(crate) enum EngineEvent {
    Property { name: String, value: Node },
    FileLoaded,
    PlaybackRestart,
    VideoReconfig,
    AudioReconfig,
    EndFile { reason: EndReason, error: Option<String> },
    /// mpv could not open the audio output (typically a refused bitstream).
    AudioOutputFailed(String),
    Shutdown,
}

#[derive(Debug)]
pub(crate) struct Engine {
    pub mpv: Mpv,
    pub presenter: Arc<dyn Presenter>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Engine {
    pub fn start(
        api: Arc<Api>,
        presenter: Arc<dyn Presenter>,
        extra_options: &[(String, String)],
        tx: UnboundedSender<EngineEvent>,
    ) -> oneshot_mpv::Result<Self> {
        let mut options: Vec<(String, String)> = [
            ("vo", "gpu-next"),
            ("idle", "yes"),
            // Keep the last frame at EOF: the UI decides what happens next.
            ("keep-open", "yes"),
            ("osc", "no"),
            ("osd-level", "0"),
            ("input-default-bindings", "no"),
            ("input-vo-keyboard", "no"),
            ("input-cursor", "no"),
            ("cursor-autohide", "no"),
            ("terminal", "no"),
            // Deterministic behaviour: ignore the user's global mpv.conf.
            ("config", "no"),
            ("ytdl", "no"),
            ("audio-client-name", "Flick"),
            ("cache", "yes"),
            ("track-auto-selection", "yes"),
            ("stop-screensaver", "yes"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        options.extend(presenter.init_options());
        options.extend(extra_options.iter().cloned());

        let (mpv, mut events) = Mpv::create(api, options.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;
        mpv.request_log_messages("warn")?;
        presenter.on_mpv_ready(&mpv);
        for (i, name) in OBSERVED.iter().chain(presenter.observed()).enumerate() {
            mpv.observe(i as u64 + 1, name)?;
        }

        let presenter_t = Arc::clone(&presenter);
        let thread = std::thread::Builder::new()
            .name("mpv-events".into())
            .spawn(move || {
                let mut last_pos = Instant::now() - POSITION_INTERVAL;
                loop {
                    let Some(ev) = events.wait(-1.0) else { continue };
                    let out = match ev {
                        Event::Shutdown => {
                            let _ = tx.send(EngineEvent::Shutdown);
                            break;
                        }
                        Event::PropertyChange { name, value, .. } => {
                            if presenter_t.observed().contains(&name.as_str()) {
                                presenter_t.on_property(&name, &value);
                                continue;
                            }
                            if name == "time-pos" {
                                if last_pos.elapsed() < POSITION_INTERVAL {
                                    continue;
                                }
                                last_pos = Instant::now();
                            }
                            EngineEvent::Property { name, value }
                        }
                        Event::FileLoaded => EngineEvent::FileLoaded,
                        Event::PlaybackRestart => {
                            last_pos = Instant::now() - POSITION_INTERVAL;
                            EngineEvent::PlaybackRestart
                        }
                        Event::VideoReconfig => EngineEvent::VideoReconfig,
                        Event::AudioReconfig => EngineEvent::AudioReconfig,
                        Event::EndFile { reason, error } => EngineEvent::EndFile { reason, error },
                        Event::Log { prefix, level, text } => {
                            let text = text.trim_end().to_owned();
                            match level.as_str() {
                                "error" | "fatal" => tracing::error!(target: "mpv", "{prefix}: {text}"),
                                _ => tracing::warn!(target: "mpv", "{prefix}: {text}"),
                            }
                            if prefix == "ao" && text.starts_with("Failed to initialize audio driver") {
                                EngineEvent::AudioOutputFailed(text)
                            } else {
                                continue;
                            }
                        }
                        _ => continue,
                    };
                    if tx.send(out).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn mpv event thread");
        Ok(Self { mpv, presenter, thread: Some(thread) })
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.mpv.command(&["quit"]);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
