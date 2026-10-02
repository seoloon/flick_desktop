//! The connection test against a canned HTTP server: each misconfiguration
//! must end on the step (and the advice) that names it.

use async_trait::async_trait;
use oneshot_flicksync::auth::TokenProvider;
use oneshot_flicksync::diagnose::{self, Status, Step};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

struct StaticToken;

#[async_trait]
impl TokenProvider for StaticToken {
    async fn token(&self) -> oneshot_flicksync::Result<String> {
        Ok("test-token".into())
    }
}

/// `(status line, extra headers, body)` per request path.
type Reply = (&'static str, &'static str, &'static str);

async fn serve(routes: fn(&str) -> Reply) -> Url {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { return };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let (status, headers, body) = routes(&path);
                let resp = format!("HTTP/1.1 {status}\r\ncontent-type: application/json\r\n{headers}content-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
                let _ = stream.write_all(resp.as_bytes()).await;
            });
        }
    });
    base
}

fn status_of(r: &diagnose::Report, step: Step) -> Status {
    r.checks.iter().find(|c| c.step == step).unwrap().status
}

fn detail_of(r: &diagnose::Report, step: Step) -> &str {
    &r.checks.iter().find(|c| c.step == step).unwrap().detail
}

#[tokio::test]
async fn a_healthy_server_passes_every_step() {
    let base = serve(|path| match path {
        "/health" => ("200 OK", "", r#"{"status":"ok","version":"1.2.3"}"#),
        "/ready" => ("200 OK", "", r#"{"status":"ready"}"#),
        _ => ("404 Not Found", "", r#"{"error":{"code":"ROOM_NOT_FOUND","message":"no such room"}}"#),
    })
    .await;
    let r = diagnose::run(&oneshot_net::reqwest::Client::new(), &base, &StaticToken).await;
    assert!(r.passed(), "{r:?}");
    assert!(detail_of(&r, Step::Reach).contains("1.2.3"));
    assert_eq!(status_of(&r, Step::Auth), Status::Ok);
    // No Date header from the canned server: the clock step is skipped, not failed.
    assert_eq!(status_of(&r, Step::Clock), Status::Skipped);
}

#[tokio::test]
async fn a_rejected_key_says_which_part_to_check() {
    let base = serve(|path| match path {
        "/health" => ("200 OK", "", r#"{"status":"ok"}"#),
        "/ready" => ("200 OK", "", r#"{"status":"ready"}"#),
        _ => ("401 Unauthorized", "", r#"{"error":{"code":"UNAUTHENTICATED","message":"unknown signing key"}}"#),
    })
    .await;
    let r = diagnose::run(&oneshot_net::reqwest::Client::new(), &base, &StaticToken).await;
    assert_eq!(status_of(&r, Step::Reach), Status::Ok);
    assert_eq!(status_of(&r, Step::Auth), Status::Failed);
    assert!(detail_of(&r, Step::Auth).contains("new invitation link"));
    assert!(r.reachable_and_ready(), "a refused key does not stop the address from being kept");
}

#[tokio::test]
async fn a_server_without_keys_is_reported_before_auth_is_blamed() {
    let base = serve(|path| match path {
        "/health" => ("200 OK", "", r#"{"status":"ok"}"#),
        "/ready" => ("503 Service Unavailable", "", r#"{"status":"not_ready"}"#),
        _ => ("401 Unauthorized", "", r#"{"error":{"code":"UNAUTHENTICATED","message":"unknown signing key"}}"#),
    })
    .await;
    let r = diagnose::run(&oneshot_net::reqwest::Client::new(), &base, &StaticToken).await;
    assert_eq!(status_of(&r, Step::Ready), Status::Failed);
    assert_eq!(status_of(&r, Step::Auth), Status::Skipped);
    assert!(!r.reachable_and_ready());
}

#[tokio::test]
async fn something_that_is_not_flicksync_is_not_mistaken_for_it() {
    let base = serve(|_| ("200 OK", "", r#"<html>hello</html>"#)).await;
    let r = diagnose::run(&oneshot_net::reqwest::Client::new(), &base, &StaticToken).await;
    assert_eq!(status_of(&r, Step::Reach), Status::Failed);
    assert!(detail_of(&r, Step::Reach).contains("isn't FlickSync"));
    assert_eq!(status_of(&r, Step::Auth), Status::Skipped);
}

#[tokio::test]
async fn a_skewed_clock_is_named() {
    // Far in the past: any machine's clock is "ahead" of it.
    let base = serve(|path| match path {
        "/health" => ("200 OK", "date: Tue, 15 Nov 1994 08:12:31 GMT\r\n", r#"{"status":"ok"}"#),
        "/ready" => ("200 OK", "", r#"{"status":"ready"}"#),
        _ => ("404 Not Found", "", r#"{"error":{"code":"ROOM_NOT_FOUND","message":"x"}}"#),
    })
    .await;
    let r = diagnose::run(&oneshot_net::reqwest::Client::new(), &base, &StaticToken).await;
    assert_eq!(status_of(&r, Step::Clock), Status::Failed);
    assert!(detail_of(&r, Step::Clock).contains("ahead of"));
}

#[tokio::test]
async fn nothing_listening_is_a_reach_failure() {
    // Bind then drop to get a port that is certainly closed.
    let port = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
    let base = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
    let r = diagnose::run(&oneshot_net::reqwest::Client::new(), &base, &StaticToken).await;
    assert_eq!(status_of(&r, Step::Reach), Status::Failed);
    assert_eq!(status_of(&r, Step::Ready), Status::Skipped);
    assert!(!r.reachable_and_ready());
}
