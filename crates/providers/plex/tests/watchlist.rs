use oneshot_core::provider::MediaProvider;
use oneshot_core::server::{ProviderKind, ServerDescriptor, UserProfile};
use oneshot_core::{Error, ServerId};
use oneshot_plex::{PlexIdentity, PlexProvider, Watchlist};
use url::Url;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn identity() -> PlexIdentity {
    PlexIdentity {
        product: "Flick".into(),
        version: "0".into(),
        client_identifier: "test".into(),
        device_name: "test".into(),
        platform: "Windows".into(),
    }
}

fn base(server: &MockServer) -> Url {
    Url::parse(&format!("{}/", server.uri())).unwrap()
}

fn watchlist(server: &MockServer) -> Watchlist {
    Watchlist::new(oneshot_net::reqwest::Client::new(), identity(), "acct".into()).with_bases(base(server), base(server))
}

async fn mount_watchlist(server: &MockServer, calls: u64) {
    Mock::given(method("GET"))
        .and(path("/library/sections/watchlist/all"))
        .and(header("X-Plex-Token", "acct"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "MediaContainer": { "Metadata": [
            { "ratingKey": "5d77a", "type": "movie", "title": "Dune", "guid": "plex://movie/5d77a" },
            { "ratingKey": "5d77b", "type": "show", "title": "Severance", "guid": "plex://show/5d77b" }
        ] } })))
        .expect(calls)
        .mount(server)
        .await;
}

#[tokio::test]
async fn reads_the_watchlist_once_then_from_cache() {
    let server = MockServer::start().await;
    mount_watchlist(&server, 1).await;
    let w = watchlist(&server);
    let guids: Vec<String> = w.entries().await.unwrap().into_iter().map(|e| e.guid).collect();
    assert_eq!(guids, ["plex://movie/5d77a", "plex://show/5d77b"]);
    assert!(w.contains("plex://show/5d77b").await.unwrap());
    assert!(!w.contains("plex://movie/other").await.unwrap());
}

#[tokio::test]
async fn adds_and_removes_by_discover_key() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/actions/addToWatchlist"))
        .and(query_param("ratingKey", "5d77a"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/actions/removeFromWatchlist"))
        .and(query_param("ratingKey", "5d77a"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let w = watchlist(&server);
    w.set("plex://movie/5d77a", true).await.unwrap();
    w.set("plex://movie/5d77a", false).await.unwrap();
    assert!(matches!(w.set("local://123", true).await, Err(Error::Unsupported(_))), "only Plex catalogue titles can be watchlisted");
}

#[tokio::test]
async fn a_token_of_another_account_is_not_used() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 99, "uuid": "u99", "title": "Someone else" })))
        .mount(&server)
        .await;
    mount_watchlist(&server, 0).await;
    let w = watchlist(&server).for_user("11");
    assert!(matches!(w.entries().await, Err(Error::Unsupported(_))));
}

#[tokio::test]
async fn favourites_are_the_library_copies_of_watchlisted_titles() {
    let server = MockServer::start().await;
    mount_watchlist(&server, 1).await;
    Mock::given(method("GET"))
        .and(path("/library/all"))
        .and(query_param("guid", "plex://movie/5d77a"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "MediaContainer": { "Metadata": [
            { "ratingKey": "42", "type": "movie", "title": "Dune", "guid": "plex://movie/5d77a" }
        ] } })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/all"))
        .and(query_param("guid", "plex://show/5d77b"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "MediaContainer": { "size": 0 } })))
        .mount(&server)
        .await;
    let descriptor = ServerDescriptor {
        id: ServerId::new(),
        kind: ProviderKind::Plex,
        name: "pms".into(),
        remote_id: "pms".into(),
        base_url: base(&server),
        alternate_urls: vec![],
        version: None,
        user: UserProfile { id: "11".into(), name: "Antoine".into(), avatar: None, is_admin: true },
        disabled: false,
        home_member: false,
    };
    let provider = PlexProvider::new(descriptor, oneshot_net::reqwest::Client::new(), identity(), "srv".into(), true).with_watchlist(watchlist(&server));

    let favs = provider.favorites(50).await.unwrap();
    assert_eq!(favs.len(), 1, "a watchlisted title that is not on this server is left out");
    assert_eq!(favs[0].id.key, "42");
    assert!(favs[0].user.favorite);
}

#[tokio::test]
async fn without_a_plex_tv_sign_in_there_are_no_favourites() {
    let descriptor = ServerDescriptor {
        id: ServerId::new(),
        kind: ProviderKind::Plex,
        name: "pms".into(),
        remote_id: "pms".into(),
        base_url: Url::parse("http://pms.local/").unwrap(),
        alternate_urls: vec![],
        version: None,
        user: UserProfile { id: "11".into(), name: "Antoine".into(), avatar: None, is_admin: true },
        disabled: false,
        home_member: false,
    };
    let provider = PlexProvider::new(descriptor, oneshot_net::reqwest::Client::new(), identity(), "srv".into(), true);
    assert!(matches!(provider.favorites(50).await, Err(Error::Unsupported(_))));
}

#[tokio::test]
async fn reads_every_page_of_a_long_watchlist_in_pages_plex_tv_accepts() {
    let server = MockServer::start().await;
    // plex.tv answers 400 to page sizes it does not accept; unmatched
    // requests get a 404 here, so a wrong size fails the test too.
    Mock::given(method("GET"))
        .and(path("/library/sections/watchlist/all"))
        .and(query_param("X-Plex-Container-Size", "100"))
        .and(query_param("X-Plex-Container-Start", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "MediaContainer": { "totalSize": 101, "Metadata": [
            // Discover's own shape: tag ids are strings, not numbers.
            { "ratingKey": "5d77a", "type": "movie", "title": "Dune", "guid": "plex://movie/5d77a",
              "Genre": [{ "id": "5d7768254de0ee001fcc8034", "tag": "Science Fiction" }] }
        ] } })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/sections/watchlist/all"))
        .and(query_param("X-Plex-Container-Size", "100"))
        .and(query_param("X-Plex-Container-Start", "100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "MediaContainer": { "totalSize": 101, "Metadata": [
            { "ratingKey": "5d77c", "type": "show", "title": "Andor", "guid": "plex://show/5d77c" }
        ] } })))
        .mount(&server)
        .await;
    let titles: Vec<String> = watchlist(&server).entries().await.unwrap().into_iter().map(|e| e.title).collect();
    assert_eq!(titles, ["Dune", "Andor"]);
}

