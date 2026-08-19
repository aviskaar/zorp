//! zorp-web serving the chat UI itself.
//!
//! Until this existed the server answered `/api/*` and nothing else, so a
//! user who installed zorp, ran `zorp-web` and opened the URL the installer
//! prints got a 404 on every asset. The API was fine, which is what made it
//! hard to notice: `/api/health` returned 200 the whole time.

use std::fs;
use std::net::SocketAddr;
use std::path::Path;

/// Write the three files install.sh actually lays down, in the same shape:
/// index.html and styles.css at the top, the bundle under dist/.
fn ui_tree(root: &Path) {
    fs::write(
        root.join("index.html"),
        "<!doctype html><title>zorp</title>",
    )
    .unwrap();
    fs::write(root.join("styles.css"), "body{}").unwrap();
    fs::create_dir_all(root.join("dist")).unwrap();
    fs::write(root.join("dist").join("main.js"), "console.log('zorp')").unwrap();
}

async fn spawn_with_ui(dir: Option<std::path::PathBuf>) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = zorp_web::state::AppState::with_token(None);
    tokio::spawn(async move {
        axum::serve(listener, zorp_web::api::router_with_ui(state, dir))
            .await
            .unwrap();
    });
    addr
}

async fn get(url: String) -> (u16, String) {
    tokio::task::spawn_blocking(move || match ureq::get(&url).call() {
        Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
        Err(e) => panic!("request failed: {e}"),
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn root_serves_the_page() {
    let dir = tempfile::tempdir().unwrap();
    ui_tree(dir.path());
    let addr = spawn_with_ui(Some(dir.path().to_path_buf())).await;
    let (status, body) = get(format!("http://{addr}/")).await;
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("<title>zorp</title>"), "got: {body}");
}

/// The bundle lives under dist/. Serving the root but not this is the same
/// blank page from the user's side.
#[tokio::test]
async fn nested_assets_are_served() {
    let dir = tempfile::tempdir().unwrap();
    ui_tree(dir.path());
    let addr = spawn_with_ui(Some(dir.path().to_path_buf())).await;

    let (status, body) = get(format!("http://{addr}/dist/main.js")).await;
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("console.log"), "got: {body}");

    let (status, _) = get(format!("http://{addr}/styles.css")).await;
    assert_eq!(status, 200);
}

/// Mounting the UI must not shadow the API. This is the regression that
/// would turn one broken surface into two.
#[tokio::test]
async fn the_api_still_answers_with_a_ui_mounted() {
    let dir = tempfile::tempdir().unwrap();
    ui_tree(dir.path());
    let addr = spawn_with_ui(Some(dir.path().to_path_buf())).await;
    let (status, body) = get(format!("http://{addr}/api/health")).await;
    assert_eq!(status, 200);
    assert!(body.contains("\"status\":\"ok\""), "got: {body}");
}

/// The container split serves the UI from nginx and points it at this
/// server, so running with no UI directory stays valid.
#[tokio::test]
async fn without_a_ui_directory_the_api_still_answers() {
    let addr = spawn_with_ui(None).await;
    let (status, body) = get(format!("http://{addr}/api/health")).await;
    assert_eq!(status, 200);
    assert!(body.contains("\"status\":\"ok\""), "got: {body}");
}

/// Status, plus one response header.
async fn get_header(url: String, name: &'static str) -> (u16, Option<String>) {
    tokio::task::spawn_blocking(move || {
        let r = match ureq::get(&url).call() {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("request failed: {e}"),
        };
        (r.status(), r.header(name).map(str::to_string))
    })
    .await
    .unwrap()
}

/// A conditional request, the way a browser revalidates.
async fn get_if_modified_since(url: String, since: String) -> u16 {
    tokio::task::spawn_blocking(move || {
        match ureq::get(&url).set("If-Modified-Since", &since).call() {
            Ok(r) => r.status(),
            Err(ureq::Error::Status(code, _)) => code,
            Err(e) => panic!("request failed: {e}"),
        }
    })
    .await
    .unwrap()
}

/// Every file the page is built from must be revalidated before it is reused.
///
/// None of these names carry a content hash, so a rebuilt `dist/main.js`
/// arrives at the same URL as the one already in the browser's cache. With no
/// `Cache-Control` at all a browser is free to guess a freshness lifetime from
/// `Last-Modified`, and it does: a rebuilt bundle would run against a freshly
/// fetched `index.html`, which is how a UI ends up half old and half new with
/// nothing in the server log to explain it.
#[tokio::test]
async fn the_files_the_page_is_built_from_are_revalidated() {
    let dir = tempfile::tempdir().unwrap();
    ui_tree(dir.path());
    let addr = spawn_with_ui(Some(dir.path().to_path_buf())).await;

    for path in ["/", "/index.html", "/styles.css", "/dist/main.js"] {
        let (status, cache) = get_header(format!("http://{addr}{path}"), "cache-control").await;
        assert_eq!(status, 200, "{path}");
        assert_eq!(
            cache.as_deref(),
            Some("no-cache"),
            "{path} may be reused without asking the server first",
        );
    }
}

/// `no-cache` means revalidate, not refetch. The point of choosing it over
/// `no-store` is that an unchanged file still answers 304 with no body, so
/// the cost of always asking is one small conditional request.
#[tokio::test]
async fn an_unchanged_file_still_answers_304() {
    let dir = tempfile::tempdir().unwrap();
    ui_tree(dir.path());
    let addr = spawn_with_ui(Some(dir.path().to_path_buf())).await;

    let url = format!("http://{addr}/dist/main.js");
    let (status, last_modified) = get_header(url.clone(), "last-modified").await;
    assert_eq!(status, 200);
    let last_modified = last_modified.expect("no Last-Modified to revalidate against");

    assert_eq!(
        get_if_modified_since(url, last_modified).await,
        304,
        "revalidating an unchanged file sent the whole thing again",
    );
}

/// The API answers with its own headers and must not pick these up: a JSON
/// endpoint has nothing to revalidate and no cache entry to worry about.
#[tokio::test]
async fn the_api_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    ui_tree(dir.path());
    let addr = spawn_with_ui(Some(dir.path().to_path_buf())).await;
    let (status, cache) = get_header(format!("http://{addr}/api/health"), "cache-control").await;
    assert_eq!(status, 200);
    assert_eq!(cache, None, "the API grew a static file header");
}
