//! End-to-end: the real client against a scripted in-process FlickSync server
//! (REST + WebSocket on one port, like the real service).

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use oneshot_flicksync::auth::TokenProvider;
use oneshot_flicksync::clock::MonotonicClock;
use oneshot_flicksync::protocol::{ControlMode, MediaRef, MediaType, Provider};
use oneshot_flicksync::room::ConnectionState;
use oneshot_flicksync::sync::{LocalPlayback, SyncConfig};
use oneshot_flicksync::{ClientEvent, FlickSyncClient, LoadError, PlaybackController};
use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

fn epoch_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as f64
}

struct StaticToken;

#[async_trait]
impl TokenProvider for StaticToken {
    async fn token(&self) -> oneshot_flicksync::Result<String> {
        Ok("test-token".into())
    }
}

#[derive(Default)]
struct Fake {
    log: Mutex<Vec<String>>,
    local: Mutex<Option<LocalPlayback>>,
    unavailable: Mutex<bool>,
}

impl Fake {
    fn new() -> Arc<Self> {
        let f = Arc::new(Self::default());
        *f.local.lock() = Some(LocalPlayback { ready: true, position: 100.0, paused: false, buffering: false, rate: 1.0 });
        f
    }
    fn calls(&self, prefix: &str) -> usize {
        self.log.lock().iter().filter(|l| l.starts_with(prefix)).count()
    }
}

#[async_trait]
impl PlaybackController for Fake {
    fn local(&self) -> LocalPlayback {
        self.local.lock().unwrap()
    }
    fn play(&self) {
        self.log.lock().push("play".into());
        self.local.lock().as_mut().unwrap().paused = false;
    }
    fn pause(&self) {
        self.log.lock().push("pause".into());
        self.local.lock().as_mut().unwrap().paused = true;
    }
    fn seek(&self, secs: f64) {
        self.log.lock().push(format!("seek {secs:.1}"));
        self.local.lock().as_mut().unwrap().position = secs;
    }
    fn set_rate(&self, rate: f64) {
        self.log.lock().push(format!("rate {rate:.2}"));
        self.local.lock().as_mut().unwrap().rate = rate;
    }
    async fn load_media(&self, _media: &MediaRef) -> Result<(), LoadError> {
        self.log.lock().push("load".into());
        if *self.unavailable.lock() { Err(LoadError::Unavailable) } else { Ok(()) }
    }
    fn unload(&self) {
        self.log.lock().push("unload".into());
    }
}

fn room_state(state: &str, position: f64, sequence: u64, you: &str) -> String {
    format!(
        r#"{{"protocol_version":1,"type":"room_state","payload":{{"room":{{"room_id":"ROOM1","state":"{state}","host_id":"alice","control_mode":"everyone","chat_enabled":true,
        "participants":[{{"participant_id":"alice","display_name":"Alice","presence":"connected","is_host":true}},{{"participant_id":"{you}","display_name":"Me","presence":"connected","is_host":false}}],
        "media":{{"provider":"jellyfin","server_id":"srv","media_id":"m1","media_type":"movie","title":"A Movie"}},
        "playback":{{"state":"{state}","position":{position},"rate":1.0,"server_time":{t},"sequence":{sequence}}}}},"you":"{you}","server_time":{t}}}}}"#,
        t = epoch_ms()
    )
}

fn playback(kind: &str, state: &str, position: f64, sequence: u64) -> String {
    format!(
        r#"{{"protocol_version":1,"type":"{kind}","payload":{{"by":"alice","state":"{state}","position":{position},"rate":1.0,"server_time":{t},"sequence":{sequence}}}}}"#,
        t = epoch_ms()
    )
}

/// What the scripted server tells a connected socket, and what it saw.
struct Server {
    base: Url,
    /// Frames the client sent (text), across every connection.
    received: Arc<Mutex<Vec<String>>>,
    /// Push a frame to the current socket.
    push: mpsc::UnboundedSender<ServerCmd>,
    connections: Arc<Mutex<u32>>,
}

enum ServerCmd {
    Frame(String),
    Drop,
}

