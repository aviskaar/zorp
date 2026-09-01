//! What `GET /api/sessions/:id` hands the browser to rebuild a transcript.
//!
//! The endpoint's contract is a role and the text of each turn. A turn where
//! the model only called a tool has no text, and the browser has nothing to
//! draw for it: tool activity reaches the page as its own event kind, not as
//! a message. So a content-free row is not a message the transcript is
//! missing detail for, it is not a message at all.

mod common;
use common::{mock_script, EventStream};
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;
use tokio::sync::Mutex;
use zorp_agent::{Message, Store, ToolCall};

/// `ZORP_STATE_DB` is process wide, so tests that set it take turns.
static ENV: Mutex<()> = Mutex::const_new(());

const PATIENCE: Duration = Duration::from_secs(20);

/// Port 0 so parallel test runs never collide on a fixed port.
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

async fn get_json(url: String) -> serde_json::Value {
    let body =
        tokio::task::spawn_blocking(move || ureq::get(&url).call().unwrap().into_string().unwrap())
            .await
            .unwrap();
    serde_json::from_str(&body).unwrap()
}

/// Status only. These calls are about whether the server will take the
/// request at all, not about what comes back.
async fn post_status(url: String, body: &'static str) -> u16 {
    tokio::task::spawn_blocking(move || {
        match ureq::post(&url)
            .set("content-type", "application/json")
            .send_string(body)
        {
            Ok(response) => response.status(),
            Err(ureq::Error::Status(code, _)) => code,
            Err(e) => panic!("{e}"),
        }
    })
    .await
    .unwrap()
}

async fn get_status(url: String) -> u16 {
    tokio::task::spawn_blocking(move || match ureq::get(&url).call() {
        Ok(response) => response.status(),
        Err(ureq::Error::Status(code, _)) => code,
        Err(e) => panic!("{e}"),
    })
    .await
    .unwrap()
}

async fn delete_status(url: String) -> u16 {
    tokio::task::spawn_blocking(move || match ureq::delete(&url).call() {
        Ok(response) => response.status(),
        Err(ureq::Error::Status(code, _)) => code,
        Err(e) => panic!("{e}"),
    })
    .await
    .unwrap()
}

/// A session shaped like the one a real tool-using turn leaves behind: the
/// user's request, an assistant turn carrying only a tool call, the tool's
/// result, and then the answer.
fn seed(db: &Path) -> String {
    let mut store = Store::open_at(db).unwrap();
    let id = "replay-session".to_string();
    store
        .create_session(&id, "write hello.txt", "repo", "model")
        .unwrap();
    store
        .record_message(&id, 0, &Message::system("you are zorp"))
        .unwrap();
    store
        .record_message(&id, 1, &Message::user("write hello.txt"))
        .unwrap();
    store
        .record_message(
            &id,
            2,
            &Message::assistant_with_calls(
                "",
                vec![ToolCall {
                    id: "c1".into(),
                    name: "write_file".into(),
                    arguments: serde_json::json!({"path": "hello.txt"}),
                }],
            ),
        )
        .unwrap();
    store
        .record_message(&id, 3, &Message::tool_result("c1", "wrote hello.txt"))
        .unwrap();
    store
        .record_message(&id, 4, &Message::assistant("Done."))
        .unwrap();
    id
}

#[tokio::test]
async fn a_tool_only_turn_is_left_out_of_the_replay() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("sessions.db");
    std::env::set_var("ZORP_STATE_DB", &db);
    let id = seed(&db);

    let addr = spawn().await;
    let body = get_json(format!("http://{addr}/api/sessions/{id}")).await;
    let messages = body["messages"].as_array().unwrap();

    assert!(
        !messages.iter().any(|m| m["content"].as_str() == Some("")),
        "an empty message draws a labelled bubble with nothing under it: {body}"
    );
}

/// The other half of the same rule: dropping the content-free rows must not
/// turn into dropping the conversation. A filter that returns nothing passes
/// the test above and fails this one.
#[tokio::test]
async fn the_replay_still_carries_both_sides_of_the_conversation() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("sessions.db");
    std::env::set_var("ZORP_STATE_DB", &db);
    let id = seed(&db);

    let addr = spawn().await;
    let body = get_json(format!("http://{addr}/api/sessions/{id}")).await;
    let messages = body["messages"].as_array().unwrap();

    assert_eq!(messages.len(), 2, "expected the ask and the answer: {body}");
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "write hello.txt");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"], "Done.");
}

/// What the sidebar shows, and what it shows when nothing named a session.
///
/// A generated title lives in `sessions.display_title` and is the only
/// thing that reads it. `sessions.task` still holds the verbatim first
/// message, and a session nobody named still shows it, which is exactly
/// how the sidebar read before titles existed. That fallback is a
/// requirement and not a convenience: a titling call can fail, decline, or
/// never be made, and none of those may leave a blank row.
#[tokio::test]
async fn the_sidebar_shows_a_generated_title_and_falls_back_to_the_first_message() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("sessions.db");
    std::env::set_var("ZORP_STATE_DB", &db);
    let named = seed(&db);
    let store = Store::open_at(&db).unwrap();
    store
        .create_session(
            "unnamed",
            "how many .rs files are in the root",
            "repo",
            "model",
        )
        .unwrap();
    store
        .set_display_title(&named, "Writing hello.txt")
        .unwrap();
    drop(store);

    let addr = spawn().await;
    let listed = get_json(format!("http://{addr}/api/sessions")).await;
    let rows = listed.as_array().unwrap();
    let row = |id: &str| {
        rows.iter()
            .find(|s| s["id"] == id)
            .unwrap_or_else(|| panic!("{id} is not listed: {listed}"))
            .clone()
    };

    assert_eq!(row(&named)["title"], "Writing hello.txt");
    assert_eq!(
        row("unnamed")["title"],
        "how many .rs files are in the root"
    );
}

