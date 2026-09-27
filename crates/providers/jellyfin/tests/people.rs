use oneshot_core::provider::MediaProvider;
use oneshot_core::server::{ProviderKind, ServerDescriptor, UserProfile};
use oneshot_core::{ItemRef, ServerId};
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

async fn mount_items_of(server: &MockServer, person: &str, calls: u64) {
    Mock::given(method("GET"))
        .and(path("/Items"))
        .and(query_param("PersonIds", person))
        .and(query_param("IncludeItemTypes", "Movie,Series"))
        .and(query_param("Recursive", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "Items": [{ "Id": "m1", "Name": "Forrest Gump", "Type": "Movie" }], "TotalRecordCount": 1 })))
        .expect(calls)
        .mount(server)
        .await;
}

#[tokio::test]
async fn reads_a_person_of_this_server() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Items/p1"))
        .and(query_param("userId", "u1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "Id": "p1", "Name": "Tom Hanks", "Type": "Person", "Overview": "Bio",
            "PremiereDate": "1956-07-09T00:00:00.0000000Z", "ProductionLocations": ["Concord"],
            "ProviderIds": { "Tmdb": "31" }, "ImageTags": { "Primary": "t1" } })))
        .mount(&server)
        .await;
    let p = provider(&server);
    let id = ItemRef::new(p.descriptor().id, "p1");
    let person = p.person(&id).await.unwrap();
    assert_eq!(person.name, "Tom Hanks");
    assert_eq!(person.overview.as_deref(), Some("Bio"));
    assert_eq!(person.birth.unwrap().to_string(), "1956-07-09");
    assert_eq!(person.birthplace.as_deref(), Some("Concord"));
    assert_eq!(person.external_ids.tmdb.as_deref(), Some("31"));
    let image = person.image.unwrap();
    assert_eq!(image.tag, "Primary/t1");
    assert_eq!(image.item.key, "p1");
}

#[tokio::test]
async fn titles_of_a_person_of_this_server_use_their_id() {
    let server = MockServer::start().await;
    mount_items_of(&server, "p1", 1).await;
    let p = provider(&server);
    let hint = ItemRef::new(p.descriptor().id, "p1");
    let items = p.person_items("Tom Hanks", Some(&hint)).await.unwrap();
    assert_eq!(items.iter().map(|i| i.id.key.as_str()).collect::<Vec<_>>(), ["m1"]);
}

#[tokio::test]
async fn titles_of_a_person_from_elsewhere_are_found_by_exact_name() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Persons"))
        .and(query_param("searchTerm", "Tom Hanks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "Items": [
            { "Id": "p9", "Name": "Tom Hankson", "Type": "Person" },
            { "Id": "p1", "Name": "tom hanks", "Type": "Person" } ] })))
        .mount(&server)
        .await;
    mount_items_of(&server, "p1", 1).await;
    let p = provider(&server);
    let elsewhere = ItemRef::new(ServerId::new(), "4242");
    assert_eq!(p.person_items("Tom Hanks", Some(&elsewhere)).await.unwrap().len(), 1);
}

#[tokio::test]
async fn nobody_of_that_name_means_no_titles() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Persons"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "Items": [
            { "Id": "p9", "Name": "Tom Hankson", "Type": "Person" } ] })))
        .mount(&server)
        .await;
    mount_items_of(&server, "p9", 0).await;
    assert!(provider(&server).person_items("Tom Hanks", None).await.unwrap().is_empty());
}
