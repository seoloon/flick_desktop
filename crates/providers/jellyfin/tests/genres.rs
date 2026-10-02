//! Browsing by genre across every library of a Jellyfin server.

use oneshot_core::ServerId;
use oneshot_core::media::ItemKind;
use oneshot_core::provider::MediaProvider;
use oneshot_core::query::{GenreQuery, SortBy, SortOrder};
use oneshot_core::server::{ProviderKind, ServerDescriptor, UserProfile};
use oneshot_jellyfin::{ClientIdentity, JellyfinProvider};
use url::Url;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn provider(server: &MockServer) -> JellyfinProvider {
    let identity = ClientIdentity { client: "Flick".into(), device_name: "test".into(), device_id: "dev".into(), version: "0".into() };
    let descriptor = ServerDescriptor {
        id: ServerId::new(),
        kind: ProviderKind::Jellyfin,
        name: "jf".into(),
        remote_id: "jf".into(),
        base_url: Url::parse(&format!("{}/", server.uri())).unwrap(),
        alternate_urls: vec![],
        version: None,
        user: UserProfile { id: "u1".into(), name: "Antoine".into(), avatar: None, is_admin: true },
        disabled: false,
        home_member: false,
    };
    JellyfinProvider::new(descriptor, oneshot_net::reqwest::Client::new(), identity, "tok".into())
}

async fn mount_genres(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/Genres"))
        .and(query_param("IncludeItemTypes", "Movie"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "Items": [{ "Id": "g1", "Name": "Action", "Type": "Genre" }, { "Id": "g2", "Name": "Science Fiction", "Type": "Genre" }],
            "TotalRecordCount": 2
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn lists_the_genres_of_the_asked_kind() {
    let server = MockServer::start().await;
    mount_genres(&server).await;
    assert_eq!(provider(&server).genres(ItemKind::Movie).await.unwrap(), vec!["Action", "Science Fiction"]);
}

#[tokio::test]
async fn lists_titles_of_a_genre_with_the_servers_own_spelling_and_no_library_restriction() {
    let server = MockServer::start().await;
    mount_genres(&server).await;
    Mock::given(method("GET"))
        .and(path("/Items"))
        .and(query_param("Genres", "Science Fiction"))
        .and(query_param("IncludeItemTypes", "Movie"))
        .and(query_param("Recursive", "true"))
        .and(query_param("SortBy", "Random"))
        .and(query_param("Limit", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "Items": [{ "Id": "m1", "Name": "Dune", "Type": "Movie" }],
            "TotalRecordCount": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    let query = GenreQuery { kind: ItemKind::Movie, genre: "science fiction".into(), sort: SortBy::Random, order: SortOrder::Ascending, start: 0, limit: 1 };
    let items = provider(&server).by_genre(&query).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title, "Dune");
}

#[tokio::test]
async fn an_unknown_genre_asks_for_no_titles() {
    let server = MockServer::start().await;
    mount_genres(&server).await;
    Mock::given(method("GET")).and(path("/Items")).respond_with(ResponseTemplate::new(500)).expect(0).mount(&server).await;
    let query = GenreQuery { kind: ItemKind::Movie, genre: "Western".into(), sort: SortBy::Title, order: SortOrder::Ascending, start: 0, limit: 50 };
    assert!(provider(&server).by_genre(&query).await.unwrap().is_empty());
}
