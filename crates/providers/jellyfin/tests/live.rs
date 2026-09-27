//! Live integration tests against a real Jellyfin server.
//!
//! Skipped unless `ONESHOT_JELLYFIN_URL` is set. Start one with
//! `tools/dev-jellyfin.sh` (user oneshot/oneshot, synthetic corpus).

use oneshot_core::ServerId;
use oneshot_core::ids::ItemRef;
use oneshot_core::media::ItemKind;
use oneshot_core::playback::{
    ClientProfile, DeliveryKind, DeliveryRequest, PlaybackReport, PlaybackState, ReportKind, StreamRequest,
};
use oneshot_core::provider::MediaProvider;
use oneshot_core::query::ItemQuery;
use oneshot_core::server::{ProviderKind, ServerDescriptor, UserProfile};
use oneshot_core::settings::NetworkSettings;
use oneshot_core::stream::{AudioCodec, SubtitleFormat, VideoCodec};
use oneshot_jellyfin::{ClientIdentity, Connector, JellyfinProvider};

fn identity() -> ClientIdentity {
    ClientIdentity {
        client: "Flick-tests".into(),
        device_name: "ci".into(),
        device_id: "oneshot-live-tests".into(),
        version: "0.1.0".into(),
    }
}

fn full_profile() -> ClientProfile {
    ClientProfile {
        name: "Flick (libmpv)".into(),
        max_bitrate: None,
        video_codecs: vec![VideoCodec::H264, VideoCodec::Hevc, VideoCodec::Av1, VideoCodec::Vp9],
        audio_codecs: vec![
            AudioCodec::Aac,
            AudioCodec::Ac3,
            AudioCodec::Eac3,
            AudioCodec::Dts,
            AudioCodec::TrueHd,
            AudioCodec::Flac,
        ],
        containers: vec!["mkv".into(), "mp4".into()],
        subtitle_formats: vec![SubtitleFormat::Srt, SubtitleFormat::Ass, SubtitleFormat::Pgs],
        max_width: 7680,
        max_height: 4320,
        max_audio_channels: 8,
    }
}

async fn provider() -> Option<JellyfinProvider> {
    let url = std::env::var("ONESHOT_JELLYFIN_URL").ok()?;
    let http = oneshot_net::client(&NetworkSettings::default()).unwrap();
    let connector = Connector::new(http.clone(), identity());
    let (base, info) = connector.probe(&url).await.expect("server reachable");
    println!("server {} version {:?}", info.id, info.version);
    let session = connector.login(&base, "oneshot", "oneshot").await.expect("login");
    let descriptor = ServerDescriptor {
        id: ServerId::new(),
        kind: ProviderKind::Jellyfin,
        name: session.server_name.clone(),
        remote_id: session.server_id.clone(),
        base_url: base,
        alternate_urls: vec![],
        version: session.version.clone(),
        user: UserProfile { id: session.user_id.clone(), name: session.user_name.clone(), avatar: None, is_admin: session.is_admin },
        disabled: false,
    };
    Some(JellyfinProvider::new(descriptor, http, identity(), session.token))
}

