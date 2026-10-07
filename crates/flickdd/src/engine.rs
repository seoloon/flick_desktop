//! The download manager: the resumable algorithm of `docs/flickdd-integration.md`
//! (sections 4 to 8) for a queue of downloads.
//!
//! * One worker task per active download; at most [`MAX_ACTIVE`] run at once,
//!   the rest of the queue stays local (a grant is created just before its
//!   download starts).
//! * One request at a time per grant, segments of the server's `chunk_bytes`,
//!   always resumed from the local offset, never from what the server counted.
//! * The record is saved to disk after state changes and about once a second
//!   while bytes flow. Bytes are written before the offset advances, so the
//!   saved offset never exceeds the bytes on disk; on resume the partial file
//!   is truncated to it.
//! * Failures are classified (guide, section 5): retried with full-jitter
//!   back-off, a grant that died is recreated, and after [`GIVE_UP`] failures
//!   without a received byte the download is paused, not failed.

use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use parking_lot::Mutex;
use tokio::io::AsyncWriteExt;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use crate::api::{self, Failure, Grant, Link};
use crate::files;
use crate::item::{Item, NewDownload, State};

/// Downloads running at once. The server allows 10 grants per user, shared by all their devices.
pub const MAX_ACTIVE: usize = 3;
/// Consecutive failures without a received byte before a download is paused.
const GIVE_UP: u32 = 8;
/// Consecutive `SOURCE_CHANGED` without a received byte before a download fails.
const MAX_CHANGES: u32 = 3;
/// How often progress reaches the UI.
const EMIT_EVERY: Duration = Duration::from_millis(250);
/// How often progress reaches the disk.
const SAVE_EVERY: Duration = Duration::from_secs(1);

/// What a download needs to reach the server, asked when a grant is created.
#[derive(Debug, Clone)]
pub struct Connection {
    pub link: Link,
    /// A Flick JWT with `downloads:create`, valid for a few minutes.
    pub jwt: String,
}

/// The app side: where the server is, and where files go.
#[async_trait]
pub trait Source: Send + Sync {
    async fn connect(&self) -> Result<Connection, Failure>;
    /// Where new downloads are written.
    fn directory(&self) -> PathBuf;
}

#[derive(Debug, Clone)]
pub enum Event {
    Changed(Box<Item>),
    Removed(String),
}

pub type EventSink = Arc<dyn Fn(Event) + Send + Sync>;

struct Slot {
    item: Item,
    grant: Option<Grant>,
    link: Option<Link>,
    /// Consecutive failures without a received byte.
    fails: u32,
    /// Consecutive `SOURCE_CHANGED` without a received byte.
    changes: u32,
    task: Option<JoinHandle<()>>,
}

struct Inner {
    slots: Vec<Slot>,
    saved: Instant,
}

pub struct Manager {
    source: Arc<dyn Source>,
    sink: EventSink,
    path: PathBuf,
    rt: Handle,
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for Manager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Manager").finish_non_exhaustive()
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Step {
    More,
    Done,
}

#[derive(Debug, PartialEq, Eq)]
enum Flow {
    Continue,
    Stop,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// Full-jitter back-off: `random(0, min(60 s, 1 s * 2^n))`.
pub fn backoff(n: u32) -> Duration {
    let cap = 60_000u64.min(1000u64.saturating_mul(1u64 << n.min(16)));
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u8(0);
    let unit = (h.finish() >> 11) as f64 / (1u64 << 53) as f64;
    Duration::from_millis((cap as f64 * unit) as u64)
}

/// A sentence for the user about why a download keeps failing.
fn describe(f: &Failure) -> String {
    match f {
        Failure::Http { status: 401, .. } => "The server did not accept this device.".into(),
        Failure::Http { status: 429, code, .. } if code == "RATE_LIMITED" => "The server asked to slow down.".into(),
        Failure::Http { status: 502, .. } => "The media server did not answer the Flick server.".into(),
        Failure::Http { status, .. } if *status >= 500 => "The server had a problem.".into(),
        Failure::Http { .. } => "The server refused the download.".into(),
        Failure::Network(_) | Failure::Stalled | Failure::CutShort => "The connection to the server is unstable.".into(),
        Failure::NotConfigured(m) | Failure::Disk(m) => m.clone(),
    }
}

impl Manager {
    /// Loads the saved downloads. Nothing runs until [`Manager::start`].
    pub fn open(source: Arc<dyn Source>, path: PathBuf, sink: EventSink, rt: Handle) -> Arc<Self> {
        let mut items: Vec<Item> = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                tracing::warn!(target: "flickdd", "downloads.json is unreadable, starting empty: {e}");
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        for it in &mut items {
            // A worker did not survive the restart: it is simply queued again.
            if it.state == State::Active {
                it.state = State::Queued;
            }
            it.note = None;
            it.speed = None;
        }
        let slots = items.into_iter().map(|item| Slot { item, grant: None, link: None, fails: 0, changes: 0, task: None }).collect();
        Arc::new(Self { source, sink, path, rt, inner: Mutex::new(Inner { slots, saved: Instant::now() }) })
    }

