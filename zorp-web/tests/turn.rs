mod common;
use common::mock_script;
use std::net::SocketAddr;
use std::sync::Mutex;

/// Model configuration lives in process-global env vars, so tests that set it
/// cannot run concurrently with each other.
static ENV: Mutex<()> = Mutex::new(());

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

fn get(url: &str) -> String {
    ureq::get(url).call().unwrap().into_string().unwrap()
}

#[tokio::test]
async fn a_turn_streams_assistant_text_then_done() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let base = mock_script(vec![
        r#"{"choices":[{"message":{"content":"hello from the model"},"finish_reason":"stop"}]}"#,
    ]);
    std::env::set_var("ZORP_BASE_URL", &base);
    std::env::set_var("ZORP_MODEL", "m");
    std::env::set_var("ZORP_STATE_DB", dir.path().join("s.db"));
    std::env::remove_var("ZORP_API_KEY");

    let addr = spawn().await;
    let (_, created) = tokio::task::spawn_blocking({
        let addr = addr;
        move || post(&format!("http://{addr}/api/sessions"), "{}")
    })
    .await
    .unwrap();
    let id = serde_json::from_str::<serde_json::Value>(&created).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, _) = tokio::task::spawn_blocking({
        let (addr, id) = (addr, id.clone());
        move || {
            post(
                &format!("http://{addr}/api/sessions/{id}/turn"),
                r#"{"message":"hi"}"#,
            )
        }
    })
    .await
    .unwrap();
    assert_eq!(status, 202, "turn should be accepted");

    let stream = tokio::task::spawn_blocking({
        let (addr, id) = (addr, id.clone());
        move || get(&format!("http://{addr}/api/sessions/{id}/events"))
    })
    .await
    .unwrap();

    assert!(
        stream.contains("hello from the model"),
        "assistant text missing from stream: {stream}"
    );
    assert!(
        stream.contains("\"type\":\"done\""),
        "stream never terminated: {stream}"
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

/// Two turns on one session must not restart the sequence. `Last-Event-ID`
/// resume is keyed on seq, so a repeat means a reconnecting browser silently
/// drops the second turn's events.
#[tokio::test]
async fn seq_keeps_climbing_across_turns() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
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
    let id = tokio::task::spawn_blocking(move || {
        let (_, body) = post(&format!("http://{addr}/api/sessions"), "{}");
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    })
    .await
    .unwrap();

    for _ in 0..2 {
        let tid = id.clone();
        tokio::task::spawn_blocking(move || {
            for _ in 0..200 {
                let (status, _) = post(
                    &format!("http://{addr}/api/sessions/{tid}/turn"),
                    r#"{"message":"go"}"#,
                );
                if status == 202 {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            panic!("turn never accepted");
        })
        .await
        .unwrap();
        let wait_id = id.clone();
        tokio::task::spawn_blocking(move || {
            ureq::get(&format!("http://{addr}/api/sessions/{wait_id}/events"))
                .call()
                .unwrap()
                .into_string()
                .unwrap()
        })
        .await
        .unwrap();
    }

    let stream = tokio::task::spawn_blocking(move || {
        ureq::get(&format!("http://{addr}/api/sessions/{id}/events"))
            .call()
            .unwrap()
            .into_string()
            .unwrap()
    })
    .await
    .unwrap();

    let seqs: Vec<u64> = stream
        .lines()
        .filter_map(|l| l.strip_prefix("id: "))
        .filter_map(|s| s.parse().ok())
        .collect();
    let mut sorted = seqs.clone();
    sorted.dedup();
    assert_eq!(seqs, sorted, "seq repeated across turns: {seqs:?}");
    assert!(
        seqs.windows(2).all(|w| w[0] < w[1]),
        "not increasing: {seqs:?}"
    );
}
