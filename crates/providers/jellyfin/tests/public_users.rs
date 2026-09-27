use oneshot_jellyfin::{ClientIdentity, Connector};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn connector() -> Connector {
    let identity = ClientIdentity {
        client: "Flick".into(),
        device_name: "test".into(),
        device_id: "dev".into(),
        version: "0".into(),
    };
    Connector::new(oneshot_net::reqwest::Client::new(), identity)
}

#[tokio::test]
async fn lists_the_sign_in_screen_users() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Users/Public"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "Id": "a1",
                    "Name": "Antoine",
                    "PrimaryImageTag": "t1",
                    "HasPassword": true
                },
                { "Id": "k2", "Name": "Kid", "HasPassword": false }
            ])),
        )
        .mount(&server)
        .await;
    let base = Url::parse(&server.uri()).unwrap();
    let users = connector().public_users(&base).await.unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].name, "Antoine");
    assert!(users[0].has_password);
    let avatar = users[0].avatar.as_ref().unwrap();
    assert_eq!(avatar.path(), "/Users/a1/Images/Primary");
    assert!(avatar.query().unwrap().contains("tag=t1"));
    assert!(!users[1].has_password);
    assert!(users[1].avatar.is_none());
}
