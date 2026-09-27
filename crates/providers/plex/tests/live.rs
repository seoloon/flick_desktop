//! Live tests against a real (unclaimed) Plex Media Server.
//! Skipped unless `ONESHOT_PLEX_URL` is set; see `tools/dev-plex.sh`.

use oneshot_core::ServerId;
use oneshot_core::media::ItemKind;
use oneshot_core::playback::{
    ClientProfile, DeliveryKind, DeliveryRequest, PlaybackReport, PlaybackState, ReportKind, StreamRequest,
};
use oneshot_core::provider::MediaProvider;
use oneshot_core::query::ItemQuery;
use oneshot_core::server::{ProviderKind, ServerDescriptor, UserProfile};
use oneshot_core::settings::NetworkSettings;
use oneshot_core::stream::{AudioCodec, DynamicRange, SubtitleFormat, VideoCodec};
use oneshot_plex::{PlexIdentity, PlexProvider};
use url::Url;

fn provider() -> Option<PlexProvider> {
    let url = std::env::var("ONESHOT_PLEX_URL").ok()?;
    let identity = PlexIdentity {
        product: "Flick".into(),
        version: "0.1.0".into(),
        client_identifier: "oneshot-live-tests".into(),
        device_name: "ci".into(),
        platform: "Windows".into(),
    };
    let descriptor = ServerDescriptor {
        id: ServerId::new(),
        kind: ProviderKind::Plex,
        name: "test-pms".into(),
        remote_id: String::new(),
        base_url: Url::parse(&url).unwrap(),
        alternate_urls: vec![],
        version: None,
        user: UserProfile { id: "1".into(), name: "owner".into(), avatar: None, is_admin: true },
        disabled: false,
        home_member: false,
    };
    let http = oneshot_net::client(&NetworkSettings::default()).unwrap();
    // Unclaimed server + allowed network: no token needed.
    Some(PlexProvider::new(descriptor, http, identity, String::new(), true))
}

fn profile() -> ClientProfile {
    ClientProfile {
        name: "Flick (libmpv)".into(),
        max_bitrate: None,
        video_codecs: vec![VideoCodec::H264, VideoCodec::Hevc, VideoCodec::Av1, VideoCodec::Vp9],
        audio_codecs: vec![AudioCodec::Aac, AudioCodec::Ac3, AudioCodec::Eac3, AudioCodec::Dts, AudioCodec::TrueHd, AudioCodec::Flac],
        containers: vec!["mkv".into(), "mp4".into()],
        subtitle_formats: vec![SubtitleFormat::Srt, SubtitleFormat::Ass, SubtitleFormat::Pgs],
        max_width: 7680,
        max_height: 4320,
        max_audio_channels: 8,
    }
}

#[tokio::test]
async fn catalogue_decision_stream_and_timeline() {
    let Some(p) = provider() else {
        eprintln!("ONESHOT_PLEX_URL not set: skipping");
        return;
    };
    let libs = p.libraries().await.expect("sections");
    let movies_lib = libs.iter().find(|l| l.name == "Movies").expect("Movies section");
    let page = p
        .items(&ItemQuery {
            parent: Some(movies_lib.id.clone()),
            kinds: vec![ItemKind::Movie],
            filter: Default::default(),
            sort: Default::default(),
            order: Default::default(),
            start: 0,
            limit: 50,
        })
        .await
        .expect("items");
    assert_eq!(page.items.len(), 8, "{:?}", page.items.iter().map(|i| &i.title).collect::<Vec<_>>());

    for m in &page.items {
        let info = p.playback_info(&m.id, &profile()).await.expect("decision");
        let o = &info.offers[0];
        println!(
            "{:<45} direct_play={} reasons={:?} range={:?}",
            m.title,
            o.policy.direct_play_allowed,
            o.policy.server_reasons,
            o.source.primary_video().map(|v| &v.range)
        );
        assert!(o.policy.direct_play_allowed, "Plex must allow direct play of {}", m.title);
    }

    let hdr = page.items.iter().find(|m| m.title.contains("hdr10")).unwrap();
    let detail = p.item(&hdr.id).await.unwrap();
    assert_eq!(detail.sources[0].primary_video().unwrap().range, DynamicRange::Hdr10);

    let long = page.items.iter().find(|m| m.title.contains("long")).unwrap();
    let info = p.playback_info(&long.id, &profile()).await.unwrap();
    let source_id = info.offers[0].source.id.clone();
    let target = p
        .stream(&StreamRequest {
            item: long.id.clone(),
            source_id: source_id.clone(),
            play_session_id: info.play_session_id.clone(),
            delivery: DeliveryRequest::Direct,
            audio_index: None,
            subtitle_index: None,
            start_ms: 0,
        })
        .await
        .unwrap();
    assert!(target.url.path().starts_with("/library/parts/"), "{}", target.url);
    let head = reqwest_head(&target.url).await;
    assert!(head.starts_with("video/") || head.contains("matroska"), "part served as {head}");

    let report = |kind, state, position_ms| PlaybackReport {
        kind,
        item: long.id.clone(),
        source_id: source_id.clone(),
        play_session_id: info.play_session_id.clone(),
        state,
        position_ms,
        duration_ms: long.runtime_ms,
        delivery: DeliveryKind::DirectPlay,
        audio_index: None,
        subtitle_index: None,
        volume: 100,
        muted: false,
    };
    p.report(&report(ReportKind::Start, PlaybackState::Playing, 0)).await.unwrap();
    p.report(&report(ReportKind::Progress, PlaybackState::Paused, 120_000)).await.unwrap();
    p.report(&report(ReportKind::Stop, PlaybackState::Stopped, 120_000)).await.unwrap();
    let after = p.item(&long.id).await.unwrap();
    assert_eq!(after.user.position_ms, 120_000, "viewOffset stored via /:/timeline");

    p.set_played(&long.id, true).await.unwrap();
    assert!(p.item(&long.id).await.unwrap().user.played);
    p.set_played(&long.id, false).await.unwrap();
    assert!(p.set_favorite(&long.id, true).await.is_err(), "Plex favourites must be reported unsupported");

    let rows = p.home().await.expect("hubs");
    println!("home: {:?}", rows.iter().map(|r| (&r.title, r.items.len())).collect::<Vec<_>>());
    let hits = p.search("hevc", 10).await.expect("search");
    println!("search: {:?}", hits.iter().map(|h| &h.title).collect::<Vec<_>>());
    let admin = p.admin().expect("owned");
    let info = admin.server_info().await.expect("server info");
    println!("admin: {} {}", info.name, info.version);
    admin.sessions().await.expect("sessions");
}

