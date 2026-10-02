//! "Test the connection": walks the path a real session takes, one step at a
//! time, and says which step breaks and what to do about it.
//!
//! Nothing here creates a room. Authentication is probed by reading a room
//! that cannot exist: the server authenticates the caller before it looks at
//! the id, so a bad key answers 401 and a good one answers 404.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use oneshot_net::reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::auth::TokenProvider;
use crate::errors::Error;

const TIMEOUT: Duration = Duration::from_secs(8);
/// FlickSync tolerates 30 s of clock skew by default; warn a little before.
const MAX_SKEW_SECS: i64 = 25;
/// Never a real room id: only there to make the server authenticate us.
const PROBE_ROOM: &str = "DIAGNOSTIC";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Config,
    Reach,
    Ready,
    Clock,
    Auth,
}

const STEPS: [Step; 5] = [Step::Config, Step::Reach, Step::Ready, Step::Clock, Step::Auth];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub step: Step,
    pub status: Status,
    /// English, actionable. The UI shows it as is.
    pub detail: String,
}

/// Every step is always present (skipped ones included) so the UI list is stable.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    fn new() -> Self {
        Self { checks: STEPS.iter().map(|&step| Check { step, status: Status::Skipped, detail: "Not tested.".into() }).collect() }
    }

    fn set(&mut self, step: Step, status: Status, detail: impl Into<String>) {
        if let Some(c) = self.checks.iter_mut().find(|c| c.step == step) {
            c.status = status;
            c.detail = detail.into();
        }
    }

    /// Nothing could be tested because the settings are incomplete.
    pub fn config_failed(detail: impl Into<String>) -> Self {
        let mut r = Self::new();
        r.set(Step::Config, Status::Failed, detail);
        r
    }

    /// A FlickSync answers and is ready to sign people in: enough to keep an
    /// address that was just added. Clock and key problems are reported, not blocking.
    pub fn reachable_and_ready(&self) -> bool {
        let status = |step| self.checks.iter().find(|c| c.step == step).map(|c| c.status);
        status(Step::Reach) == Some(Status::Ok) && status(Step::Ready) != Some(Status::Failed)
    }

    pub fn passed(&self) -> bool {
        self.checks.iter().all(|c| c.status != Status::Failed)
    }
}

