//! Zorp mode from the browser: one pre-registered `investigate`
//! attempt, and a read of what it left in the aryabhatta ledger.
//!
//! The properties worth putting behind a button: the attempt occupies
//! the session so it cannot interleave with a turn, a request that
//! cannot run is refused before anything is recorded, the ledger reads
//! back the conditions the attempt ran under, and forecasting is off
//! unless the server was told otherwise.

#![cfg(feature = "research")]

mod common;
use common::{mock_script, EventStream};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::sync::Mutex;

/// Model configuration and the process working directory are both
/// global, so these tests cannot run concurrently. Same reasoning as
/// `tests/panel.rs`.
static ENV: Mutex<()> = Mutex::const_new(());

const PATIENCE: Duration = Duration::from_secs(30);

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

fn get(url: &str) -> (u16, String) {
    match ureq::get(url).call() {
        Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
        Err(e) => panic!("{e}"),
    }
}

async fn blocking_get(url: String) -> (u16, String) {
    tokio::task::spawn_blocking(move || get(&url))
        .await
        .unwrap()
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

async fn start_investigate(addr: SocketAddr, id: &str, body: &str) -> (u16, String) {
    let id = id.to_string();
    let body = body.to_string();
    tokio::task::spawn_blocking(move || {
        post(
            &format!("http://{addr}/api/sessions/{id}/investigate"),
            &body,
        )
    })
    .await
    .unwrap()
}

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

/// One model answer in the shape `investigate` parses.
fn attempt_response(metric_value: f64) -> String {
    let answer = format!(
        "Ran it.\\n\\n```json\\n{{\\\"metric_value\\\": {metric_value}, \
         \\\"summary\\\": \\\"one attempt\\\"}}\\n```"
    );
    format!(r#"{{"choices":[{{"message":{{"content":"{answer}"}},"finish_reason":"stop"}}]}}"#)
}

fn configure(dir: &std::path::Path, responses: Vec<&str>) {
    let base = mock_script(responses);
    std::env::set_var("ZORP_BASE_URL", &base);
    std::env::set_var("ZORP_MODEL", "m");
    std::env::set_var("ZORP_STATE_DB", dir.join("investigate.db"));
    std::env::remove_var("ZORP_API_KEY");
    // Off unless a test says otherwise, which is also the product
    // default. A forecast costs an extra model call, so a test that
    // forgot this would hang waiting for a response nobody queued.
    std::env::remove_var("ZORP_FORECAST");
}

/// A model endpoint that accepts a connection and never answers, so a
/// turn stays genuinely running while the test does something else.
fn mock_hang() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming() {
            match stream {
                Ok(s) => held.push(s),
                Err(_) => break,
            }
        }
    });
    format!("http://{addr}")
}

/// The feature end to end. One attempt runs, the closing frame says the
/// track survived it, and the ledger reads back what the attempt ran
/// under.
#[tokio::test]
async fn an_attempt_runs_and_lands_in_the_ledger() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    let answer = attempt_response(42.0);
    configure(dir.path(), vec![&answer]);

    let addr = spawn().await;
    let id = new_session(addr).await;
    let events = EventStream::connect(addr, &id);

    let (status, body) = start_investigate(
        addr,
        &id,
        r#"{"question":"does caching help","metric_name":"latency_ms",
            "kill_threshold":100.0,"threshold_direction":"lower-is-better"}"#,
    )
    .await;
    assert_eq!(status, 202, "the attempt was not accepted: {body}");

    let events = on_stream(events, |s| {
        assert!(
            s.wait_for("\"type\":\"investigate_done\"", PATIENCE),
            "the attempt never closed: {}",
            s.text()
        )
    })
    .await;
    let text = events.text();
    assert!(text.contains("\"approved\":true"), "{text}");
    // An attempt ends the way a turn does, so the composer re-enables.
    assert!(text.contains("\"type\":\"done\""), "{text}");

    let question = urlencoding("does caching help");
    let (status, body) = blocking_get(format!(
        "http://{addr}/api/investigate/ledger?question={question}"
    ))
    .await;
    assert_eq!(status, 200, "{body}");
    let ledger: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(ledger["present"], true, "{body}");
    let experiments = ledger["experiments"].as_array().unwrap();
    assert_eq!(experiments.len(), 1, "{body}");
    let run = &experiments[0];
    assert_eq!(run["status"], "completed", "{body}");

    // The conditions the attempt ran under. This is the thing zorp did
    // not record at all before aryabhatta: outputs were recorded and
    // inputs were not.
    let conditions = run["conditions"].as_array().unwrap();
    let keys: Vec<&str> = conditions
        .iter()
        .map(|c| c["key"].as_str().unwrap())
        .collect();
    assert!(keys.contains(&"checkpoint_mode"), "{body}");
    // A browser has no terminal, so the checkpoint mode is auto-approve
    // and the record says so rather than leaving it to be guessed.
    let mode = conditions
        .iter()
        .find(|c| c["key"] == "checkpoint_mode")
        .unwrap();
    assert_eq!(mode["value"], "auto-approve", "{body}");

    let metrics = run["metrics"].as_array().unwrap();
    assert_eq!(metrics.len(), 1, "{body}");
    assert_eq!(metrics[0]["key"], "latency_ms", "{body}");
    assert_eq!(metrics[0]["value"], "42", "{body}");

    // Nobody asked for a forecast, so nothing was scored, and the
    // ledger says the record is empty rather than pretending otherwise.
    assert!(run["expectations"].as_array().unwrap().is_empty(), "{body}");
    assert_eq!(ledger["forecasting"], false, "{body}");
}

