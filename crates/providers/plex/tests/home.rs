use oneshot_core::Error;
use oneshot_plex::{PlexAuth, PlexIdentity};
use url::Url;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn auth(server: &MockServer) -> PlexAuth {
    let identity = PlexIdentity {
        product: "Flick".into(),
        version: "0".into(),
        client_identifier: "test".into(),
        device_name: "test".into(),
        platform: "Windows".into(),
    };
    PlexAuth::new(oneshot_net::reqwest::Client::new(), identity).with_base(Url::parse(&format!("{}/", server.uri())).unwrap())
}

#[tokio::test]
async fn lists_home_members() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/home/users"))
        .and(header("X-Plex-Token", "acct"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 1, "name": "Home",
            "users": [
                { "id": 11, "uuid": "u-admin", "title": "Antoine", "username": "antoine", "thumb": "https://plex.tv/users/u-admin/avatar", "admin": true, "restricted": false, "protected": false },
                { "id": 12, "uuid": "u-kid", "title": "Léa", "username": null, "thumb": null, "admin": false, "restricted": true, "protected": true }
            ]
        })))
        .mount(&server)
        .await;
    let members = auth(&server).home_users("acct").await.unwrap();
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].id, "11");
    assert_eq!(members[0].avatar.as_ref().unwrap().as_str(), "https://plex.tv/users/u-admin/avatar");
    assert_eq!(members[1].name, "Léa");
    assert!(members[1].protected);
}

#[tokio::test]
async fn switch_sends_the_pin_and_returns_the_member_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/home/users/u-kid/switch"))
        .and(query_param("pin", "1234"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 12, "uuid": "u-kid", "authToken": "kid-token" })))
        .mount(&server)
        .await;
    assert_eq!(auth(&server).switch_user("acct", "u-kid", Some("1234")).await.unwrap(), "kid-token");
}

#[tokio::test]
async fn a_wrong_switch_pin_is_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/api/v2/home/users/u-kid/switch")).respond_with(ResponseTemplate::new(401)).mount(&server).await;
    let err = auth(&server).switch_user("acct", "u-kid", Some("0000")).await.unwrap_err();
    assert!(matches!(err, Error::Unauthorized), "{err:?}");
}