    /// Resumes what was queued or running when the app last closed.
    pub fn start(self: &Arc<Self>) {
        self.pump();
    }

    pub fn list(&self) -> Vec<Item> {
        let mut items: Vec<Item> = self.inner.lock().slots.iter().map(|s| s.item.clone()).collect();
        for it in &mut items {
            it.missing = it.state == State::Done && it.final_path.as_deref().is_none_or(|p| !p.exists());
        }
        items
    }

    /// Adds a download. The same title already queued, running, paused or on disk is returned as is.
    pub fn enqueue(self: &Arc<Self>, req: &NewDownload) -> Item {
        let existing = {
            let g = self.inner.lock();
            g.slots
                .iter()
                .find(|s| s.item.backend == req.backend && s.item.item_id == req.item_id && s.item.state != State::Failed)
                .map(|s| (s.item.id.clone(), s.item.state, s.item.final_path.clone()))
        };
        if let Some((id, state, final_path)) = existing {
            let file_gone = state == State::Done && final_path.as_deref().is_none_or(|p| !p.exists());
            if !file_gone {
                return self.item(&id).unwrap_or_else(|| unreachable_item(req));
            }
            // Downloaded once, deleted since: download it again.
            self.discard(&id);
        }
        // A failed attempt at the same title is replaced by the new one.
        let failed: Vec<String> = {
            let g = self.inner.lock();
            g.slots.iter().filter(|s| s.item.backend == req.backend && s.item.item_id == req.item_id).map(|s| s.item.id.clone()).collect()
        };
        for id in failed {
            self.discard(&id);
        }
        let item = Item::new(req, PathBuf::new(), now_ms());
        let temp = self.source.directory().join(format!("{}.part", item.id));
        let item = Item { temp_path: temp, ..item };
        {
            let mut g = self.inner.lock();
            g.slots.push(Slot { item: item.clone(), grant: None, link: None, fails: 0, changes: 0, task: None });
        }
        self.persist();
        self.emit(&item);
        self.pump();
        self.item(&item.id).unwrap_or(item)
    }

    pub fn item(&self, id: &str) -> Option<Item> {
        self.read(id, |s| s.item.clone())
    }

    /// Stops a download and frees its slot on the server; the partial file stays.
    pub fn pause(self: &Arc<Self>, id: &str) {
        let taken = self.update(id, true, |s| {
            if !matches!(s.item.state, State::Queued | State::Active) {
                return None;
            }
            s.item.state = State::Paused;
            s.item.error = None;
            s.item.note = None;
            s.item.speed = None;
            Some((s.task.take(), s.grant.take(), s.link.clone()))
        });
        if let Some(Some((task, grant, link))) = taken {
            if let Some(t) = task {
                t.abort();
            }
            self.release_later(link, grant);
        }
        self.pump();
    }

    /// Queues a paused or failed download again.
    pub fn resume(self: &Arc<Self>, id: &str) {
        self.update(id, true, |s| {
            if matches!(s.item.state, State::Paused | State::Failed) {
                s.item.state = State::Queued;
                s.item.error = None;
                s.item.note = None;
                s.fails = 0;
                s.changes = 0;
            }
        });
        self.pump();
    }

