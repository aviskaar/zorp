mod common;
use common::{mock_script, EventStream};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::sync::Mutex;

/// Model configuration lives in process-global env vars, so tests that set it
/// cannot run concurrently with each other. tokio's mutex, not std's: the
/// guard is deliberately held across awaits, which is the whole point of the
/// lock, and a std guard held across an await is what
/// `clippy::await_holding_lock` is about. tokio's also does not poison, so a
/// test that fails while holding it no longer makes the next test fail with a
/// message about a poisoned lock instead of its real cause.
static ENV: Mutex<()> = Mutex::const_new(());

/// Long enough for a mock-backed turn to finish, short enough that a broken
/// server fails the test instead of hanging the suite.
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
    let url = url.to_string();
    let body = body.to_string();
    match ureq::post(&url)
        .set("content-type", "application/json")
        .send_string(&body)
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

/// Start a turn, waiting out the 409 a still-running previous turn returns.
async fn start_turn(addr: SocketAddr, id: &str) {
    let id = id.to_string();
    tokio::task::spawn_blocking(move || {
        for _ in 0..200 {
            let (status, _) = post(
                &format!("http://{addr}/api/sessions/{id}/turn"),
                r#"{"message":"go"}"#,
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

#[tokio::test]
async fn a_turn_streams_assistant_text_then_done() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let base = mock_script(vec![
        r#"{"choices":[{"message":{"content":"hello from the model"},"finish_reason":"stop"}]}"#,
    ]);
    std::env::set_var("ZORP_BASE_URL", &base);
    std::env::set_var("ZORP_MODEL", "m");
    std::env::set_var("ZORP_STATE_DB", dir.path().join("s.db"));
    std::env::remove_var("ZORP_API_KEY");

    let addr = spawn().await;
    let id = new_session(addr).await;
    start_turn(addr, &id).await;

    let events = on_stream(EventStream::connect(addr, &id), |events| {
        assert!(
            events.wait_for("\"type\":\"done\"", PATIENCE),
            "the turn never finished: {}",
            events.text()
        );
    })
    .await;

    assert!(
        events.text().contains("hello from the model"),
        "assistant text missing from stream: {}",
        events.text()
    );
}

#[tokio::test]
async fn a_turn_on_a_missing_session_is_not_found() {
    let addr = spawn().await;
    let (status, _) = tokio::task::spawn_blocking(move || {
        post(
            &format!("http://{addr}/api/sessions/nope/turn"),
            r#"{"message":"hi"}"#,
        )
    })
    .await
    .unwrap();
    assert_eq!(status, 404);
}

/// The stream has to outlive the turn.
///
/// `EventSource` reconnects on its own whenever the server closes the
/// response, so a stream that ends when a turn ends puts an idle browser into
/// a reconnect loop: a new request every few seconds for as long as the tab is
/// open, and a status badge stuck on "reconnecting" for a conversation that
/// finished.
#[tokio::test]
async fn a_finished_turn_leaves_the_stream_open() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let base = mock_script(vec![
        r#"{"choices":[{"message":{"content":"all done"},"finish_reason":"stop"}]}"#,
    ]);
    std::env::set_var("ZORP_BASE_URL", &base);
    std::env::set_var("ZORP_MODEL", "m");
    std::env::set_var("ZORP_STATE_DB", dir.path().join("open.db"));
    std::env::remove_var("ZORP_API_KEY");

    let addr = spawn().await;
    let id = new_session(addr).await;
    start_turn(addr, &id).await;

    let events = on_stream(EventStream::connect(addr, &id), |events| {
        assert!(
            events.wait_for("\"type\":\"done\"", PATIENCE),
            "the turn never finished: {}",
            events.text()
        );
        assert!(
            !events.response_ended_within(Duration::from_millis(750)),
            "the server ended the response when the turn finished, so the browser will reconnect forever"
        );
    })
    .await;
    drop(events);
}

/// The reconnect loop used to be load bearing: the browser keeps one stream
/// per session, the server closed it every turn, and the next turn's events
/// arrived over one of the automatic reconnects. Now that the stream stays
/// open, a second and a third turn have to stream on that same connection.
#[tokio::test]
async fn later_turns_stream_on_the_same_connection() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let base = mock_script(vec![
        r#"{"choices":[{"message":{"content":"first"},"finish_reason":"stop"}]}"#,
        r#"{"choices":[{"message":{"content":"second"},"finish_reason":"stop"}]}"#,
        r#"{"choices":[{"message":{"content":"third"},"finish_reason":"stop"}]}"#,
    ]);
    std::env::set_var("ZORP_BASE_URL", &base);
    std::env::set_var("ZORP_MODEL", "m");
    std::env::set_var("ZORP_STATE_DB", dir.path().join("multi.db"));
    std::env::remove_var("ZORP_API_KEY");

    let addr = spawn().await;
    let id = new_session(addr).await;
    let mut events = EventStream::connect(addr, &id);

    for (turn, answer) in ["first", "second", "third"].iter().enumerate() {
        start_turn(addr, &id).await;
        let want = turn + 1;
        let answer = answer.to_string();
        events = on_stream(events, move |events| {
            assert!(
                events.wait_for_count("\"type\":\"done\"", want, PATIENCE),
                "turn {want} never finished on the connection that was already open: {}",
                events.text()
            );
            assert!(
                events.text().contains(&answer),
                "turn {want} did not stream its answer: {}",
                events.text()
            );
        })
        .await;
    }

    // Every event arrived once, so a browser rendering this transcript does
    // not show the conversation twice.
    let seqs = events.seqs();
    let mut unique = seqs.clone();
    unique.dedup();
    assert_eq!(seqs, unique, "an event was delivered twice: {seqs:?}");
}

/// Two turns on one session must not restart the sequence. `Last-Event-ID`
/// resume is keyed on seq, so a repeat means a reconnecting browser silently
/// drops the second turn's events.
#[tokio::test]
async fn seq_keeps_climbing_across_turns() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let base = mock_script(vec![
        r#"{"choices":[{"message":{"content":"first"},"finish_reason":"stop"}]}"#,
        r#"{"choices":[{"message":{"content":"second"},"finish_reason":"stop"}]}"#,
    ]);
    std::env::set_var("ZORP_BASE_URL", &base);
    std::env::set_var("ZORP_MODEL", "m");
    std::env::set_var("ZORP_STATE_DB", dir.path().join("s2.db"));
    std::env::remove_var("ZORP_API_KEY");

    let addr = spawn().await;
    let id = new_session(addr).await;
    start_turn(addr, &id).await;
    start_turn(addr, &id).await;

    // A browser arriving fresh replays the whole session from seq 0.
    let events = on_stream(EventStream::connect(addr, &id), |events| {
        assert!(
            events.wait_for_count("\"type\":\"done\"", 2, PATIENCE),
            "both turns never finished: {}",
            events.text()
        );
    })
    .await;

    let seqs = events.seqs();
    let mut sorted = seqs.clone();
    sorted.dedup();
    assert_eq!(seqs, sorted, "seq repeated across turns: {seqs:?}");
    assert!(
        seqs.windows(2).all(|w| w[0] < w[1]),
        "not increasing: {seqs:?}"
    );
}

/// A browser that really did lose its connection resumes with `Last-Event-ID`
/// and must be sent what it missed, and only what it missed.
#[tokio::test]
async fn a_resumed_stream_replays_only_what_was_missed() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let base = mock_script(vec![
        r#"{"choices":[{"message":{"content":"the whole answer"},"finish_reason":"stop"}]}"#,
    ]);
    std::env::set_var("ZORP_BASE_URL", &base);
    std::env::set_var("ZORP_MODEL", "m");
    std::env::set_var("ZORP_STATE_DB", dir.path().join("resume.db"));
    std::env::remove_var("ZORP_API_KEY");

    let addr = spawn().await;
    let id = new_session(addr).await;
    start_turn(addr, &id).await;

    let first = on_stream(EventStream::connect(addr, &id), |events| {
        assert!(
            events.wait_for("\"type\":\"done\"", PATIENCE),
            "the turn never finished: {}",
            events.text()
        );
    })
    .await;
    let seen = first.seqs();
    let last = *seen.last().expect("no events at all");
    assert!(last > 0, "a one event turn cannot test resume: {seen:?}");
    drop(first);

    // Come back claiming to have seen everything but the final event.
    let resumed = on_stream(EventStream::resume(addr, &id, Some(last - 1)), |events| {
        assert!(
            events.wait_for("\"type\":\"done\"", PATIENCE),
            "the resumed stream never caught up: {}",
            events.text()
        );
    })
    .await;
    assert_eq!(
        resumed.seqs(),
        vec![last],
        "a resume replayed events the browser already had: {}",
        resumed.text()
    );
}

