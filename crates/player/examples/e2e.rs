//! End-to-end playback check against a real server (see docs/PLAYBACK_VALIDATION.md).
//!
//!   ONESHOT_JELLYFIN_URL=http://localhost:18096 cargo run -p oneshot-player --example e2e
//!   ONESHOT_PLEX_URL=http://localhost:32401     cargo run -p oneshot-player --example e2e
//!
//! Uses the dedicated-window presenter (no Tauri host needed): a real mpv
//! window opens for each file.
//!
//! `E2E_ONLY=<title part>` plays matching titles only; `E2E_MAX_BITRATE=<bps>`
//! caps the bitrate so the server transcodes (checks capped, offset streams).

use std::sync::Arc;
use std::time::Duration;

use oneshot_core::ServerId;
use oneshot_core::media::ItemKind;
use oneshot_core::provider::MediaProvider;
use oneshot_core::query::ItemQuery;
use oneshot_core::server::{ProviderKind, ServerDescriptor, UserProfile};
use oneshot_core::settings::{PresenterChoice, Settings};
use oneshot_player::presenter::HostWindow;
use oneshot_player::state::PlayerEvent;
use oneshot_player::{PlayRequest, Player, PlayerCommand, PlayerConfig};

async fn provider() -> Arc<dyn MediaProvider> {
    let http = oneshot_net::client(&Default::default()).unwrap();
    if let Ok(url) = std::env::var("ONESHOT_JELLYFIN_URL") {
        let id = oneshot_jellyfin::ClientIdentity {
            client: "Flick-e2e".into(),
            device_name: "e2e".into(),
            device_id: "oneshot-e2e".into(),
            version: "0.1.0".into(),
        };
        let c = oneshot_jellyfin::Connector::new(http.clone(), id.clone());
        let (base, _) = c.probe(&url).await.unwrap();
        let s = c.login(&base, "oneshot", "oneshot").await.unwrap();
        let d = ServerDescriptor {
            id: ServerId::new(),
            kind: ProviderKind::Jellyfin,
            name: s.server_name.clone(),
            remote_id: s.server_id.clone(),
            base_url: base,
            alternate_urls: vec![],
            version: s.version.clone(),
            user: UserProfile { id: s.user_id.clone(), name: s.user_name.clone(), avatar: None, is_admin: s.is_admin },
            disabled: false,
        };
        return Arc::new(oneshot_jellyfin::JellyfinProvider::new(d, http, id, s.token));
    }
    let url = std::env::var("ONESHOT_PLEX_URL").expect("set ONESHOT_JELLYFIN_URL or ONESHOT_PLEX_URL");
    let id = oneshot_plex::PlexIdentity {
        product: "Flick".into(),
        version: "0.1.0".into(),
        client_identifier: "oneshot-e2e".into(),
        device_name: "e2e".into(),
        platform: "Windows".into(),
    };
    let d = ServerDescriptor {
        id: ServerId::new(),
        kind: ProviderKind::Plex,
        name: "pms".into(),
        remote_id: String::new(),
        base_url: url::Url::parse(&url).unwrap(),
        alternate_urls: vec![],
        version: None,
        user: UserProfile { id: "1".into(), name: "owner".into(), avatar: None, is_admin: true },
        disabled: false,
    };
    Arc::new(oneshot_plex::PlexProvider::new(d, http, id, String::new(), true))
}

