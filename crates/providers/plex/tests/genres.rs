//! Browsing by genre: Plex filters by genre *tag id* per library section, so
//! a genre name has to be resolved in the sections of the right kind.

use oneshot_core::media::ItemKind;
use oneshot_core::provider::MediaProvider;
use oneshot_core::query::{GenreQuery, SortBy, SortOrder};
use oneshot_core::server::{ProviderKind, ServerDescriptor, UserProfile};
use oneshot_core::ServerId;
use oneshot_plex::{PlexIdentity, PlexProvider};
use url::Url;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn provider(server: &MockServer) -> PlexProvider {
    let identity = PlexIdentity {
        product: "Flick".into(),
        version: "0".into(),
        client_identifier: "test".into(),
        device_name: "test".into(),
        platform: "Windows".into(),
    };
    let descriptor = ServerDescriptor {
        id: ServerId::new(),
        kind: ProviderKind::Plex,
        name: "pms".into(),
        remote_id: "pms".into(),
        base_url: Url::parse(&format!("{}/", server.uri())).unwrap(),
        alternate_urls: vec![],
        version: None,
        user: UserProfile { id: "11".into(), name: "Antoine".into(), avatar: None, is_admin: true },
        disabled: false,
        home_member: false,
    };
    PlexProvider::new(descriptor, oneshot_net::reqwest::Client::new(), identity, "srv".into(), true)
}

async fn json(server: &MockServer, p: &str, body: serde_json::Value) {
    Mock::given(method("GET")).and(path(p)).respond_with(ResponseTemplate::new(200).set_body_json(body)).mount(server).await;
}

/// A movie section (1), a show section (2) and a music section (3) that must never be asked.
async fn mount_libraries(server: &MockServer) {
    json(
        server,
        "/library/sections",
        serde_json::json!({ "MediaContainer": { "Directory": [
            { "key": "1", "type": "movie", "title": "Films" },
            { "key": "2", "type": "show", "title": "Séries" },
            { "key": "3", "type": "artist", "title": "Musique" }
        ] } }),
    )
    .await;
    Mock::given(method("GET")).and(path("/library/sections/3/genre")).respond_with(ResponseTemplate::new(500)).expect(0).mount(server).await;
    json(
        server,
        "/library/sections/1/genre",
        serde_json::json!({ "MediaContainer": { "Directory": [
            { "key": "19", "title": "Action" }, { "key": "21", "title": "Science-Fiction" }
        ] } }),
    )
    .await;
    json(
        server,
        "/library/sections/2/genre",
        serde_json::json!({ "MediaContainer": { "Directory": [
            { "key": "77", "title": "Drame" }, { "key": "19", "title": "Action" }
        ] } }),
    )
    .await;
}

#[tokio::test]
async fn genres_are_listed_from_the_sections_of_the_asked_kind() {
    let server = MockServer::start().await;
    mount_libraries(&server).await;
    let p = provider(&server);
    assert_eq!(p.genres(ItemKind::Movie).await.unwrap(), vec!["Action", "Science-Fiction"]);
    assert_eq!(p.genres(ItemKind::Series).await.unwrap(), vec!["Drame", "Action"]);
    assert!(p.genres(ItemKind::Episode).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_genre_name_is_resolved_to_the_tag_id_of_its_section() {
    let server = MockServer::start().await;
    mount_libraries(&server).await;
    Mock::given(method("GET"))
        .and(path("/library/sections/1/all"))
        .and(query_param("type", "1"))
        .and(query_param("genre", "21"))
        .and(query_param("sort", "titleSort:asc"))
        .and(query_param("X-Plex-Container-Size", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "MediaContainer": { "Metadata": [
            { "ratingKey": "10", "type": "movie", "title": "Dune" } ] } })))
        .expect(1)
        .mount(&server)
        .await;
    let query = GenreQuery { kind: ItemKind::Movie, genre: "science-fiction".into(), sort: SortBy::Title, order: SortOrder::Ascending, start: 0, limit: 50 };
    let items = provider(&server).by_genre(&query).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title, "Dune");
}

#[tokio::test]
async fn a_genre_the_server_does_not_have_is_an_empty_list_not_an_error() {
    let server = MockServer::start().await;
    mount_libraries(&server).await;
    // "Drame" exists only among the series: asking for movies must not query any section.
    let query = GenreQuery { kind: ItemKind::Movie, genre: "Drame".into(), sort: SortBy::Title, order: SortOrder::Ascending, start: 0, limit: 50 };
    assert!(provider(&server).by_genre(&query).await.unwrap().is_empty());
}
