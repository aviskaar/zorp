//! A state-changing request another page sent is refused before any handler
//! runs.
//!
//! CORS keeps a foreign page from reading this server's answers, not from
//! sending a "simple" request, and the loopback install has no token. So a
//! route whose handler takes no JSON body would run for any page the person
//! visited. `POST /api/sessions` is one such route and always answers 200
//! with an id when it runs, so a 403 here means the handler was never
//! reached.

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

/// POST `path` with the given headers and a `text/plain` body, the shape a
/// page can send without a preflight. Returns the status.
async fn post(addr: SocketAddr, path: &str, headers: &[(&'static str, String)]) -> u16 {
    let url = format!("http://{addr}{path}");
    let headers = headers.to_vec();
    tokio::task::spawn_blocking(move || {
        let mut request = ureq::post(&url).set("Content-Type", "text/plain");
        for (name, value) in &headers {
            request = request.set(name, value);
        }
        match request.send_string("x") {
            Ok(r) => r.status(),
            Err(ureq::Error::Status(code, _)) => code,
            Err(e) => panic!("request failed: {e}"),
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn a_page_on_another_site_cannot_create_anything() {
    let addr = spawn(zorp_web::state::AppState::new()).await;
    let status = post(
        addr,
        "/api/sessions",
        &[
            ("Origin", "https://evil.example".into()),
            ("Sec-Fetch-Site", "cross-site".into()),
        ],
    )
    .await;
    assert_eq!(status, 403, "a page the person visited created a session");
}

/// A dev server on another loopback port is the same site but not the same
/// origin. It is another program, and it is refused.
#[tokio::test]
async fn a_page_on_another_loopback_port_is_refused() {
    let addr = spawn(zorp_web::state::AppState::new()).await;
    let status = post(
        addr,
        "/api/sessions",
        &[
            ("Origin", "http://127.0.0.1:5173".into()),
            ("Sec-Fetch-Site", "same-site".into()),
        ],
    )
    .await;
    assert_eq!(status, 403);
}

/// The trust route takes no body at all, which is why it needs this most.
/// Without the guard an unknown agent answers 404 from inside the handler.
#[tokio::test]
async fn trusting_an_agent_from_another_page_never_reaches_the_handler() {
    let addr = spawn(zorp_web::state::AppState::new()).await;
    let status = post(
        addr,
        "/api/agents/project/anything/trust",
        &[
            ("Origin", "https://evil.example".into()),
            ("Sec-Fetch-Site", "cross-site".into()),
        ],
    )
    .await;
    assert_eq!(status, 403);
}

/// The UI's own requests, served by this server or through the container's
/// nginx, which rewrites `Host`, both say `same-origin`.
#[tokio::test]
async fn the_servers_own_page_is_allowed() {
    let addr = spawn(zorp_web::state::AppState::new()).await;
    let status = post(
        addr,
        "/api/sessions",
        &[
            ("Origin", "http://localhost:8080".into()),
            ("Sec-Fetch-Site", "same-origin".into()),
        ],
    )
    .await;
    assert_eq!(status, 200);
}

/// curl, the CLI and the Mac app send neither header, and are not a page.
#[tokio::test]
async fn a_request_that_is_not_from_a_page_is_allowed() {
    let addr = spawn(zorp_web::state::AppState::new()).await;
    assert_eq!(post(addr, "/api/sessions", &[]).await, 200);
}

/// A browser too old to send `Sec-Fetch-Site` is judged on `Origin` against
/// `Host`.
#[tokio::test]
async fn without_fetch_metadata_the_origin_must_match_the_host() {
    let addr = spawn(zorp_web::state::AppState::new()).await;
    assert_eq!(
        post(
            addr,
            "/api/sessions",
            &[("Origin", format!("http://{addr}"))]
        )
        .await,
        200
    );
    assert_eq!(
        post(
            addr,
            "/api/sessions",
            &[("Origin", "https://evil.example".into())]
        )
        .await,
        403
    );
    assert_eq!(
        post(addr, "/api/sessions", &[("Origin", "null".into())]).await,
        403
    );
}

/// An origin named with `--allow-origin` is the container split's UI, and
/// passes whatever the browser says about the site.
#[tokio::test]
async fn a_named_origin_is_allowed() {
    let state =
        zorp_web::state::AppState::new().with_allowed_origins(vec!["http://localhost:8080".into()]);
    let addr = spawn(state).await;
    let status = post(
        addr,
        "/api/sessions",
        &[
            ("Origin", "http://localhost:8080".into()),
            ("Sec-Fetch-Site", "same-site".into()),
        ],
    )
    .await;
    assert_eq!(status, 200);
}
