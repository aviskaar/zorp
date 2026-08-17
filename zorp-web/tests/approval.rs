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
    match ureq::post(url)
        .set("content-type", "application/json")
        .send_string(body)
    {
        Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
        Err(e) => panic!("{e}"),
    }
}

/// A denied write must leave the file absent. This is the whole point of the
/// approval gate, so it is asserted on disk and not just in the transcript.
#[tokio::test]
async fn a_denied_write_never_touches_the_disk() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    let base = mock_script(vec![
        r#"{"choices":[{"message":{"content":null,"tool_calls":[{"id":"c1","type":"function","function":{"name":"write_file","arguments":"{\"path\":\"denied.txt\",\"content\":\"x\\n\"}"}}]},"finish_reason":"tool_calls"}]}"#,
        r#"{"choices":[{"message":{"content":"could not write"},"finish_reason":"stop"}]}"#,
    ]);
    std::env::set_var("ZORP_BASE_URL", &base);
    std::env::set_var("ZORP_MODEL", "m");
    std::env::set_var("ZORP_STATE_DB", dir.path().join("s.db"));
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

    let turn_id = id.clone();
    tokio::task::spawn_blocking(move || {
        post(
            &format!("http://{addr}/api/sessions/{turn_id}/turn"),
            r#"{"message":"write denied.txt"}"#,
        )
    })
    .await
    .unwrap();

    // Deny as soon as the agent parks. Retry because the request has to reach
    // the approver before a decision can land.
    let deny_id = id.clone();
    let denied = tokio::task::spawn_blocking(move || {
        for _ in 0..200 {
            let (status, _) = post(
                &format!("http://{addr}/api/sessions/{deny_id}/approve"),
                r#"{"allow":false}"#,
            );
            if status == 200 {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        false
    })
    .await
    .unwrap();
    assert!(denied, "no approval request ever arrived");

    let stream = tokio::task::spawn_blocking(move || {
        ureq::get(&format!("http://{addr}/api/sessions/{id}/events"))
            .call()
            .unwrap()
            .into_string()
            .unwrap()
    })
    .await
    .unwrap();

    assert!(
        stream.contains("approval_request"),
        "the browser was never asked: {stream}"
    );
    assert!(
        !dir.path().join("denied.txt").exists(),
        "a denied write reached the disk"
    );
}
