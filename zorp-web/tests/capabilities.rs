//! `GET /api/capabilities`: what this build can actually do.
//!
//! There is one capability here, `web_search`, and the endpoint exists
//! because the browser cannot work it out for itself. Three separate things
//! decide whether that tool is there: whether `zorp-web` was built with the
//! `search` feature, whether the policy permits the tool, and whether the
//! search provider found its key in the server's environment. A page can see
//! none of the three, so it has to be told.
//!
//! A default build has the feature off, which is the state these tests run
//! in unless `--features search` says otherwise.

use std::net::SocketAddr;
use tokio::sync::Mutex;
use zorp_web::state::AppState;

/// The search provider reads its key from the process environment, so tests
/// that set or clear it cannot run beside each other. tokio's mutex, not
/// std's, for the reason `zorp-web/tests/settings.rs` gives: it does not
/// poison, so one failing test does not take the rest down with it.
#[cfg_attr(not(feature = "search"), allow(dead_code))]
static ENV: Mutex<()> = Mutex::const_new(());

async fn spawn_with(state: AppState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, zorp_web::api::router_with_state(state))
            .await
            .unwrap();
    });
    addr
}

async fn spawn() -> SocketAddr {
    spawn_with(AppState::with_token(None)).await
}

fn get(url: &str) -> (u16, String) {
    match ureq::get(url).call() {
        Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
        Err(e) => panic!("{e}"),
    }
}

/// The reported `web_search` capability, as the browser would read it.
async fn web_search(addr: SocketAddr) -> serde_json::Value {
    let url = format!("http://{addr}/api/capabilities");
    let (status, body) = tokio::task::spawn_blocking(move || get(&url))
        .await
        .unwrap();
    assert_eq!(status, 200, "{body}");
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    value
        .get("web_search")
        .cloned()
        .unwrap_or_else(|| panic!("no web_search in {body}"))
}

fn available(capability: &serde_json::Value) -> bool {
    capability
        .get("available")
        .and_then(serde_json::Value::as_bool)
        .expect("web_search.available must be a boolean")
}

fn detail(capability: &serde_json::Value) -> String {
    capability
        .get("detail")
        .and_then(serde_json::Value::as_str)
        .expect("web_search.detail must be a string")
        .to_string()
}

/// The default build compiles the tool out entirely, and says so. This is
/// the case the indicator in the browser is off for, and it is the one most
/// people are in: egress stays opt-in here exactly as it does in
/// `zorp-agent`.
#[cfg(not(feature = "search"))]
#[tokio::test]
async fn without_the_search_feature_web_search_is_unavailable() {
    let addr = spawn().await;
    let capability = web_search(addr).await;
    assert!(!available(&capability), "{capability}");
    let detail = detail(&capability);
    assert!(
        detail.contains("search feature"),
        "the reason has to name the feature: {detail}"
    );
}

/// A key that is missing is not a build problem, and the difference matters
/// to whoever is trying to turn search on. The reason names the variable.
#[cfg(feature = "search")]
#[tokio::test]
async fn with_the_feature_but_no_key_web_search_is_unavailable() {
    let _guard = ENV.lock().await;
    std::env::remove_var("ZORP_TAVILY_API_KEY");
    let addr = spawn().await;
    let capability = web_search(addr).await;
    assert!(!available(&capability), "{capability}");
    let detail = detail(&capability);
    assert!(
        detail.contains("ZORP_TAVILY_API_KEY"),
        "the reason has to name the variable: {detail}"
    );
}

/// With the feature and a key, the tool registers and the endpoint says so.
///
/// The key here is nonsense, and that is the honest scope of the answer:
/// this reports that the tool is there, not that the key works. Finding out
/// whether Tavily accepts it would mean spending a real search on every
/// page load.
#[cfg(feature = "search")]
#[tokio::test]
async fn with_the_feature_and_a_key_web_search_is_available() {
    let _guard = ENV.lock().await;
    std::env::set_var("ZORP_TAVILY_API_KEY", "test-key-not-a-real-one");
    let addr = spawn().await;
    let capability = web_search(addr).await;
    std::env::remove_var("ZORP_TAVILY_API_KEY");
    assert!(available(&capability), "{capability}");
}

/// Same gate as every other route. It reports what this server is built
/// with and what its environment holds, which is not something to hand out
/// to an unauthenticated caller when the server is reachable off this
/// machine.
#[tokio::test]
async fn capabilities_is_behind_the_token_gate() {
    let addr = spawn_with(AppState::with_token(Some("sekrit".to_string()))).await;
    let url = format!("http://{addr}/api/capabilities");
    let (status, body) = tokio::task::spawn_blocking(move || get(&url))
        .await
        .unwrap();
    assert_eq!(status, 401, "{body}");
}