    /// Cancels an unfinished download (partial file deleted), or forgets a finished one
    /// (its file is deleted only when `delete_file`).
    pub fn remove(self: &Arc<Self>, id: &str, delete_file: bool) {
        let Some(item) = self.discard(id) else { return };
        if delete_file && let Some(p) = &item.final_path {
            let _ = std::fs::remove_file(p);
        }
        self.pump();
    }

    /// Cancels everything and deletes every downloaded file.
    pub fn clear(self: &Arc<Self>) {
        let ids: Vec<String> = self.inner.lock().slots.iter().map(|s| s.item.id.clone()).collect();
        for id in ids {
            self.remove(&id, true);
        }
    }

    /// Takes a download out of the queue and cleans up after it.
    fn discard(self: &Arc<Self>, id: &str) -> Option<Item> {
        let slot = {
            let mut g = self.inner.lock();
            let i = g.slots.iter().position(|s| s.item.id == id)?;
            g.slots.remove(i)
        };
        if let Some(t) = &slot.task {
            t.abort();
        }
        self.release_later(slot.link.clone(), slot.grant.clone());
        if slot.item.state != State::Done {
            let temp = slot.item.temp_path.clone();
            self.rt.spawn(async move {
                let _ = tokio::fs::remove_file(temp).await;
            });
        }
        self.persist();
        (self.sink)(Event::Removed(id.to_owned()));
        Some(slot.item)
    }

    // ----------------------------------------------------------- plumbing

    fn read<R>(&self, id: &str, f: impl FnOnce(&Slot) -> R) -> Option<R> {
        self.inner.lock().slots.iter().find(|s| s.item.id == id).map(f)
    }

    /// Changes a download, tells the UI, and saves when asked (or when a second passed).
    fn update<R>(&self, id: &str, save: bool, f: impl FnOnce(&mut Slot) -> R) -> Option<R> {
        let (r, item, snapshot) = {
            let mut g = self.inner.lock();
            let slot = g.slots.iter_mut().find(|s| s.item.id == id)?;
            let r = f(slot);
            let item = slot.item.clone();
            let due = save || g.saved.elapsed() >= SAVE_EVERY;
            let snapshot = due.then(|| {
                g.saved = Instant::now();
                g.slots.iter().map(|s| s.item.clone()).collect::<Vec<_>>()
            });
            (r, item, snapshot)
        };
        self.emit(&item);
        if let Some(items) = snapshot {
            self.write(&items);
        }
        Some(r)
    }

    fn emit(&self, item: &Item) {
        (self.sink)(Event::Changed(Box::new(item.clone())));
    }

    fn persist(&self) {
        let items: Vec<Item> = {
            let mut g = self.inner.lock();
            g.saved = Instant::now();
            g.slots.iter().map(|s| s.item.clone()).collect()
        };
        self.write(&items);
    }

    fn write(&self, items: &[Item]) {
        let tmp = self.path.with_extension("json.tmp");
        let result = (|| -> std::io::Result<()> {
            if let Some(dir) = self.path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&tmp, serde_json::to_vec_pretty(items)?)?;
            std::fs::rename(&tmp, &self.path)
        })();
        if let Err(e) = result {
            tracing::warn!(target: "flickdd", "could not save the downloads: {e}");
        }
    }

    /// Starts queued downloads while a slot is free.
    fn pump(self: &Arc<Self>) {
        let started: Vec<Item> = {
            let mut g = self.inner.lock();
            let mut running = g.slots.iter().filter(|s| s.task.is_some()).count();
            let mut started = Vec::new();
            for s in g.slots.iter_mut() {
                if running >= MAX_ACTIVE {
                    break;
                }
                if s.task.is_none() && s.item.state == State::Queued {
                    s.item.state = State::Active;
                    running += 1;
                    let me = Arc::clone(self);
                    let id = s.item.id.clone();
                    // Spawned under the lock: the worker's first step cannot run before `task` is set.
                    s.task = Some(self.rt.spawn(me.worker(id)));
                    started.push(s.item.clone());
                }
            }
            started
        };
        for it in &started {
            self.emit(it);
        }
        if !started.is_empty() {
            self.persist();
        }
    }