/// Runs every step against `base`, signing in with `tokens`.
pub async fn run(http: &Client, base: &Url, tokens: &dyn TokenProvider) -> Report {
    let mut r = Report::new();
    r.set(Step::Config, Status::Ok, "Address and sign-in are set.");
    let base = http_base(base);

    // 1. Is there a FlickSync at this address?
    let resp = match http.get(join(&base, "health")).timeout(TIMEOUT).send().await {
        Ok(resp) => resp,
        Err(e) => {
            r.set(Step::Reach, Status::Failed, describe_transport(&e));
            return r;
        }
    };
    let status = resp.status();
    let server_time = resp.headers().get("date").and_then(|v| v.to_str().ok()).and_then(|v| httpdate::parse_http_date(v).ok());
    if !status.is_success() {
        let detail = if status == StatusCode::NOT_FOUND {
            "The address answers, but it isn't FlickSync (404). Check the port and the path; behind a reverse proxy, make sure it forwards to FlickSync.".to_owned()
        } else {
            format!("The address answers with an error (HTTP {}).", status.as_u16())
        };
        r.set(Step::Reach, Status::Failed, detail);
        return r;
    }
    match resp.json::<Health>().await {
        Ok(h) if h.status == "ok" => {
            let version = h.version.map(|v| format!("FlickSync {v} found.")).unwrap_or_else(|| "FlickSync found.".into());
            r.set(Step::Reach, Status::Ok, version);
        }
        _ => {
            r.set(Step::Reach, Status::Failed, "Something answers here, but it isn't FlickSync. Check the address and port.");
            return r;
        }
    }

    // 2. Can it sign people in?
    let mut ready = true;
    match http.get(join(&base, "ready")).timeout(TIMEOUT).send().await {
        Ok(resp) if resp.status().is_success() => r.set(Step::Ready, Status::Ok, "The server is ready."),
        Ok(resp) if resp.status() == StatusCode::SERVICE_UNAVAILABLE => {
            ready = false;
            r.set(Step::Ready, Status::Failed, "The server is running but not ready: it has no signing key configured (FLICKSYNC_AUTH_KEYS), or it is shutting down.");
        }
        Ok(resp) if resp.status() == StatusCode::NOT_FOUND => r.set(Step::Ready, Status::Skipped, "This server version doesn't report readiness."),
        Ok(resp) => r.set(Step::Ready, Status::Failed, format!("The readiness check answered HTTP {}.", resp.status().as_u16())),
        Err(e) => r.set(Step::Ready, Status::Failed, describe_transport(&e)),
    }

    // 3. Are the clocks close enough for tokens to be accepted?
    match server_time {
        Some(server) => {
            let skew = clock_skew_secs(SystemTime::now(), server);
            if skew.abs() > MAX_SKEW_SECS {
                let side = if skew > 0 { "ahead of" } else { "behind" };
                r.set(
                    Step::Clock,
                    Status::Failed,
                    format!("Your clock is {} s {side} the server's. Sign-in tokens will be refused; fix the date and time on your computer (or the server).", skew.abs()),
                );
            } else {
                r.set(Step::Clock, Status::Ok, "Clocks agree.");
            }
        }
        None => r.set(Step::Clock, Status::Skipped, "The server didn't report its time."),
    }

    // 4. Does the server accept our key?
    if !ready {
        r.set(Step::Auth, Status::Skipped, "Skipped: the server isn't ready.");
        return r;
    }
    let (status, detail) = probe_auth(http, &base, tokens).await;
    r.set(Step::Auth, status, detail);
    r
}

#[derive(Deserialize)]
struct Health {
    status: String,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    #[serde(default)]
    message: String,
}

async fn probe_auth(http: &Client, base: &Url, tokens: &dyn TokenProvider) -> (Status, String) {
    let fail = |s: &str| (Status::Failed, s.to_owned());
    let token = match tokens.token().await {
        Ok(t) => t,
        Err(Error::Unauthenticated) => return fail("The Flick Server refused to issue a sign-in token."),
        Err(Error::NotConfigured) => return fail("The signing key is missing or malformed. Expected kid:server_id:secret."),
        Err(Error::Network(_)) => return fail("The Flick Server's sign-in endpoint isn't reachable."),
        Err(_) => return fail("Couldn't get a sign-in token."),
    };
    let resp = match http.get(join(base, &format!("api/v1/rooms/{PROBE_ROOM}"))).bearer_auth(token).timeout(TIMEOUT).send().await {
        Ok(resp) => resp,
        Err(e) => return (Status::Failed, describe_transport(&e)),
    };
    let status = resp.status();
    if status == StatusCode::UNAUTHORIZED {
        let message = resp.json::<ErrorBody>().await.map(|b| b.error.message).unwrap_or_default();
        return (Status::Failed, explain_rejection(&message).to_owned());
    }
    if status == StatusCode::FORBIDDEN {
        return fail("The key is accepted but doesn't allow using rooms.");
    }
    if status.is_server_error() {
        return (Status::Failed, format!("The server failed while checking the key (HTTP {}).", status.as_u16()));
    }
    // 404 / 400: authenticated, then the made-up room was (rightly) not found.
    (Status::Ok, "The server accepts this key.".into())
}