/// Reopening a session from the sidebar after a restart and carrying on.
///
/// Live session state is held in a process-local map. The store outlives the
/// process and the map does not, so after a restart a session read straight
/// out of the sidebar had no entry, and both the turn endpoint and the event
/// stream answered "no such session". The transcript rendered fine and the
/// composer was dead, which reads as the server having lost the conversation
/// it is visibly showing you. A session the store knows about is adopted on
/// first use instead.
#[tokio::test]
async fn a_stored_session_can_be_continued_after_a_restart() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("sessions.db");
    std::env::set_var("ZORP_STATE_DB", &db);
    let id = seed(&db);

    // A server that has never seen this session, which is what a restart is.
    let addr = spawn().await;

    let turn = post_status(
        format!("http://{addr}/api/sessions/{id}/turn"),
        r#"{"message":"carry on"}"#,
    )
    .await;
    assert_eq!(
        turn, 202,
        "a session the store knows about was refused after a restart"
    );
    assert_eq!(
        get_status(format!("http://{addr}/api/sessions/{id}/events")).await,
        200,
        "the reopened session's event stream was refused"
    );
}

/// The other half: an id nobody has ever heard of stays a 404. The event
/// stream in particular must keep refusing those, because an empty stream
/// that ends at once is a reconnect loop by another name.
#[tokio::test]
async fn an_unknown_session_is_still_not_found() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("sessions.db");
    std::env::set_var("ZORP_STATE_DB", &db);
    seed(&db);

    let addr = spawn().await;

    assert_eq!(
        post_status(
            format!("http://{addr}/api/sessions/not-a-session/turn"),
            r#"{"message":"hello"}"#,
        )
        .await,
        404
    );
    assert_eq!(
        get_status(format!("http://{addr}/api/sessions/not-a-session/events")).await,
        404
    );
}

/// Deleting a conversation removes its row and its messages, so it is gone
/// from the sidebar and a replay comes back 404.
#[tokio::test]
async fn deleting_a_session_removes_it_from_the_list_and_the_store() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("sessions.db");
    std::env::set_var("ZORP_STATE_DB", &db);
    let id = seed(&db);

    let addr = spawn().await;

    assert_eq!(
        delete_status(format!("http://{addr}/api/sessions/{id}")).await,
        204
    );
    let transcript = get_json(format!("http://{addr}/api/sessions/{id}")).await;
    assert_eq!(
        transcript["messages"].as_array().unwrap().len(),
        0,
        "a deleted session's messages should be gone: {transcript}"
    );
    let listed = get_json(format!("http://{addr}/api/sessions")).await;
    let rows = listed.as_array().unwrap();
    assert!(
        !rows.iter().any(|s| s["id"] == id),
        "deleted session still listed: {listed}"
    );
}

/// The other half: deleting an id nobody has heard of is a 404, not a
/// silent success.
#[tokio::test]
async fn deleting_an_unknown_session_is_not_found() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("sessions.db");
    std::env::set_var("ZORP_STATE_DB", &db);
    seed(&db);

    let addr = spawn().await;

    assert_eq!(
        delete_status(format!("http://{addr}/api/sessions/not-a-session")).await,
        404
    );
}

/// A session with a turn in flight is refused rather than deleted out from
/// under the thread still writing to it.
///
/// The turn is parked on an approval, the same trick `tests/stop.rs` uses to
/// stop a genuinely running turn rather than one that raced past before the
/// request landed.
#[tokio::test]
async fn deleting_a_running_session_is_refused() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    let base = mock_script(vec![
        r#"{"choices":[{"message":{"content":null,"tool_calls":[{"id":"c1","type":"function","function":{"name":"write_file","arguments":"{\"path\":\"x.txt\",\"content\":\"x\\n\"}"}}]},"finish_reason":"tool_calls"}]}"#,
    ]);
    std::env::set_var("ZORP_BASE_URL", &base);
    std::env::set_var("ZORP_MODEL", "m");
    let db = dir.path().join("sessions.db");
    std::env::set_var("ZORP_STATE_DB", &db);
    std::env::remove_var("ZORP_API_KEY");
    let id = seed(&db);

    let addr = spawn().await;
    assert_eq!(
        post_status(
            format!("http://{addr}/api/sessions/{id}/turn"),
            r#"{"message":"carry on"}"#,
        )
        .await,
        202
    );

    let mut events = EventStream::connect(addr, &id);
    let parked = tokio::task::spawn_blocking(move || {
        let ok = events.wait_for("\"type\":\"approval_request\"", PATIENCE);
        (events, ok)
    })
    .await
    .unwrap();
    assert!(parked.1, "the agent never parked on an approval");

    assert_eq!(
        delete_status(format!("http://{addr}/api/sessions/{id}")).await,
        409
    );
    assert_eq!(
        get_status(format!("http://{addr}/api/sessions/{id}")).await,
        200,
        "a refused delete must leave the session in place"
    );
}