    fn release_later(&self, link: Option<Link>, grant: Option<Grant>) {
        if let (Some(link), Some(grant)) = (link, grant) {
            self.rt.spawn(async move { api::release(&link, &grant).await });
        }
    }

    /// Frees the current grant on the server, best effort.
    async fn drop_grant(&self, id: &str) {
        let taken = self.update(id, false, |s| (s.link.clone(), s.grant.take()));
        if let Some((Some(link), Some(grant))) = taken {
            api::release(&link, &grant).await;
        }
    }

    fn fail(&self, id: &str, why: &str) {
        self.update(id, true, |s| {
            s.item.state = State::Failed;
            s.item.error = Some(why.to_owned());
            s.item.note = None;
            s.item.speed = None;
        });
    }

    fn pause_with(&self, id: &str, why: String) {
        self.update(id, true, |s| {
            s.item.state = State::Paused;
            s.item.error = Some(why);
            s.item.note = None;
            s.item.speed = None;
        });
    }

    // -------------------------------------------------------------- worker

    async fn worker(self: Arc<Self>, id: String) {
        while let Some(State::Active) = self.read(&id, |s| s.item.state) {
            match self.step(&id).await {
                Ok(Step::Done) => break,
                Ok(Step::More) => {}
                Err(f) => {
                    if self.on_error(&id, f).await == Flow::Stop {
                        break;
                    }
                }
            }
        }
        // Whatever ended the loop, the slot on the server is not needed any more.
        self.drop_grant(&id).await;
        self.update(&id, false, |s| {
            s.task = None;
            s.item.speed = None;
        });
        self.pump();
    }

    async fn step(&self, id: &str) -> Result<Step, Failure> {
        let Some(has_grant) = self.read(id, |s| s.grant.is_some()) else { return Ok(Step::Done) };
        if !has_grant {
            self.create(id).await?;
        }
        let Some((size, offset)) = self.read(id, |s| (s.grant.as_ref().map_or(0, |g| g.size), s.item.offset)) else { return Ok(Step::Done) };
        if offset >= size {
            return self.finish(id).await;
        }
        self.segment(id).await?;
        Ok(Step::More)
    }

    /// Asks for a grant. If the file's size or ETag differ from the previous grant's, it changed:
    /// the partial file is dropped and the download starts over.
    async fn create(&self, id: &str) -> Result<(), Failure> {
        let Some((backend, item_id, title, kind, old_size, old_etag, offset, temp)) = self.read(id, |s| {
            let i = &s.item;
            (i.backend, i.item_id.clone(), i.title.clone(), i.kind, i.size, i.etag.clone(), i.offset, i.temp_path.clone())
        }) else {
            return Ok(());
        };
        let conn = self.source.connect().await?;
        let grant = api::create(&conn.link, &conn.jwt, backend, &item_id, &title, kind).await?;
        let changed = matches!((old_size, &old_etag), (Some(s), Some(e)) if s != grant.size || *e != grant.etag);
        let lost = offset > 0 && files::size_of(&temp).await < offset;
        if changed || lost {
            let _ = tokio::fs::remove_file(&temp).await;
        }
        let link = conn.link;
        self.update(id, true, |s| {
            if changed || lost {
                s.item.offset = 0;
            }
            s.item.size = Some(grant.size);
            s.item.etag = Some(grant.etag.clone());
            s.item.filename = Some(grant.filename.clone());
            s.item.mime = Some(grant.mime.clone()).filter(|m| !m.is_empty());
            s.item.error = None;
            s.item.note = None;
            s.grant = Some(grant);
            s.link = Some(link);
        });
        Ok(())
    }

