//! Developer mode's routes on a server built without `train`.
//!
//! The routes are registered whatever the feature says and answer 501, so a
//! browser is told the server does not do this rather than getting a 404 it
//! cannot tell from a typo. That is the claim `zorp-web/Cargo.toml` and
//! CLAUDE.md both make, and `train_api.rs` cannot check it because the whole
//! file is inside the feature. This one is the other half, and it runs in
//! the plain `cargo test --workspace` that gates every pull request.
#![cfg(not(feature = "train"))]

use std::net::SocketAddr;

async fn spawn() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = zorp_web::state::AppState::new();
    tokio::spawn(async move {
        axum::serve(listener, zorp_web::api::router_with_state(state))
            .await
            .unwrap();
    });
    addr
}

#[tokio::test]
async fn dev_routes_answer_501_without_the_feature() {
    let addr = spawn().await;

    for (method, path) in [
        ("GET", "/api/dev/status"),
        ("GET", "/api/dev/recipes"),
        ("GET", "/api/dev/models"),
        ("GET", "/api/dev/train/stream"),
        ("POST", "/api/dev/environment/setup"),
        ("POST", "/api/dev/tokenizer/train"),
        ("POST", "/api/dev/train/start"),
        ("POST", "/api/dev/models/some-run/serve"),
    ] {
        let url = format!("http://{addr}{path}");
        let (status, body) = tokio::task::spawn_blocking(move || {
            let call = if method == "GET" {
                ureq::get(&url).call()
            } else {
                ureq::post(&url).send_json(serde_json::json!({}))
            };
            match call {
                Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
                Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
                Err(e) => panic!("request failed: {e}"),
            }
        })
        .await
        .unwrap();

        assert_eq!(status, 501, "{method} {path} answered {status}: {body}");
        assert!(
            body.contains("train"),
            "{method} {path} did not name the feature: {body}"
        );
    }
}
