//! Structured logging into an in-memory ring buffer (Debug panel) plus
//! stderr. Every event keeps its target so the panel can filter by area
//! (`provider`, `playback`, `player`, `mpv`, `capabilities`, `catalog`…).

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::Arc;

use chrono::{SecondsFormat, Utc};
use parking_lot::Mutex;
use serde::Serialize;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

const CAPACITY: usize = 4_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub seq: u64,
    pub time: String,
    pub level: &'static str,
    pub target: String,
    pub message: String,
    pub fields: String,
}

#[derive(Debug, Default)]
struct Buffer {
    entries: VecDeque<LogEntry>,
    next_seq: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    buffer: Arc<Mutex<Buffer>>,
}

impl Diagnostics {
    /// Entries with `seq > since`, optionally filtered by target prefix.
    pub fn entries(&self, since: u64, target: Option<&str>) -> Vec<LogEntry> {
        self.buffer
            .lock()
            .entries
            .iter()
            .filter(|e| e.seq > since && target.is_none_or(|t| e.target.starts_with(t)))
            .cloned()
            .collect()
    }

    pub fn layer(&self) -> RingLayer {
        RingLayer { buffer: Arc::clone(&self.buffer) }
    }
}

#[derive(Debug)]
pub struct RingLayer {
    buffer: Arc<Mutex<Buffer>>,
}

#[derive(Default)]
struct Collect {
    message: String,
    fields: String,
}

impl Visit for Collect {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.fields, "{}={value:?} ", field.name());
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            let _ = write!(self.fields, "{}={value} ", field.name());
        }
    }
}

impl<S: Subscriber> Layer<S> for RingLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let mut c = Collect::default();
        event.record(&mut c);
        let level = match *meta.level() {
            Level::ERROR => "error",
            Level::WARN => "warn",
            Level::INFO => "info",
            Level::DEBUG => "debug",
            Level::TRACE => "trace",
        };
        let mut b = self.buffer.lock();
        b.next_seq += 1;
        let entry = LogEntry {
            seq: b.next_seq,
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            level,
            target: meta.target().to_owned(),
            message: oneshot_net::redact(&c.message),
            fields: oneshot_net::redact(c.fields.trim_end()),
        };
        if b.entries.len() == CAPACITY {
            b.entries.pop_front();
        }
        b.entries.push_back(entry);
    }
}
