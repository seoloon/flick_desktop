//! The manager against a fake FlickDD server.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use oneshot_flickdd::api::{Failure, Link};
use oneshot_flickdd::{Backend, Connection, Event, Kind, Manager, NewDownload, Source, State};
use oneshot_net::reqwest::Client;
use parking_lot::Mutex;
use url::Url;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const CHUNK: u64 = 1000;

fn content(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i * 7 % 251) as u8).collect()
}

struct TestSource {
    base: Url,
    dir: PathBuf,
}

#[async_trait]
impl Source for TestSource {
    async fn connect(&self) -> Result<Connection, Failure> {
        Ok(Connection { link: Link { http: Client::new(), base: self.base.clone() }, jwt: "jwt".into() })
    }

    fn directory(&self) -> PathBuf {
        self.dir.clone()
    }
}

/// Serves `data` with `Range` support; `hook` can break a request first.
struct FileServer {
    data: Vec<u8>,
    etag: &'static str,
    hook: Box<dyn Fn(usize) -> Option<ResponseTemplate> + Send + Sync>,
    calls: AtomicUsize,
}

impl Respond for FileServer {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(r) = (self.hook)(n) {
            return r;
        }
        assert_eq!(req.headers.get("if-range").and_then(|v| v.to_str().ok()), Some(self.etag), "If-Range must carry the grant's ETag");
        assert!(req.headers.get("authorization").is_some_and(|v| v.to_str().unwrap().starts_with("Bearer tok")));
        let range = req.headers.get("range").unwrap().to_str().unwrap().strip_prefix("bytes=").unwrap().to_owned();
        let (a, b) = range.split_once('-').unwrap();
        let start: usize = a.parse().unwrap();
        let end: usize = b.parse::<usize>().unwrap().min(self.data.len() - 1);
        ResponseTemplate::new(206)
            .insert_header("Content-Range", format!("bytes {start}-{end}/{}", self.data.len()))
            .set_body_bytes(self.data[start..=end].to_vec())
    }
}

struct Harness {
    server: MockServer,
    dir: tempfile::TempDir,
    manager: Arc<Manager>,
    creates: Arc<AtomicUsize>,
    deletes: Arc<AtomicUsize>,
    events: Arc<Mutex<Vec<Event>>>,
}

async fn harness(size: usize, etag: &'static str, hook: impl Fn(usize) -> Option<ResponseTemplate> + Send + Sync + 'static) -> Harness {
    let server = MockServer::start().await;
    let data = content(size);
    let creates = Arc::new(AtomicUsize::new(0));
    let c2 = Arc::clone(&creates);
    Mock::given(method("POST"))
        .and(path("/api/v1/downloads"))
        .respond_with(move |_: &Request| {
            let n = c2.fetch_add(1, Ordering::SeqCst) + 1;
            ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "url": format!("/api/v1/downloads/D{n}/file"), "download_id": format!("D{n}"), "token": format!("tok{n}"),
                "size": size, "filename": "The Long Night (2021).mkv", "mime": "video/x-matroska", "etag": etag,
                "chunk_bytes": CHUNK, "max_range_bytes": 64 * CHUNK, "rate_limit_bps": 1_000_000, "expires_at": 0
            }))
        })
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/api/v1/downloads/D\d+/file$"))
        .respond_with(FileServer { data, etag, hook: Box::new(hook), calls: AtomicUsize::new(0) })
        .mount(&server)
        .await;
    let deletes = Arc::new(AtomicUsize::new(0));
    let d2 = Arc::clone(&deletes);
    Mock::given(method("DELETE"))
        .and(path_regex(r"^/api/v1/downloads/D\d+$"))
        .respond_with(move |_: &Request| {
            d2.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(204)
        })
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let source = Arc::new(TestSource { base: Url::parse(&format!("{}/", server.uri())).unwrap(), dir: dir.path().join("downloads") });
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink_events = Arc::clone(&events);
    let manager = Manager::open(source, dir.path().join("downloads.json"), Arc::new(move |e| sink_events.lock().push(e)), tokio::runtime::Handle::current());
    Harness { server, dir, manager, creates, deletes, events }
}

fn request() -> NewDownload {
    NewDownload {
        backend: Backend::Jellyfin,
        item_id: "a1b2c3d4".into(),
        item_ref: "srv:a1b2c3d4".into(),
        title: "The Long Night".into(),
        subtitle: None,
        kind: Some(Kind::Movie),
    }
}

