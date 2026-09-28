use oneshot_core::provider::MediaProvider;
use oneshot_core::server::{LibraryKind, ProviderKind, ServerDescriptor, UserProfile};
use oneshot_core::ServerId;
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
        base_url: Url::parse(&server.uri()).unwrap(),
        alternate_urls: vec![],
        version: None,
        user: UserProfile { id: "u1".into(), name: "Antoine".into(), avatar: None, is_admin: false },
        disabled: false,
        home_member: false,
    };
    JellyfinProvider::new(descriptor, oneshot_net::reqwest::Client::new(), identity, "tok".into())
}

async fn mount_count(server: &MockServer, parent: &str, kind: &str, total: u32) {
    Mock::given(method("GET"))
        .and(path("/Items"))
        .and(query_param("ParentId", parent))
        .and(query_param("IncludeItemTypes", kind))
        .and(query_param("Recursive", "true"))
        .and(query_param("Limit", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "Items": [], "TotalRecordCount": total })))
        .expect(1)
        .mount(server)
        .await;
}

/// A library's `ChildCount` is its top-level folders, not its titles: the
/// count is the number of movies or series the library grid lists.
#[tokio::test]
async fn library_counts_are_titles_not_folders() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/UserViews"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "Items": [
            { "Id": "films", "Name": "Films", "Type": "CollectionFolder", "CollectionType": "movies", "ChildCount": 8 },
            { "Id": "series", "Name": "Séries", "Type": "CollectionFolder", "CollectionType": "tvshows", "ChildCount": 9 },
            { "Id": "pl", "Name": "Playlists", "Type": "UserView", "CollectionType": "playlists", "ChildCount": 3 },
        ], "TotalRecordCount": 3 })))
        .mount(&server)
        .await;
    mount_count(&server, "films", "Movie", 214).await;
    mount_count(&server, "series", "Series", 8).await;

    let libs = provider(&server).libraries().await.unwrap();
    let count = |kind| libs.iter().find(|l| l.kind == kind).and_then(|l| l.item_count);
    assert_eq!(count(LibraryKind::Movies), Some(214));
    assert_eq!(count(LibraryKind::Shows), Some(8));
    assert_eq!(count(LibraryKind::Other), None);
}
