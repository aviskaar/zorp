//! Running a review panel from the browser.
//!
//! The properties a panel has to have to be worth putting behind a
//! button: every reviewer is heard from, a reviewer that fell over is
//! visible rather than silently absent, the panel occupies the session
//! so it cannot interleave with a turn, and the existing stop control
//! reaches it.

mod common;
use common::{mock_script, EventStream};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::sync::Mutex;

/// Model configuration lives in process-global env vars, so tests that
/// set it cannot run concurrently. Same reasoning as `tests/turn.rs`.
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

async fn start_panel(addr: SocketAddr, id: &str, body: &str) -> (u16, String) {
    let id = id.to_string();
    let body = body.to_string();
    tokio::task::spawn_blocking(move || {
        post(&format!("http://{addr}/api/sessions/{id}/panel"), &body)
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

/// One model response carrying a well formed verdict.
fn verdict_response(locus: &str) -> String {
    let answer = format!(
        "Looked at it.\\n\\n```json\\n{{\\\"findings\\\": [{{\\\"severity\\\": \
         \\\"concern\\\", \\\"claim\\\": \\\"0.91 is not in the record\\\", \
         \\\"locus\\\": \\\"{locus}\\\"}}]}}\\n```"
    );
    format!(r#"{{"choices":[{{"message":{{"content":"{answer}"}},"finish_reason":"stop"}}]}}"#)
}

fn configure(dir: &std::path::Path, responses: Vec<&str>) {
    let base = mock_script(responses);
    std::env::set_var("ZORP_BASE_URL", &base);
    std::env::set_var("ZORP_MODEL", "m");
    std::env::set_var("ZORP_STATE_DB", dir.path_db());
    std::env::remove_var("ZORP_API_KEY");
}

/// A model endpoint that accepts a connection and never answers.
///
/// `mock_script(vec![])` will not do: with nothing queued its accept
/// loop ends, the listener drops, and the next connection is refused,
/// so the turn fails in milliseconds and the session is idle again
/// before the panel request lands. The test needs a turn that is
/// genuinely still running.
fn mock_hang() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        // Held for the life of the test process. Every accepted socket is
        // parked in the vector so nothing is closed, which is what keeps
        // the client waiting.
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

/// Small helper so `configure` reads cleanly.
trait DbPath {
    fn path_db(&self) -> std::path::PathBuf;
}

impl DbPath for std::path::Path {
    fn path_db(&self) -> std::path::PathBuf {
        self.join("panel.db")
    }
}

/// The feature end to end: two reviewers asked, two heard from, and the
/// closing frame says the panel was complete.
#[tokio::test]
async fn a_panel_reports_every_reviewer_and_its_own_completeness() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    let a = verdict_response("section 3");
    let b = verdict_response("section 3");
    configure(dir.path(), vec![&a, &b]);

    let addr = spawn().await;
    let id = new_session(addr).await;
    let events = EventStream::connect(addr, &id);

    let (status, body) = start_panel(
        addr,
        &id,
        r#"{"label":"draft.md","body":"The accuracy was 0.91.","lenses":["evidence","method"]}"#,
    )
    .await;
    assert_eq!(status, 202, "panel not accepted: {body}");

    let events = on_stream(events, |s| {
        assert!(
            s.wait_for("\"type\":\"panel_done\"", PATIENCE),
            "the panel never closed: {}",
            s.text()
        )
    })
    .await;
    let text = events.text();

    assert!(text.contains("\"type\":\"reviewer_started\""), "{text}");
    assert!(text.contains("\"lens\":\"evidence\""), "{text}");
    assert!(text.contains("\"lens\":\"method\""), "{text}");
    assert!(text.contains("\"type\":\"reviewer_finished\""), "{text}");
    assert!(text.contains("\"complete\":true"), "{text}");
    assert!(text.contains("\"lenses_requested\":2"), "{text}");
    // Both reviewers named the same locus, so the panel found an
    // agreement without either of them being told what the other said.
    assert!(text.contains("\"agreements\""), "{text}");
    assert!(text.contains("\"section 3\""), "{text}");
    // A panel ends the way a turn does, so the composer re-enables.
    assert!(text.contains("\"type\":\"done\""), "{text}");
}

/// A reviewer that answers in prose is reported as a failure and the
/// panel says it was not complete. The alternative is a panel of two
/// that quietly reports as a panel of one.
#[tokio::test]
async fn a_reviewer_that_fails_is_visible_and_makes_the_panel_incomplete() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    let good = verdict_response("section 3");
    let prose =
        r#"{"choices":[{"message":{"content":"Looks fine to me."},"finish_reason":"stop"}]}"#;
    configure(dir.path(), vec![&good, prose]);

    let addr = spawn().await;
    let id = new_session(addr).await;
    let events = EventStream::connect(addr, &id);

    let (status, _) = start_panel(
        addr,
        &id,
        r#"{"label":"draft.md","body":"The accuracy was 0.91.","lenses":["evidence","method"]}"#,
    )
    .await;
    assert_eq!(status, 202);

    let events = on_stream(events, |s| {
        assert!(
            s.wait_for("\"type\":\"panel_done\"", PATIENCE),
            "the panel never closed: {}",
            s.text()
        )
    })
    .await;
    let text = events.text();

    assert!(text.contains("\"type\":\"reviewer_failed\""), "{text}");
    assert!(text.contains("fenced"), "{text}");
    assert!(text.contains("\"complete\":false"), "{text}");
    assert!(text.contains("\"verdicts\":1"), "{text}");
    assert!(text.contains("\"lenses_requested\":2"), "{text}");
}

/// Five agents asked to review nothing produce five confident answers
/// about nothing, at the cost of five requests, and it reads exactly
/// like a real panel.
#[tokio::test]
async fn an_empty_target_is_refused_before_any_reviewer_runs() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    configure(dir.path(), vec![]);

    let addr = spawn().await;
    let id = new_session(addr).await;
    let (status, body) = start_panel(addr, &id, r#"{"label":"draft.md","body":"   \n  "}"#).await;
    assert_eq!(status, 400, "{body}");
    assert!(body.contains("nothing to review"), "{body}");
}

/// A panel and a turn under one sequence counter would put two
/// conversations in one transcript.
#[tokio::test]
async fn a_panel_is_refused_while_a_turn_is_running() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    // A response that never arrives, so the turn stays running.
    std::env::set_var("ZORP_BASE_URL", mock_hang());
    std::env::set_var("ZORP_MODEL", "m");
    std::env::set_var("ZORP_STATE_DB", dir.path().join("panel.db"));
    std::env::remove_var("ZORP_API_KEY");

    let addr = spawn().await;
    let id = new_session(addr).await;
    let turn_id = id.clone();
    tokio::task::spawn_blocking(move || {
        post(
            &format!("http://{addr}/api/sessions/{turn_id}/turn"),
            r#"{"message":"go"}"#,
        )
    })
    .await
    .unwrap();

    // Give the turn a moment to mark the session running.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (status, body) = start_panel(addr, &id, r#"{"label":"draft.md","body":"something"}"#).await;
    assert_eq!(status, 409, "{body}");
}

#[tokio::test]
async fn the_lens_list_is_readable_without_running_a_panel() {
    let addr = spawn().await;
    let (status, body) =
        tokio::task::spawn_blocking(move || get(&format!("http://{addr}/api/panel/lenses")))
            .await
            .unwrap();
    assert_eq!(status, 200);
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    let lenses = parsed["lenses"].as_array().unwrap();
    assert!(lenses.len() >= 3, "a panel of two is not a panel: {body}");
    let names: Vec<&str> = lenses.iter().map(|l| l["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"evidence"), "{names:?}");
    // Every lens carries its instruction, so the page can show what a
    // reviewer will actually be told rather than only its name.
    assert!(lenses
        .iter()
        .all(|l| l["instruction"].as_str().is_some_and(|s| !s.is_empty())));
}