async fn reqwest_head(url: &Url) -> String {
    let c = oneshot_net::client(&NetworkSettings::default()).unwrap();
    let r = c.get(url.clone()).header("Range", "bytes=0-1023").send().await.unwrap();
    assert!(r.status().is_success(), "{}", r.status());
    r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned()
}

/// Confirms the Plex Home endpoints and their JSON shape on a real account:
/// `PLEX_ACCOUNT_TOKEN=… cargo test -p oneshot-plex --test live home_users_live -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn home_users_live() {
    let Ok(token) = std::env::var("PLEX_ACCOUNT_TOKEN") else { return };
    let identity = oneshot_plex::PlexIdentity {
        product: "Flick".into(),
        version: "0".into(),
        client_identifier: "flick-live-test".into(),
        device_name: "live test".into(),
        platform: "Windows".into(),
    };
    let auth = oneshot_plex::PlexAuth::new(oneshot_net::reqwest::Client::new(), identity);
    let members = auth.home_users(&token).await.expect("home users");
    for m in &members {
        println!("{} uuid={} protected={} admin={}", m.name, m.uuid, m.protected, m.admin);
    }
    assert!(!members.is_empty(), "an account is always a member of its own home");
}

/// Confirms the Watchlist endpoint and its JSON shape on a real account:
/// `PLEX_ACCOUNT_TOKEN=… cargo test -p oneshot-plex --test live watchlist_live -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn watchlist_live() {
    let Ok(token) = std::env::var("PLEX_ACCOUNT_TOKEN") else { return };
    let identity = PlexIdentity {
        product: "Flick".into(),
        version: "0".into(),
        client_identifier: "flick-live-test".into(),
        device_name: "live test".into(),
        platform: "Windows".into(),
    };
    let watchlist = oneshot_plex::Watchlist::new(oneshot_net::reqwest::Client::new(), identity, token);
    for entry in watchlist.entries().await.expect("watchlist") {
        println!("{} — {}", entry.title, entry.guid);
    }
}

/// Confirms the actor search hubs on a real server:
/// `ONESHOT_PLEX_URL=… ONESHOT_PLEX_TOKEN=… ONESHOT_PLEX_ACTOR="Tom Hanks" cargo test -p oneshot-plex --test live actor_search_live -- --ignored --nocapture`
/// Nothing found while the actor is in the library → print the raw
/// `hubs/search?query=` body and adapt `HubTag`.
#[tokio::test]
#[ignore]
async fn actor_search_live() {
    let (Ok(url), Ok(token), Ok(actor)) = (std::env::var("ONESHOT_PLEX_URL"), std::env::var("ONESHOT_PLEX_TOKEN"), std::env::var("ONESHOT_PLEX_ACTOR")) else {
        return;
    };
    let identity = PlexIdentity {
        product: "Flick".into(),
        version: "0".into(),
        client_identifier: "flick-live-test".into(),
        device_name: "live test".into(),
        platform: "Windows".into(),
    };
    let descriptor = ServerDescriptor {
        id: ServerId::new(),
        kind: ProviderKind::Plex,
        name: "live".into(),
        remote_id: String::new(),
        base_url: Url::parse(&url).unwrap(),
        alternate_urls: vec![],
        version: None,
        user: UserProfile { id: "1".into(), name: "owner".into(), avatar: None, is_admin: true },
        disabled: false,
        home_member: false,
    };
    let provider = PlexProvider::new(descriptor, oneshot_net::client(&NetworkSettings::default()).unwrap(), identity, token, true);
    for item in provider.person_items(&actor, None).await.expect("person items") {
        println!("{} ({:?})", item.title, item.year);
    }
}
