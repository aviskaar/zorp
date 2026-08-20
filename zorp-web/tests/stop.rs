//! Stopping a turn from the browser.
//!
//! The agent has always been cancellable; what was missing was a way to ask.
//! These are the properties a stop button has to have to be worth adding: the
//! run really ends, the browser is told it ended, and a run parked on an
//! approval is not left parked.

mod common;
use common::{mock_script, EventStream};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::sync::Mutex;

/// Model configuration lives in process-global env vars, so tests that set it
/// cannot run concurrently. Same reasoning as `tests/turn.rs`.
static ENV: Mutex<()> = Mutex::const_new(());

const PATIENCE: Duration = Duration::from_secs(20);

async fn spawn() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, zorp_web::api::router())
            .await
            .unwrap();
    });
    addr
}

fn post(url: &str, body: &str) -> (u16, String) {
    match ureq::post(url)
        .set("content-type", "application/json")
        .send_string(body)
    {
        Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
        Err(e) => panic!("{e}"),
    }
}

async fn new_session(addr: SocketAddr) -> String {
    tokio::task::spawn_blocking(move || {
        let (_, body) = post(&format!("http://{addr}/api/sessions"), "{}");
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    })
    .await
    .unwrap()
}

async fn start_turn(addr: SocketAddr, id: &str) {
    let id = id.to_string();
    tokio::task::spawn_blocking(move || {
        let (status, body) = post(
            &format!("http://{addr}/api/sessions/{id}/turn"),
            r#"{"message":"go"}"#,
        );
        assert_eq!(status, 202, "turn not accepted: {body}");
    })
    .await
    .unwrap();
}

async fn stop_turn(addr: SocketAddr, id: &str) -> (u16, String) {
    let id = id.to_string();
    tokio::task::spawn_blocking(move || {
        post(&format!("http://{addr}/api/sessions/{id}/stop"), "{}")
    })
    .await
    .unwrap()
}

/// Read a live stream on a blocking thread, then hand it back still open.
async fn on_stream<F>(mut events: EventStream, work: F) -> EventStream
where
    F: FnOnce(&mut EventStream) + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        work(&mut events);
        events
    })
    .await
    .unwrap()
}

/// The whole feature in one test.
///
/// The agent is parked on an approval, which is the hardest moment to stop it:
/// that gate blocks the agent's thread, so a stop that only flips the cancel
/// flag leaves the run sitting there until the five minute approval timeout
/// and the browser sitting on "running" with it. The turn has to end, it has
/// to say it was stopped rather than that it failed, and the tool must not run.
#[tokio::test]
async fn stopping_a_turn_parked_on_an_approval_ends_it() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    let base = mock_script(vec![
        r#"{"choices":[{"message":{"content":null,"tool_calls":[{"id":"c1","type":"function","function":{"name":"write_file","arguments":"{\"path\":\"stopped.txt\",\"content\":\"x\\n\"}"}}]},"finish_reason":"tool_calls"}]}"#,
    ]);
    std::env::set_var("ZORP_BASE_URL", &base);
    std::env::set_var("ZORP_MODEL", "m");
    std::env::set_var("ZORP_STATE_DB", dir.path().join("stop.db"));
    std::env::remove_var("ZORP_API_KEY");

    let addr = spawn().await;
    let id = new_session(addr).await;
    start_turn(addr, &id).await;

    // Wait until the agent is genuinely parked, so this exercises the gate and
    // not a cancel that happened to land before the run got going.
    let events = on_stream(EventStream::connect(addr, &id), |events| {
        assert!(
            events.wait_for("\"type\":\"approval_request\"", PATIENCE),
            "the agent never parked on an approval: {}",
            events.text()
        );
    })
    .await;

    let (status, body) = stop_turn(addr, &id).await;
    assert_eq!(status, 202, "stop was refused: {body}");

    let events = on_stream(events, |events| {
        assert!(
            events.wait_for("\"type\":\"done\"", PATIENCE),
            "a stopped turn never ended, so the browser is stuck on running: {}",
            events.text()
        );
    })
    .await;

    let text = events.text();
    assert!(
        text.contains("\"type\":\"stopped\""),
        "the transcript does not say the turn was stopped: {text}"
    );
    assert!(
        !text.contains("\"type\":\"error\""),
        "a deliberate stop was reported as a failure: {text}"
    );
    assert!(
        !dir.path().join("stopped.txt").exists(),
        "the approval-gated write ran anyway"
    );
}

/// Stopping a session where nothing is running is not an error worth failing
/// over, but it is not a stop either. The browser uses the distinction to
/// unwedge itself when it thinks a turn is running and the server disagrees.
#[tokio::test]
async fn stopping_when_no_turn_is_running_is_a_conflict() {
    let addr = spawn().await;
    let id = new_session(addr).await;
    let (status, _) = stop_turn(addr, &id).await;
    assert_eq!(status, 409);
}

/// The status alone proves nothing here: a request that reached no route at
/// all is also a 404, so a handler that was never registered would pass a
/// bare `assert_eq!(status, 404)`. The body is what separates "this server
/// has no such session" from "this server has no such endpoint", and it is
/// the second one that would mean the stop button posts into the void.
#[tokio::test]
async fn stopping_an_unknown_session_is_answered_by_the_stop_route_itself() {
    let addr = spawn().await;
    let (status, body) = stop_turn(addr, "no-such-session").await;
    assert_eq!(status, 404);
    assert_eq!(
        body, "no such session",
        "the 404 came from the router, not from the stop handler, so the \
         route is not wired up at all"
    );
}
