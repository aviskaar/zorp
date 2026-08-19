//! What `GET /api/sessions/:id` hands the browser to rebuild a transcript.
//!
//! The endpoint's contract is a role and the text of each turn. A turn where
//! the model only called a tool has no text, and the browser has nothing to
//! draw for it: tool activity reaches the page as its own event kind, not as
//! a message. So a content-free row is not a message the transcript is
//! missing detail for, it is not a message at all.

use std::net::SocketAddr;
use std::path::Path;
use tokio::sync::Mutex;
use zorp_agent::{Message, Store, ToolCall};

/// `ZORP_STATE_DB` is process wide, so tests that set it take turns.
static ENV: Mutex<()> = Mutex::const_new(());

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
