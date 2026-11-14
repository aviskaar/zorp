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
            // No body on the POSTs. These handlers never read one, and hyper
            // closes a connection whose request body is still unread when
            // the response goes out. A body that lands after that close is
            // answered with a reset, and on macOS the reset makes ureq's
            // `set_read_timeout` on returning the socket to its pool fail
            // with "Invalid argument", which it turns into a panic.
            let call = ureq::request(method, &url).call();
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
