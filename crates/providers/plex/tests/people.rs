use oneshot_core::provider::MediaProvider;
use oneshot_core::server::{ProviderKind, ServerDescriptor, UserProfile};
use oneshot_core::{ItemRef, ServerId};
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

async fn mount_sections(server: &MockServer, actor: &str) {
    Mock::given(method("GET"))
        .and(path("/library/sections"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "MediaContainer": { "Directory": [
            { "key": "1", "type": "movie", "title": "Films" },
            { "key": "2", "type": "show", "title": "Séries" },
            { "key": "3", "type": "artist", "title": "Musique" }
        ] } })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/sections/1/all"))
        .and(query_param("actor", actor))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "MediaContainer": { "Metadata": [
            { "ratingKey": "10", "type": "movie", "title": "Forrest Gump" } ] } })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/sections/2/all"))
        .and(query_param("actor", actor))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "MediaContainer": { "Metadata": [
            { "ratingKey": "20", "type": "show", "title": "Band of Brothers" } ] } })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/sections/3/all"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(server)
        .await;
}

#[tokio::test]
async fn titles_of_an_actor_of_this_server_use_their_tag_id() {
    let server = MockServer::start().await;
    mount_sections(&server, "4242").await;
    let p = provider(&server);
    let hint = ItemRef::new(p.descriptor().id, "4242");
    let keys: Vec<String> = p.person_items("Tom Hanks", Some(&hint)).await.unwrap().into_iter().map(|i| i.id.key).collect();
    assert_eq!(keys, ["10", "20"], "movie and show sections, music left alone");
}

#[tokio::test]
async fn an_actor_from_elsewhere_is_found_in_the_search_hubs() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/hubs/search"))
        .and(query_param("query", "Tom Hanks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "MediaContainer": { "Hub": [
            { "type": "movie", "title": "Movies", "Metadata": [] },
            { "type": "actor", "title": "People", "Directory": [ { "tag": "Tom Hankson", "id": 1 }, { "tag": "tom hanks", "id": "4242" } ] }
        ] } })))
        .mount(&server)
        .await;
    mount_sections(&server, "4242").await;
    assert_eq!(provider(&server).person_items("Tom Hanks", None).await.unwrap().len(), 2);
}

#[tokio::test]
async fn an_actor_hub_with_metadata_entries_works_too() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/hubs/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "MediaContainer": { "Hub": [
            { "type": "actor", "title": "People", "Metadata": [ { "ratingKey": "4242", "type": "person", "title": "Tom Hanks" } ] }
        ] } })))
        .mount(&server)
        .await;
    mount_sections(&server, "4242").await;
    assert_eq!(provider(&server).person_items("Tom Hanks", None).await.unwrap().len(), 2);
}

#[tokio::test]
async fn nobody_of_that_name_means_no_titles() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/hubs/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "MediaContainer": { "Hub": [] } })))
        .mount(&server)
        .await;
    assert!(provider(&server).person_items("Tom Hanks", None).await.unwrap().is_empty());
}
