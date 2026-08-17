use std::net::SocketAddr;
use zorp_web::state::AppState;

async fn spawn(token: Option<&str>) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = AppState::with_token(token.map(str::to_string));
    tokio::spawn(async move {
        axum::serve(listener, zorp_web::api::router_with_state(state))
            .await
            .unwrap();
    });
    addr
}

fn status(url: &str) -> u16 {
    match ureq::get(url).call() {
        Ok(r) => r.status(),
        Err(ureq::Error::Status(code, _)) => code,
        Err(e) => panic!("{e}"),
    }
}

fn status_with_header(url: &str, value: &str) -> u16 {
    match ureq::get(url).set("authorization", value).call() {
        Ok(r) => r.status(),
        Err(ureq::Error::Status(code, _)) => code,
        Err(e) => panic!("{e}"),
    }
}

#[tokio::test]
async fn no_token_configured_means_no_token_required() {
    let addr = spawn(None).await;
    let code = tokio::task::spawn_blocking(move || status(&format!("http://{addr}/api/health")))
        .await
        .unwrap();
    assert_eq!(code, 200);
}

#[tokio::test]
async fn a_configured_token_is_enforced() {
    let addr = spawn(Some("sekrit")).await;
    let code = tokio::task::spawn_blocking(move || status(&format!("http://{addr}/api/health")))
        .await
        .unwrap();
    assert_eq!(code, 401, "an unauthenticated request should be refused");
}

#[tokio::test]
async fn the_header_form_is_accepted() {
    let addr = spawn(Some("sekrit")).await;
    let code = tokio::task::spawn_blocking(move || {
        status_with_header(&format!("http://{addr}/api/health"), "Bearer sekrit")
    })
    .await
    .unwrap();
    assert_eq!(code, 200);
}

/// EventSource cannot set headers, so a header-only scheme would leave the
/// event stream unusable and with it the entire UI.
#[tokio::test]
async fn the_query_parameter_form_is_accepted() {
    let addr = spawn(Some("sekrit")).await;
    let code = tokio::task::spawn_blocking(move || {
        status(&format!("http://{addr}/api/health?token=sekrit"))
    })
    .await
    .unwrap();
    assert_eq!(code, 200);
}

#[tokio::test]
async fn a_wrong_token_is_refused() {
    let addr = spawn(Some("sekrit")).await;
    let code = tokio::task::spawn_blocking(move || {
        status(&format!("http://{addr}/api/health?token=guess"))
    })
    .await
    .unwrap();
    assert_eq!(code, 401);
}