/// What a 401 from FlickSync means for the person holding the key. The server
/// words these generically on purpose; each maps to one thing to check.
fn explain_rejection(message: &str) -> &'static str {
    match message {
        "malformed token" | "unsupported token algorithm" | "token has no key id" => "The signing key is malformed. Copy it again from the server.",
        "unknown signing key" => "The server doesn't know this key (its key id). If the key was changed, ask the server's administrator for a new invitation link and paste it again.",
        "token server_id does not match its signing key" => "The server id (the second part of the key) doesn't match this key id. Paste the invitation link again.",
        "invalid or expired token" => "The server refused the key: the secret is wrong (if it was changed, ask the administrator for a new invitation link), or the clocks are too far apart.",
        "token lifetime exceeds the allowed maximum" => "The server refused the token lifetime. Update Flick or the server.",
        _ => "The server refused the key.",
    }
}

fn describe_transport(e: &oneshot_net::reqwest::Error) -> String {
    let chain = error_chain(e);
    let has = |needles: &[&str]| needles.iter().any(|n| chain.contains(n));
    if e.is_timeout() {
        "No answer within 8 seconds. Check the address and port, and that no firewall blocks it."
    } else if has(&["certificate", "tls", "ssl", "handshake"]) {
        "The secure connection failed (certificate or HTTPS problem). If the server has no certificate, use http:// instead of https://."
    } else if has(&["dns", "lookup", "resolve", "not known", "no such host", "11001"]) {
        "Unknown host: the address doesn't resolve. Check it for a typo."
    } else if has(&["refused", "10061"]) {
        "Connection refused: nothing listens on this port. Check the port and that FlickSync is running."
    } else {
        "Couldn't connect to the server. Check the address, the port and your network."
    }
    .to_owned()
}

/// Lower-cased text of an error and all its causes, for matching only (never shown).
fn error_chain(e: &(dyn std::error::Error + 'static)) -> String {
    let mut out = e.to_string();
    let mut cur = e.source();
    while let Some(c) = cur {
        out.push_str(": ");
        out.push_str(&c.to_string());
        cur = c.source();
    }
    out.to_lowercase()
}

/// Seconds the local clock is ahead of the server's (negative: behind).
fn clock_skew_secs(local: SystemTime, server: SystemTime) -> i64 {
    let secs = |t: SystemTime| t.duration_since(UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    secs(local) - secs(server)
}

/// Settings may hold a `ws://` address; the probes are plain HTTP.
fn http_base(base: &Url) -> Url {
    let mut url = base.clone();
    let _ = match base.scheme() {
        "ws" => url.set_scheme("http"),
        "wss" => url.set_scheme("https"),
        _ => Ok(()),
    };
    url
}

fn join(base: &Url, path: &str) -> Url {
    base.join(path).unwrap_or_else(|_| base.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejections_point_at_the_part_of_the_key_to_check() {
        assert!(explain_rejection("unknown signing key").contains("new invitation link"));
        assert!(explain_rejection("token server_id does not match its signing key").contains("server id"));
        assert!(explain_rejection("invalid or expired token").contains("secret"));
        assert!(explain_rejection("something new").contains("refused"));
    }

    #[test]
    fn skew_is_signed_from_our_point_of_view() {
        let t = |s| UNIX_EPOCH + Duration::from_secs(s);
        assert_eq!(clock_skew_secs(t(1_000), t(940)), 60);
        assert_eq!(clock_skew_secs(t(940), t(1_000)), -60);
        assert_eq!(clock_skew_secs(t(1_000), t(1_000)), 0);
    }

    #[test]
    fn websocket_addresses_are_probed_over_http() {
        assert_eq!(http_base(&Url::parse("ws://h:8787/").unwrap()).scheme(), "http");
        assert_eq!(http_base(&Url::parse("wss://h/").unwrap()).scheme(), "https");
        assert_eq!(http_base(&Url::parse("https://h/").unwrap()).scheme(), "https");
    }

    #[test]
    fn an_incomplete_setup_reports_every_step() {
        let r = Report::config_failed("no address");
        assert_eq!(r.checks.len(), STEPS.len());
        assert!(!r.passed());
        assert_eq!(r.checks[1].status, Status::Skipped);
    }
}