#[tokio::main]
async fn main() {
    tracing_subscriber_init();
    let secs: u64 = std::env::var("E2E_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(3);
    let provider = provider().await;
    let caps = oneshot_capabilities::CapabilityManager::new().report();
    let mut settings = Settings::default();
    settings.advanced.presenter = PresenterChoice::DedicatedWindow;
    settings.audio.volume = 10;
    settings.playback.report_interval_secs = 5;
    settings.playback.max_bitrate = std::env::var("E2E_MAX_BITRATE").ok().and_then(|s| s.parse().ok());
    let only = std::env::var("E2E_ONLY").ok();

    let events: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    let ev2 = Arc::clone(&events);
    let player = Player::new(
        PlayerConfig {
            libmpv_path: None,
            search_dirs: vec!["third_party/mpv/windows-x64".into()],
            host: HostWindow::Other,
            dispatch: Arc::new(|f| f()),
            runtime: tokio::runtime::Handle::current(),
        },
        Arc::new(move |e| {
            if let PlayerEvent::Decision { decision } = &e {
                ev2.lock().unwrap().push(format!("decision update: video={:?}", decision.video));
            }
        }),
    );
    let (path, ver) = player.engine_info(&settings).expect("libmpv");
    println!("libmpv {path} api {ver:?}");

    let libs = provider.libraries().await.unwrap();
    let parent = libs.iter().find(|l| l.name == "Movies").map(|l| l.id.clone());
    let mut items = provider
        .items(&ItemQuery { parent, kinds: vec![ItemKind::Movie], filter: Default::default(), sort: Default::default(), order: Default::default(), start: 0, limit: 50 })
        .await
        .unwrap()
        .items;
    items.sort_by_key(|i| i.sort_title.clone().unwrap_or_else(|| i.title.clone()));

    for item in items.iter().filter(|i| only.as_deref().is_none_or(|o| i.title.contains(o))) {
        let long = item.runtime_ms.is_some_and(|r| r > 300_000);
        let start_ms = if long { Some(60_000) } else { None };
        println!("\n=== {} ({})", item.title, item.id);
        let decision = match player
            .play(Arc::clone(&provider), Arc::clone(&caps), settings.clone(), None, PlayRequest { item: Some(item.id.clone()), start_ms, ..Default::default() })
            .await
        {
            Ok(d) => d,
            Err(e) => {
                println!("  PLAY FAILED: {e}");
                continue;
            }
        };
        println!("  decision: {:?} | video {:?} | audio {:?} | subs {:?}", decision.label, decision.video, decision.audio, decision.subtitles);
        tokio::time::sleep(Duration::from_secs(secs)).await;
        let snap = player.snapshot();
        let stats = player.stats();
        println!("  phase={:?} pos={}ms dur={:?} presenter={:?} error={:?}", snap.phase, snap.position_ms, snap.duration_ms, snap.presenter, snap.error);
        println!("  hwdec={:?} ao={:?}", stats.hwdec, stats.current_ao);
        let vp = stats.video_params.as_ref();
        println!(
            "  video in: {:?} {:?} -> out {:?}",
            vp.and_then(|v| v.get("gamma")),
            vp.and_then(|v| v.get("primaries")),
            stats.video_target.as_ref().and_then(|v| v.get("gamma"))
        );
        println!("  audio out: {:?}", stats.audio_out.as_ref().and_then(|a| a.get("hr-channels")));
        let tracks: Vec<String> = snap.tracks.iter().map(|t| format!("{:?}#{}{}", t.kind, t.mpv_id, if t.selected { "*" } else { "" })).collect();
        println!("  tracks: {}", tracks.join(" "));
        if let Some(d) = &snap.decision
            && d.video != decision.video
        {
            println!("  RECONCILED video: {:?} ({})", d.video, d.reasons.last().map(|r| r.message.as_str()).unwrap_or(""));
        }
        if long {
            player.command(PlayerCommand::SeekAbsolute { ms: 150_000 }).unwrap();
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        player.command(PlayerCommand::Stop).unwrap();
        tokio::time::sleep(Duration::from_millis(800)).await;
        if long {
            let after = provider.item(&item.id).await.unwrap();
            println!("  server resume point after stop: {} ms (expected ~150000)", after.user.position_ms);
        }
    }
    for e in events.lock().unwrap().iter() {
        println!("{e}");
    }
    player.shutdown();
}

fn tracing_subscriber_init() {
    // Keep output readable: only warnings from mpv/player.
    let _ = std::env::var("RUST_LOG");
}
