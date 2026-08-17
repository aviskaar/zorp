mod common;
use common::mock_script;
use std::net::SocketAddr;

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