    /// One segment: `Range: bytes=<offset>-<offset + chunk - 1>`, written to the partial file.
    async fn segment(&self, id: &str) -> Result<(), Failure> {
        let Some((grant, link, mut offset, temp)) =
            self.read(id, |s| Some((s.grant.clone()?, s.link.clone()?, s.item.offset, s.item.temp_path.clone()))).flatten()
        else {
            return Ok(());
        };
        if offset > 0 && files::size_of(&temp).await < offset {
            // The partial file lost bytes: start over rather than extend it with zeros.
            offset = 0;
            self.update(id, true, |s| s.item.offset = 0);
        }
        let end = offset.saturating_add(grant.chunk_bytes).min(grant.size) - 1;
        let mut seg = api::open_segment(&link, &grant, offset, end).await?;
        let mut file = files::open_at(&temp, offset).await.map_err(|e| Failure::Disk(e.to_string()))?;

        let mut got = 0u64;
        let (mut last_emit, mut window, mut window_bytes) = (Instant::now(), Instant::now(), 0u64);
        // Carried over from the previous segment: a segment lasts well under a second at the server's cap.
        let mut speed: Option<u64> = self.read(id, |s| s.item.speed).flatten();
        while let Some(chunk) = api::next_chunk(&mut seg).await? {
            // Never past the end of the file, whatever the server sends.
            let room = grant.size.saturating_sub(offset);
            let chunk = &chunk[..chunk.len().min(room as usize)];
            if chunk.is_empty() {
                break;
            }
            file.write_all(chunk).await.map_err(|e| Failure::Disk(e.to_string()))?;
            offset += chunk.len() as u64;
            got += chunk.len() as u64;
            window_bytes += chunk.len() as u64;
            if window.elapsed() >= Duration::from_millis(300) {
                let now = (window_bytes as f64 / window.elapsed().as_secs_f64()) as u64;
                speed = Some(speed.map_or(now, |old| (old + now * 2) / 3));
                (window, window_bytes) = (Instant::now(), 0);
            }
            let due = last_emit.elapsed() >= EMIT_EVERY;
            if due {
                last_emit = Instant::now();
            }
            let spd = speed;
            // Any received byte ends the failure streaks.
            self.update_quiet(id, due, |s| {
                s.item.offset = offset;
                s.item.speed = spd;
                s.item.note = None;
                s.fails = 0;
                s.changes = 0;
            });
        }
        file.sync_all().await.map_err(|e| Failure::Disk(e.to_string()))?;
        drop(file);
        self.update(id, true, |s| s.item.offset = offset);
        if got < seg.want { Err(Failure::CutShort) } else { Ok(()) }
    }

    /// `update`, but only tells the UI when `emit`.
    fn update_quiet(&self, id: &str, emit: bool, f: impl FnOnce(&mut Slot)) {
        if emit {
            self.update(id, false, f);
            return;
        }
        let snapshot = {
            let mut g = self.inner.lock();
            let Some(slot) = g.slots.iter_mut().find(|s| s.item.id == id) else { return };
            f(slot);
            (g.saved.elapsed() >= SAVE_EVERY).then(|| {
                g.saved = Instant::now();
                g.slots.iter().map(|s| s.item.clone()).collect::<Vec<_>>()
            })
        };
        if let Some(items) = snapshot {
            self.write(&items);
        }
    }

    /// Every byte is here: check the size, rename the partial file to its final name.
    async fn finish(&self, id: &str) -> Result<Step, Failure> {
        let Some((grant, temp)) = self.read(id, |s| s.grant.clone().map(|g| (g, s.item.temp_path.clone()))).flatten() else { return Ok(Step::Done) };
        if files::size_of(&temp).await != grant.size {
            // Not the file we think we have: start over.
            self.drop_grant(id).await;
            let _ = tokio::fs::remove_file(&temp).await;
            self.update(id, true, |s| s.item.offset = 0);
            return Err(Failure::Network("the downloaded file has the wrong size".into()));
        }
        let dir = temp.parent().map(Path::to_path_buf).unwrap_or_else(|| self.source.directory());
        let target = files::unique_path(&dir, &files::media_filename(&grant.filename));
        tokio::fs::rename(&temp, &target).await.map_err(|e| Failure::Disk(e.to_string()))?;
        self.update(id, true, |s| {
            s.item.state = State::Done;
            s.item.offset = grant.size;
            s.item.final_path = Some(target);
            s.item.error = None;
            s.item.note = None;
            s.item.speed = None;
        });
        Ok(Step::Done)
    }