async fn until(h: &Harness, id: &str, want: State, secs: u64) -> oneshot_flickdd::Item {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let it = h.manager.item(id).expect("the item exists");
        if it.state == want {
            return it;
        }
        assert!(tokio::time::Instant::now() < deadline, "timed out in state {:?} (offset {}, note {:?}, error {:?})", it.state, it.offset, it.note, it.error);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn a_download_completes_and_frees_its_slot() {
    let h = harness(3500, "\"e1\"", |_| None).await;
    let it = h.manager.enqueue(&request());
    let done = until(&h, &it.id, State::Done, 10).await;
    let file = done.final_path.clone().unwrap();
    assert_eq!(file.file_name().unwrap(), "The Long Night (2021).mkv");
    assert_eq!(std::fs::read(&file).unwrap(), content(3500));
    assert!(!done.temp_path.exists(), "the partial file was renamed");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(h.deletes.load(Ordering::SeqCst), 1, "the grant is cancelled at the end");
    assert_eq!(h.creates.load(Ordering::SeqCst), 1);
    assert!(h.events.lock().iter().any(|e| matches!(e, Event::Changed(i) if i.state == State::Done)));
    // The record survives a restart.
    let saved = std::fs::read_to_string(h.dir.path().join("downloads.json")).unwrap();
    assert!(saved.contains("\"done\"") && !saved.contains("tok1"), "the token is never written");
}

#[tokio::test]
async fn a_server_error_is_retried_from_the_local_offset() {
    // The second request fails with a 502, then everything works.
    let h = harness(3500, "\"e1\"", |n| (n == 1).then(|| ResponseTemplate::new(502).set_body_json(serde_json::json!({"error": {"code": "BACKEND_UNAVAILABLE", "message": "x"}})))).await;
    let it = h.manager.enqueue(&request());
    let done = until(&h, &it.id, State::Done, 15).await;
    assert_eq!(std::fs::read(done.final_path.unwrap()).unwrap(), content(3500));
    assert_eq!(h.creates.load(Ordering::SeqCst), 1, "a 502 does not recreate the grant");
}

#[tokio::test]
async fn a_dead_grant_is_recreated_and_the_download_resumes() {
    // The third request finds the grant gone.
    let h = harness(3500, "\"e1\"", |n| (n == 2).then(|| ResponseTemplate::new(404).set_body_json(serde_json::json!({"error": {"code": "DOWNLOAD_NOT_FOUND", "message": "x"}})))).await;
    let it = h.manager.enqueue(&request());
    let done = until(&h, &it.id, State::Done, 15).await;
    assert_eq!(std::fs::read(done.final_path.unwrap()).unwrap(), content(3500));
    assert_eq!(h.creates.load(Ordering::SeqCst), 2, "one new grant, same ETag, resumed in place");
}

#[tokio::test]
async fn a_forbidden_account_fails_without_retrying() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({"error": {"code": "FORBIDDEN", "message": "no"}})))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let source = Arc::new(TestSource { base: Url::parse(&format!("{}/", server.uri())).unwrap(), dir: dir.path().join("d") });
    let manager = Manager::open(source, dir.path().join("downloads.json"), Arc::new(|_| {}), tokio::runtime::Handle::current());
    let it = manager.enqueue(&request());
    for _ in 0..100 {
        if manager.item(&it.id).unwrap().state == State::Failed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let it = manager.item(&it.id).unwrap();
    assert_eq!(it.state, State::Failed);
    assert!(it.error.is_some());
}

#[tokio::test]
async fn pausing_stops_and_resuming_finishes() {
    // A slow server keeps the download busy long enough to pause it.
    let h = harness(3500, "\"e1\"", |n| (n == 1).then(|| ResponseTemplate::new(502).set_delay(Duration::from_millis(300)))).await;
    let it = h.manager.enqueue(&request());
    tokio::time::sleep(Duration::from_millis(100)).await;
    h.manager.pause(&it.id);
    assert_eq!(h.manager.item(&it.id).unwrap().state, State::Paused);
    h.manager.resume(&it.id);
    let done = until(&h, &it.id, State::Done, 15).await;
    assert_eq!(std::fs::read(done.final_path.unwrap()).unwrap(), content(3500));
    assert!(h.server.received_requests().await.is_some());
}

#[tokio::test]
async fn cancelling_deletes_the_partial_file() {
    let h = harness(3500, "\"e1\"", |n| (n >= 1).then(|| ResponseTemplate::new(502).set_delay(Duration::from_millis(500)))).await;
    let it = h.manager.enqueue(&request());
    tokio::time::sleep(Duration::from_millis(150)).await;
    let temp = h.manager.item(&it.id).unwrap().temp_path;
    h.manager.remove(&it.id, false);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(h.manager.item(&it.id).is_none());
    assert!(!temp.exists());
    assert!(h.manager.list().is_empty());
}

#[tokio::test]
async fn the_same_title_is_not_queued_twice() {
    let h = harness(3500, "\"e1\"", |_| None).await;
    let a = h.manager.enqueue(&request());
    let b = h.manager.enqueue(&request());
    assert_eq!(a.id, b.id);
    assert_eq!(h.manager.list().len(), 1);
}
