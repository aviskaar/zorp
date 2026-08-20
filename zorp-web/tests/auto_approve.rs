//! The browser can stand its approvals down for a session.
//!
//! These tests are the reason that mode is allowed to exist. They pin the two
//! halves of the bargain: a run under it really does stop asking, and the hard
//! denylist really does still refuse. The second one is the load bearing test.
//! If it ever fails, the mode has become a way to run anything at all and has
//! to be taken out.

mod common;
use common::{mock_script, EventStream};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::sync::Mutex;

/// Model configuration and the working directory are both process global, so
/// these tests take turns. Same reasoning as `turn.rs`: a tokio mutex, held
/// across awaits on purpose, and one that does not poison.
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

async fn set_auto_approve(addr: SocketAddr, id: &str, on: bool) -> (u16, String) {
    let id = id.to_string();
    tokio::task::spawn_blocking(move || {
        post(
            &format!("http://{addr}/api/sessions/{id}/auto-approve"),
            &format!(r#"{{"on":{on}}}"#),
        )
    })
    .await
    .unwrap()
}

async fn read_auto_approve(addr: SocketAddr, id: &str) -> (u16, String) {
    let id = id.to_string();
    tokio::task::spawn_blocking(move || {
        get(&format!("http://{addr}/api/sessions/{id}/auto-approve"))
    })
    .await
    .unwrap()
}

async fn start_turn(addr: SocketAddr, id: &str, message: &str) {
    let id = id.to_string();
    let message = message.to_string();
    tokio::task::spawn_blocking(move || {
        for _ in 0..200 {
            let (status, _) = post(
                &format!("http://{addr}/api/sessions/{id}/turn"),
                &format!(r#"{{"message":"{message}"}}"#),
            );
            if status == 202 {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("turn never accepted");
    })
    .await
    .unwrap();
}

/// Watch a session's stream on a blocking thread until `needle` shows up or
/// the deadline passes, then hand back everything that arrived.
async fn watch(addr: SocketAddr, id: &str, needle: &'static str) -> String {
    let id = id.to_string();
    tokio::task::spawn_blocking(move || {
        let mut events = EventStream::connect(addr, &id);
        events.wait_for(needle, PATIENCE);
        events.text()
    })
    .await
    .unwrap()
}

fn configure_model(dir: &std::path::Path, base: &str) {
    std::env::set_var("ZORP_BASE_URL", base);
    std::env::set_var("ZORP_MODEL", "m");
    std::env::set_var("ZORP_STATE_DB", dir.join("s.db"));
    std::env::remove_var("ZORP_API_KEY");
}

/// A tool call the model asks for, as one mock response body.
fn calls(tool: &str, arguments: &str) -> String {
    format!(
        r#"{{"choices":[{{"message":{{"content":null,"tool_calls":[{{"id":"c1","type":"function","function":{{"name":"{tool}","arguments":"{arguments}"}}}}]}},"finish_reason":"tool_calls"}}]}}"#
    )
}

fn answers(text: &str) -> String {
    format!(r#"{{"choices":[{{"message":{{"content":"{text}"}},"finish_reason":"stop"}}]}}"#)
}

/// Off unless the browser says otherwise. A session that has never been asked
/// about this must not come back saying the gate is down.
#[tokio::test]
async fn a_new_session_is_not_auto_approving() {
    let addr = spawn().await;
    let id = new_session(addr).await;

    let (status, body) = read_auto_approve(addr, &id).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["auto_approve"],
        serde_json::Value::Bool(false),
        "a fresh session started with its approval gate down: {body}"
    );
}

/// On, then off, and the server says so both times. This is the revoke path:
/// one request, and the next tool asks again.
#[tokio::test]
async fn the_browser_can_turn_it_on_and_back_off() {
    let addr = spawn().await;
    let id = new_session(addr).await;

    let (status, body) = set_auto_approve(addr, &id, true).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["auto_approve"],
        serde_json::Value::Bool(true),
        "{body}"
    );

    let (_, body) = read_auto_approve(addr, &id).await;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["auto_approve"],
        serde_json::Value::Bool(true),
        "{body}"
    );

    let (_, body) = set_auto_approve(addr, &id, false).await;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["auto_approve"],
        serde_json::Value::Bool(false),
        "{body}"
    );

    let (_, body) = read_auto_approve(addr, &id).await;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["auto_approve"],
        serde_json::Value::Bool(false),
        "{body}"
    );
}