async fn all_movies(p: &JellyfinProvider) -> Vec<oneshot_core::media::MediaItem> {
    // The library scan is asynchronous after creation: wait for the corpus.
    for _ in 0..30 {
        let page = p
            .items(&ItemQuery {
                parent: None,
                kinds: vec![ItemKind::Movie],
                filter: Default::default(),
                sort: Default::default(),
                order: Default::default(),
                start: 0,
                limit: 100,
            })
            .await
            .expect("items");
        if page.items.len() >= 8 {
            return page.items;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    panic!("library scan did not index the corpus");
}

#[tokio::test]
async fn catalogue_and_direct_play_negotiation() {
    let Some(p) = provider().await else {
        eprintln!("ONESHOT_JELLYFIN_URL not set: skipping");
        return;
    };
    let libs = p.libraries().await.expect("libraries");
    assert!(libs.iter().any(|l| l.name == "Movies"), "{libs:?}");
    assert!(p.admin().is_some(), "wizard user is an administrator");

    let movies = all_movies(&p).await;
    for m in &movies {
        let info = p.playback_info(&m.id, &full_profile()).await.expect("playback info");
        let offer = &info.offers[0];
        println!(
            "{:<45} direct_play={} reasons={:?} video={:?} audio={:?}",
            m.title,
            offer.policy.direct_play_allowed,
            offer.policy.server_reasons,
            offer.source.primary_video().map(|v| (&v.codec, v.width, &v.range)),
            offer.source.audio.iter().map(|a| (&a.codec, a.channels)).collect::<Vec<_>>()
        );
        assert!(
            offer.policy.direct_play_allowed,
            "server must not force a transcode for {} (reasons {:?})",
            m.title,
            offer.policy.server_reasons
        );
    }
}

#[tokio::test]
async fn progress_reporting_round_trips_to_resume_point() {
    let Some(p) = provider().await else { return };
    let movies = all_movies(&p).await;
    // Jellyfin only stores resume points for media longer than
    // MinResumeDurationSeconds (300 s): use the 7-minute corpus file.
    let movie = movies.iter().find(|m| m.runtime_ms.is_some_and(|r| r > 300_000)).expect("long corpus file");
    let info = p.playback_info(&movie.id, &full_profile()).await.unwrap();
    let source_id = info.offers[0].source.id.clone();
    let target = p
        .stream(&StreamRequest {
            item: movie.id.clone(),
            source_id: source_id.clone(),
            play_session_id: info.play_session_id.clone(),
            delivery: DeliveryRequest::Direct,
            audio_index: None,
            subtitle_index: None,
            start_ms: 0,
        })
        .await
        .unwrap();
    assert!(target.url.as_str().contains("static=true"));
    assert!(!target.url.as_str().contains("api_key"), "token must travel in headers, not the URL");

    let report = |kind, state, position_ms| PlaybackReport {
        kind,
        item: movie.id.clone(),
        source_id: source_id.clone(),
        play_session_id: info.play_session_id.clone(),
        state,
        position_ms,
        duration_ms: movie.runtime_ms,
        delivery: DeliveryKind::DirectPlay,
        audio_index: None,
        subtitle_index: None,
        volume: 100,
        muted: false,
    };
    p.report(&report(ReportKind::Start, PlaybackState::Playing, 0)).await.unwrap();
    p.report(&report(ReportKind::Progress, PlaybackState::Paused, 120_000)).await.unwrap();
    p.report(&report(ReportKind::Stop, PlaybackState::Stopped, 120_000)).await.unwrap();

    let refreshed = p.item(&movie.id).await.unwrap();
    // 120 s of 420 s (~29%) is inside Jellyfin's [5%, 90%] resume window.
    assert_eq!(refreshed.user.position_ms, 120_000, "resume point stored on server");

    p.set_played(&movie.id, true).await.unwrap();
    assert!(p.item(&movie.id).await.unwrap().user.played);
    p.set_played(&movie.id, false).await.unwrap();
    p.set_favorite(&movie.id, true).await.unwrap();
    assert!(p.item(&movie.id).await.unwrap().user.favorite);
    p.set_favorite(&movie.id, false).await.unwrap();
}

#[tokio::test]
async fn home_search_and_images() {
    let Some(p) = provider().await else { return };
    let movies = all_movies(&p).await;
    let rows = p.home().await.expect("home");
    println!("home rows: {:?}", rows.iter().map(|r| (&r.title, r.items.len())).collect::<Vec<_>>());
    let hits = p.search("hevc", 10).await.expect("search");
    assert!(!hits.is_empty());
    let with_poster = movies.iter().find_map(|m| m.images.poster.clone());
    if let Some(img) = with_poster {
        let url = p.image_url(&img, oneshot_core::media::ImageSize::Card).unwrap();
        assert!(url.as_str().contains("maxWidth=360"));
    }
    let markers = p.markers(&movies[0].id).await.expect("markers endpoint");
    assert!(markers.is_empty(), "no segment provider installed");
    let _ = ItemRef::new(ServerId::new(), "x");
}