/// Opening a session the server does not have in memory, which is every
/// session in the sidebar after a restart, must not become a reconnect loop
/// either. An empty stream that closes immediately is one, so say plainly
/// that there is nothing to stream.
#[tokio::test]
async fn events_for_an_unknown_session_are_not_found() {
    let addr = spawn().await;
    let mut events =
        tokio::task::spawn_blocking(move || EventStream::connect(addr, "no-such-session"))
            .await
            .unwrap();
    let status = tokio::task::spawn_blocking(move || events.status_line(PATIENCE))
        .await
        .unwrap();
    assert!(
        status.contains("404"),
        "expected a 404 for an unknown session, got: {status}"
    );
}

/// With nothing configured anywhere (no UI setting, no `ZORP_*` env var),
/// the turn must fail with a clear, actionable error event instead of
/// reaching a real provider and coming back with a raw 401. This is the bug
/// the settings feature exists to fix: previously
/// `HttpModel::try_from_env` silently defaulted to
/// `https://api.openai.com/v1` and `gpt-4o` with no key, so the first
/// message a fresh install ever sent died deep inside the provider call.
#[tokio::test]
async fn a_turn_with_nothing_configured_fails_with_a_clear_error_not_a_provider_401() {
    let _env = ENV.lock().await;
    for var in [
        "ZORP_PROVIDER",
        "ZORP_BASE_URL",
        "ZORP_MODEL",
        "ZORP_API_KEY",
        "ZORP_MAX_TOKENS",
    ] {
        std::env::remove_var(var);
    }
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZORP_STATE_DB", dir.path().join("unconfigured.db"));

    let addr = spawn().await;
    let id = new_session(addr).await;
    start_turn(addr, &id).await;

    let events = on_stream(EventStream::connect(addr, &id), |events| {
        assert!(
            events.wait_for("\"type\":\"error\"", PATIENCE),
            "no error event arrived: {}",
            events.text()
        );
    })
    .await;

    let text = events.text();
    assert!(
        text.contains("no model configured"),
        "error message did not explain the real cause: {text}"
    );
    assert!(
        text.contains("settings"),
        "error message did not point at settings: {text}"
    );
    assert!(
        !text.contains("api.openai.com"),
        "the turn reached a real provider instead of refusing up front: {text}"
    );
}