    /// Section 5 of the guide: what each failure means.
    async fn on_error(&self, id: &str, f: Failure) -> Flow {
        tracing::warn!(target: "flickdd", "a download step failed: {f}");
        let (status, code) = (f.status(), f.code().to_owned());
        match &f {
            Failure::NotConfigured(m) => {
                self.pause_with(id, m.clone());
                return Flow::Stop;
            }
            Failure::Disk(m) => {
                self.pause_with(id, format!("Flick cannot write the file: {m}"));
                return Flow::Stop;
            }
            _ => {}
        }
        // Queueing, not failing: another device holds the slots, or the server is busy.
        if status == 429 && code == "TOO_MANY_DOWNLOADS" {
            let fails = self.read(id, |s| s.fails).unwrap_or(0);
            self.wait(id, backoff((fails + 3).min(6)), "Waiting for a free download slot on the server").await;
            return Flow::Continue;
        }
        let Some((had_grant, offset, size)) = self.read(id, |s| (s.grant.is_some(), s.item.offset, s.item.size)) else { return Flow::Stop };
        match (status, code.as_str()) {
            (403, _) => {
                self.fail(id, "This account is not allowed to download.");
                return Flow::Stop;
            }
            (400, _) => {
                self.fail(id, "The server cannot download this title (it may be empty).");
                return Flow::Stop;
            }
            (503, _) => {
                self.fail(id, "The server is not connected to this kind of media server.");
                return Flow::Stop;
            }
            (404, _) if !had_grant => {
                self.fail(id, "Downloads are not available on this server, or the title is gone.");
                return Flow::Stop;
            }
            (409, _) => {
                // The file changed: nothing already downloaded is worth keeping.
                self.drop_grant(id).await;
                let temp = self.read(id, |s| s.item.temp_path.clone());
                if let Some(t) = temp {
                    let _ = tokio::fs::remove_file(t).await;
                }
                let changes = self
                    .update(id, true, |s| {
                        s.item.offset = 0;
                        s.item.size = None;
                        s.item.etag = None;
                        s.changes += 1;
                        s.changes
                    })
                    .unwrap_or(MAX_CHANGES);
                if changes >= MAX_CHANGES {
                    self.fail(id, "The file keeps changing on the server.");
                    return Flow::Stop;
                }
            }
            (401 | 404, _) | (_, "QUOTA_EXCEEDED") => {
                // The grant is gone (or spent): the next step creates another and resumes at the local offset.
                self.update(id, false, |s| s.grant = None);
            }
            (416, _) => {
                if size.is_some_and(|sz| offset >= sz) {
                    return Flow::Continue;
                }
                self.drop_grant(id).await;
            }
            _ => {}
        }
        let fails = self.update(id, false, |s| {
            s.fails += 1;
            s.fails
        });
        let Some(fails) = fails else { return Flow::Stop };
        if fails >= GIVE_UP {
            self.drop_grant(id).await;
            self.pause_with(id, format!("{} Resume it when the connection is back.", describe(&f)));
            return Flow::Stop;
        }
        let retry_after = match &f {
            Failure::Http { retry_after: Some(s), .. } => Duration::from_secs(*s),
            _ => Duration::ZERO,
        };
        self.wait(id, backoff(fails).max(retry_after), &format!("Connection problem ({f}), retrying")).await;
        Flow::Continue
    }

    async fn wait(&self, id: &str, d: Duration, why: &str) {
        let note = format!("{why} ({} s)", d.as_secs().max(1));
        self.update(id, false, |s| {
            s.item.note = Some(note);
            s.item.speed = None;
        });
        tokio::time::sleep(d).await;
        self.update(id, false, |s| s.item.note = None);
    }
}

/// `enqueue` found a match that vanished before it could be read back.
fn unreachable_item(req: &NewDownload) -> Item {
    Item::new(req, PathBuf::new(), now_ms())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_capped_and_jittered() {
        for n in 0..40 {
            assert!(backoff(n) <= Duration::from_secs(60), "n = {n}");
        }
        assert!(backoff(0) <= Duration::from_secs(1));
        assert!(backoff(3) <= Duration::from_secs(8));
        let spread: std::collections::HashSet<u128> = (0..30).map(|_| backoff(6).as_millis()).collect();
        assert!(spread.len() > 5, "full jitter must vary");
    }
}