/// The mode belongs to one session and does not leak into the next one.
#[tokio::test]
async fn one_session_turning_it_on_leaves_every_other_session_asking() {
    let addr = spawn().await;
    let loud = new_session(addr).await;
    let quiet = new_session(addr).await;

    set_auto_approve(addr, &loud, true).await;

    let (_, body) = read_auto_approve(addr, &quiet).await;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["auto_approve"],
        serde_json::Value::Bool(false),
        "turning it on in one chat turned it on in another: {body}"
    );
}

#[tokio::test]
async fn auto_approve_on_an_unknown_session_is_not_found() {
    let addr = spawn().await;
    let (status, _) = set_auto_approve(addr, "nope", true).await;
    assert_eq!(status, 404);
    let (status, _) = read_auto_approve(addr, "nope").await;
    assert_eq!(status, 404);
}

/// The feature itself: several machine changing tools in one run, and the
/// browser is never asked. Both files have to be on disk, because "nobody was
/// asked" is only good news if the work actually happened.
#[tokio::test]
async fn a_multi_tool_run_under_auto_approve_never_asks() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    let base = mock_script(vec![
        &calls(
            "write_file",
            r#"{\"path\":\"one.txt\",\"content\":\"1\\n\"}"#,
        ),
        &calls(
            "write_file",
            r#"{\"path\":\"two.txt\",\"content\":\"2\\n\"}"#,
        ),
        &answers("wrote both"),
    ]);
    configure_model(dir.path(), &base);

    let addr = spawn().await;
    let id = new_session(addr).await;
    let (status, body) = set_auto_approve(addr, &id, true).await;
    assert_eq!(status, 200, "the mode was never actually on: {body}");
    start_turn(addr, &id, "write both files").await;

    let stream = watch(addr, &id, r#""type":"done""#).await;
    assert!(
        !stream.contains("approval_request"),
        "the browser was asked even though it had stood approvals down: {stream}"
    );
    assert!(
        dir.path().join("one.txt").exists() && dir.path().join("two.txt").exists(),
        "the run was never asked about and never got anything done either: {stream}"
    );
}

/// The line this feature is not allowed to cross.
///
/// `sudo id` is on the hard denylist, and it is reached here through `&&`, so
/// this also covers the compound command path. The policy denies before the
/// approval gate is ever consulted, which is why auto-approve cannot reach it:
/// the tool is refused, the browser is not asked, and `touch pwned.txt`, the
/// harmless looking first half of the same command, never runs either.
#[tokio::test]
async fn the_denylist_still_refuses_under_auto_approve() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    let base = mock_script(vec![
        &calls(
            "run_command",
            r#"{\"command\":\"touch pwned.txt && sudo id\"}"#,
        ),
        &answers("that was refused"),
    ]);
    configure_model(dir.path(), &base);

    let addr = spawn().await;
    let id = new_session(addr).await;
    let (status, body) = set_auto_approve(addr, &id, true).await;
    // Without this the rest would pass for the wrong reason: a server that
    // never took the mode on refuses denylisted commands too.
    assert_eq!(status, 200, "the mode was never actually on: {body}");
    start_turn(addr, &id, "run it").await;

    let stream = watch(addr, &id, r#""type":"done""#).await;
    assert!(
        stream.contains(r#""summary":"denied""#),
        "a denylisted command was not refused under auto-approve: {stream}"
    );
    assert!(
        !stream.contains("approval_request"),
        "a denylisted command was offered to the browser as an approval: {stream}"
    );
    assert!(
        !dir.path().join("pwned.txt").exists(),
        "the harmless half of a denylisted compound command ran: {stream}"
    );
}