/// An attempt occupies the session exactly as a turn does. Two of them
/// under one sequence counter would interleave into one unreadable
/// transcript.
#[tokio::test]
async fn an_attempt_is_refused_while_a_turn_is_running() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    std::env::set_var("ZORP_BASE_URL", mock_hang());
    std::env::set_var("ZORP_MODEL", "m");
    std::env::set_var("ZORP_STATE_DB", dir.path().join("busy.db"));
    std::env::remove_var("ZORP_API_KEY");
    std::env::remove_var("ZORP_FORECAST");

    let addr = spawn().await;
    let id = new_session(addr).await;
    let turn_id = id.clone();
    let (status, _) = tokio::task::spawn_blocking(move || {
        post(
            &format!("http://{addr}/api/sessions/{turn_id}/turn"),
            r#"{"message":"hello"}"#,
        )
    })
    .await
    .unwrap();
    assert_eq!(status, 202);

    // The turn is parked on a model that never answers, so the session
    // stays occupied for as long as this test needs it.
    let (status, body) = start_investigate(
        addr,
        &id,
        r#"{"question":"does caching help","metric_name":"latency_ms",
            "kill_threshold":100.0,"threshold_direction":"lower-is-better"}"#,
    )
    .await;
    assert_eq!(status, 409, "{body}");
}

/// Refused before the session is occupied and before anything is
/// recorded, so a mistyped request costs nothing.
#[tokio::test]
async fn a_request_that_cannot_run_is_refused_without_recording_anything() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    let answer = attempt_response(1.0);
    configure(dir.path(), vec![&answer]);

    let addr = spawn().await;
    let id = new_session(addr).await;

    let (status, body) = start_investigate(addr, &id, r#"{"question":"   "}"#).await;
    assert_eq!(status, 400, "{body}");

    // Half a pre-registration is not a pre-registration. Recording one
    // with a guessed direction could kill a healthy track or spare a
    // doomed one.
    let (status, body) = start_investigate(
        addr,
        &id,
        r#"{"question":"does caching help","metric_name":"latency_ms"}"#,
    )
    .await;
    assert_eq!(status, 400, "{body}");

    // NaN would be written into the pre-registration and never compare
    // equal to itself again, locking the track out of every later run.
    let (status, body) = start_investigate(
        addr,
        &id,
        r#"{"question":"does caching help","metric_name":"latency_ms",
            "kill_threshold":null,"threshold_direction":"sideways"}"#,
    )
    .await;
    assert_eq!(status, 400, "{body}");

    assert!(
        !dir.path().join(".zorp").exists(),
        "a refused request created a run record"
    );
}

#[tokio::test]
async fn an_unknown_session_cannot_be_investigated() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    let answer = attempt_response(1.0);
    configure(dir.path(), vec![&answer]);

    let addr = spawn().await;
    let (status, body) = start_investigate(
        addr,
        "no-such-session",
        r#"{"question":"does caching help"}"#,
    )
    .await;
    assert_eq!(status, 404, "{body}");
}

/// Forecasting costs a model call on every attempt and is off unless
/// the person running the server turned it on. The page is told, and
/// there is no request that turns it on from the browser.
#[tokio::test]
async fn the_status_endpoint_reports_forecasting_and_never_sets_it() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    std::env::remove_var("ZORP_FORECAST");

    let addr = spawn().await;
    let (status, body) = blocking_get(format!("http://{addr}/api/investigate/status")).await;
    assert_eq!(status, 200, "{body}");
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["available"], true, "{body}");
    assert_eq!(value["forecasting"], false, "{body}");

    // There is no POST here. A browser control that flipped it would be
    // one page changing what the whole server does for everyone.
    let (status, _) = tokio::task::spawn_blocking(move || {
        post(&format!("http://{addr}/api/investigate/status"), "{}")
    })
    .await
    .unwrap();
    assert_eq!(status, 405, "the status endpoint accepted a write");
}

/// Reading a ledger must never bring a run record into existence.
/// Opening the view on a fresh checkout would otherwise write to it.
#[tokio::test]
async fn reading_a_ledger_creates_nothing() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();

    let addr = spawn().await;
    let question = urlencoding("a question nobody has run");
    let (status, body) = blocking_get(format!(
        "http://{addr}/api/investigate/ledger?question={question}"
    ))
    .await;
    assert_eq!(status, 200, "{body}");
    let ledger: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(ledger["present"], false, "{body}");
    assert!(
        ledger["experiments"].as_array().unwrap().is_empty(),
        "{body}"
    );
    assert!(
        !dir.path().join(".zorp").exists(),
        "a read created a run record"
    );

    let (status, body) = blocking_get(format!("http://{addr}/api/investigate/ledger")).await;
    assert_eq!(
        status, 400,
        "a ledger read with no question was answered: {body}"
    );
}

/// Percent-encode a question for the query string. Small enough that a
/// dependency for it would cost more than it saves.
fn urlencoding(s: &str) -> String {
    let mut out = String::new();
    for byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