async fn start_server() -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let received = Arc::new(Mutex::new(Vec::new()));
    let connections = Arc::new(Mutex::new(0u32));
    let (push, rx) = mpsc::unbounded_channel();
    let rx = Arc::new(tokio::sync::Mutex::new(rx));
    let (rec, conns) = (Arc::clone(&received), Arc::clone(&connections));
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { return };
            let (rec, conns, rx) = (Arc::clone(&rec), Arc::clone(&conns), Arc::clone(&rx));
            tokio::spawn(async move { serve(stream, rec, conns, rx).await });
        }
    });
    Server { base, received, push, connections }
}

async fn serve(
    mut stream: TcpStream,
    received: Arc<Mutex<Vec<String>>>,
    connections: Arc<Mutex<u32>>,
    rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<ServerCmd>>>,
) {
    let mut peek = [0u8; 8];
    let n = stream.peek(&mut peek).await.unwrap_or(0);
    if peek[..n].starts_with(b"POST") {
        // REST: create/join. Drain the request, answer with a canned room.
        let mut buf = vec![0u8; 8192];
        let _ = stream.read(&mut buf).await;
        let body =
            r#"{"room_id":"ROOM1","share_code":"ROOM-1","participant_id":"me","host_id":"alice","ws_path":"/api/v1/rooms/ROOM1/ws"}"#;
        let resp = format!(
            "HTTP/1.1 201 Created\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes()).await;
        return;
    }
    let Ok(ws) = tokio_tungstenite::accept_async(stream).await else { return };
    *connections.lock() += 1;
    let (mut sink, mut source) = ws.split();
    let mut rx = rx.lock().await;
    loop {
        tokio::select! {
            incoming = source.next() => {
                let Some(Ok(Message::Text(text))) = incoming else {
                    if incoming.is_none() || matches!(incoming, Some(Err(_)) | Some(Ok(Message::Close(_)))) { return; }
                    continue;
                };
                let text = text.to_string();
                if text.contains(r#""type":"ping""#) {
                    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
                    let ct = v["payload"]["client_time"].clone();
                    let pong = format!(r#"{{"protocol_version":1,"type":"pong","payload":{{"client_time":{ct},"server_time":{}}}}}"#, epoch_ms());
                    let _ = sink.send(Message::text(pong)).await;
                } else {
                    received.lock().push(text);
                }
            }
            cmd = rx.recv() => match cmd {
                Some(ServerCmd::Frame(f)) => { let _ = sink.send(Message::text(f)).await; }
                Some(ServerCmd::Drop) | None => return,
            },
        }
    }
}

fn fast_cfg() -> SyncConfig {
    SyncConfig { tick_ms: 40.0, report_interval_ms: 200.0, ..SyncConfig::default() }
}

async fn wait_for(what: &str, mut cond: impl FnMut() -> bool) {
    for _ in 0..200 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for: {what}");
}

fn client(server: &Server, fake: &Arc<Fake>) -> (FlickSyncClient, Arc<Mutex<Vec<ClientEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = Arc::clone(&events);
    let c = FlickSyncClient::new(
        oneshot_net::reqwest::Client::new(),
        server.base.clone(),
        Arc::new(StaticToken),
        Arc::clone(fake) as Arc<dyn PlaybackController>,
        Arc::new(MonotonicClock::new()),
        fast_cfg(),
        Arc::new(move |e| ev.lock().push(e)),
    );
    (c, events)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn follows_the_room_without_echoing_remote_changes() {
    let server = start_server().await;
    let fake = Fake::new();
    let (c, _events) = client(&server, &fake);

    let room = c.create_room(ControlMode::Everyone).await.unwrap();
    assert_eq!(room.share_code, "ROOM-1");
    wait_for("connection", || *server.connections.lock() == 1).await;

    // Joining a playing room: media is loaded, then we land on the canonical position.
    server.push.send(ServerCmd::Frame(room_state("playing", 500.0, 3, "me"))).unwrap();
    wait_for("media load", || fake.calls("load") == 1).await;
    wait_for("seek to canonical position", || fake.calls("seek 50") > 0).await;
    assert!(c.state().participants.len() == 2 && !c.state().is_host());

    // The room pauses: we pause, and the pause is NOT sent back.
    server.push.send(ServerCmd::Frame(playback("playback_pause", "paused", 510.0, 4))).unwrap();
    wait_for("remote pause applied", || fake.calls("pause") >= 1).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(server.received.lock().iter().all(|m| !m.contains("playback_pause")), "echoed: {:?}", server.received.lock());

    // A stale state (sequence 2 < 4) is ignored.
    let plays = fake.calls("play");
    server.push.send(ServerCmd::Frame(playback("playback_play", "playing", 10.0, 2))).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(fake.calls("play"), plays);
    assert_eq!(c.state().playback.unwrap().sequence, 4);

    // A user seek is sent as a command, with the last applied sequence, and NOT applied locally.
    let seeks = fake.calls("seek");
    c.user_seek(42.5).unwrap();
    wait_for("seek command", || {
        server.received.lock().iter().any(|m| m.contains("playback_seek") && m.contains("42.5") && m.contains(r#""sequence":4"#))
    })
    .await;
    assert_eq!(fake.calls("seek"), seeks);

    c.leave().await;
    wait_for("leave command", || server.received.lock().iter().any(|m| m.contains("leave_room"))).await;
    assert!(!c.in_room());
    assert_eq!(c.state().room_id, "");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnects_keeps_the_room_and_resynchronizes() {
    let server = start_server().await;
    let fake = Fake::new();
    let (c, events) = client(&server, &fake);
    c.join_room("room-1").await.unwrap();
    wait_for("first connection", || *server.connections.lock() == 1).await;
    server.push.send(ServerCmd::Frame(room_state("paused", 50.0, 5, "me"))).unwrap();
    wait_for("loaded", || fake.calls("load") == 1).await;

    server.push.send(ServerCmd::Drop).unwrap();
    wait_for("reconnecting shown", || {
        events.lock().iter().any(|e| matches!(e, ClientEvent::State { room } if room.connection == ConnectionState::Reconnecting))
    })
    .await;
    // The user is not kicked out; the player was not paused or stopped by the drop.
    assert!(c.in_room());
    assert_eq!(fake.calls("unload"), 0);

    wait_for("second connection", || *server.connections.lock() == 2).await;
    // After the reconnect the server tells the room moved on: we resync, the media is not reloaded.
    server.push.send(ServerCmd::Frame(room_state("playing", 90.0, 9, "me"))).unwrap();
    wait_for("resynced", || fake.calls("seek 9") > 0).await;
    assert_eq!(fake.calls("load"), 1, "same media: no reload");
    assert_eq!(c.state().connection, ConnectionState::Connected);
    c.leave().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_media_keeps_us_in_the_room() {
    let server = start_server().await;
    let fake = Fake::new();
    *fake.unavailable.lock() = true;
    let (c, events) = client(&server, &fake);
    c.join_room("room-1").await.unwrap();
    wait_for("connected", || *server.connections.lock() == 1).await;
    server.push.send(ServerCmd::Frame(room_state("playing", 5.0, 1, "me"))).unwrap();
    wait_for("unavailable notice", || events.lock().iter().any(|e| matches!(e, ClientEvent::MediaUnavailable { .. }))).await;
    assert!(c.in_room());
    assert_eq!(c.state().connection, ConnectionState::Connected);
    // And nothing was done to the (absent) player.
    assert_eq!(fake.calls("seek"), 0);
    c.leave().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn room_closure_returns_to_the_lobby_and_non_hosts_cannot_pick_media() {
    let server = start_server().await;
    let fake = Fake::new();
    let (c, events) = client(&server, &fake);
    c.join_room("room-1").await.unwrap();
    wait_for("connected", || *server.connections.lock() == 1).await;
    server.push.send(ServerCmd::Frame(room_state("paused", 0.0, 1, "me"))).unwrap();
    wait_for("state", || c.state().participants.len() == 2).await;

    let media = MediaRef {
        provider: Provider::Plex,
        server_id: "s".into(),
        media_id: "1".into(),
        media_type: MediaType::Movie,
        season_id: None,
        episode_id: None,
        title: None,
        duration_secs: None,
    };
    assert!(c.select_media(media).is_err(), "only the host chooses");

    server.push.send(ServerCmd::Frame(r#"{"protocol_version":1,"type":"room_closed","payload":{"reason":"host_closed"}}"#.into())).unwrap();
    wait_for("left", || events.lock().iter().any(|e| matches!(e, ClientEvent::Left { reason: Some(r) } if r == "host_closed"))).await;
    assert!(!c.in_room());
}


