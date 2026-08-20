//! Which origins are allowed to drive this server.
//!
//! This matters more here than it would on most servers. `POST /turn` runs an
//! agent that executes commands and edits files on the machine the server was
//! started on, and the token gate is only armed on a non-loopback bind, so on
//! the ordinary loopback install there is no secret at all. If any origin may
//! call the API, then any page the user happens to visit can drive that agent
//! and read what it produced, without the user ever seeing the request.
//!
//! Same-origin is the normal case and is unaffected: when `zorp-web` serves
//! the UI itself, the page and the API share an origin and the browser sends
//! no CORS preflight. Cross-origin is the container split, where the UI comes
//! from nginx, and that now has to be named rather than assumed.

use std::net::SocketAddr;

async fn spawn(state: zorp_web::state::AppState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, zorp_web::api::router_with_state(state))
            .await
            .unwrap();
    });
    addr
}

/// The `Access-Control-Allow-Origin` a preflight comes back with, if any.
async fn preflight_allows(addr: SocketAddr, origin: &'static str) -> Option<String> {
    let url = format!("http://{addr}/api/sessions");
    tokio::task::spawn_blocking(move || {
        let response = match ureq::request("OPTIONS", &url)
            .set("Origin", origin)
            .set("Access-Control-Request-Method", "POST")
            .set("Access-Control-Request-Headers", "content-type")
            .call()
        {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("preflight failed: {e}"),
        };
        response
            .header("access-control-allow-origin")
            .map(str::to_string)
    })
    .await
    .unwrap()
}

/// A page on some other origin must not be able to reach the API just by
/// asking. This is the whole point.
#[tokio::test]
async fn an_unknown_origin_is_not_allowed_by_default() {
    let addr = spawn(zorp_web::state::AppState::new()).await;
    assert_eq!(
        preflight_allows(addr, "https://evil.example").await,
        None,
        "any page the user visits can drive this agent",
    );
}

/// The ordinary case: no `Origin` header at all, which is what a same-origin
/// browser request and every command line client send. Nothing here may break
/// that, and a CORS change that locked out the normal install would be a
/// worse bug than the one it fixed.
#[tokio::test]
async fn a_request_with_no_origin_is_untouched() {
    let addr = spawn(zorp_web::state::AppState::new()).await;
    let url = format!("http://{addr}/api/health");
    let body =
        tokio::task::spawn_blocking(move || ureq::get(&url).call().unwrap().into_string().unwrap())
            .await
            .unwrap();
    assert!(body.contains("\"status\":\"ok\""), "got {body}");
}

/// The container split: the UI is served from somewhere else and that origin
/// is named on the command line.
#[tokio::test]
async fn a_named_origin_is_allowed() {
    let state = zorp_web::state::AppState::new()
        .with_allowed_origins(vec!["https://ui.example".to_string()]);
    let addr = spawn(state).await;
    assert_eq!(
        preflight_allows(addr, "https://ui.example")
            .await
            .as_deref(),
        Some("https://ui.example"),
    );
}

/// Naming one origin must not open the door to the rest.
#[tokio::test]
async fn naming_one_origin_does_not_admit_another() {
    let state = zorp_web::state::AppState::new()
        .with_allowed_origins(vec!["https://ui.example".to_string()]);
    let addr = spawn(state).await;
    assert_eq!(
        preflight_allows(addr, "https://evil.example").await,
        None,
        "an allowlist that admits anything is not an allowlist",
    );
}

/// `index.html` opened straight off disk sends `Origin: null`. It is a real
/// case the old comment called out, and it stays available, but it has to be
/// asked for: `null` is also what a sandboxed iframe sends, so allowing it
/// silently would be the same hole under a different name.
#[tokio::test]
async fn the_file_origin_is_available_but_only_when_asked_for() {
    let closed = spawn(zorp_web::state::AppState::new()).await;
    assert_eq!(preflight_allows(closed, "null").await, None);

    let opened =
        spawn(zorp_web::state::AppState::new().with_allowed_origins(vec!["null".to_string()]))
            .await;
    assert_eq!(
        preflight_allows(opened, "null").await.as_deref(),
        Some("null")
    );
}
