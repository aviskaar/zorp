use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;
use zorp_web::state::AppState;

/// The token the boundary tests configure, matching the `--token` a
/// non-loopback `zorp-web` is made to pass.
const TOKEN: &str = "secrettoken";

/// What `require_token` writes when it refuses. The 401 tests assert on it so
/// a 401 from somewhere else, a missing route or a panicking handler, cannot
/// pass for a 401 from the token middleware.
const REFUSED: &str = "missing or wrong token";

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

/// The server as `--bind 0.0.0.0 --token ... --ui-dir ...` builds it: a token
/// configured, and the chat UI mounted underneath the API.
async fn spawn_with_ui(token: Option<&str>, dir: PathBuf) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = AppState::with_token(token.map(str::to_string));
    tokio::spawn(async move {
        axum::serve(listener, zorp_web::api::router_with_ui(state, Some(dir)))
            .await
            .unwrap();
    });
    addr
}

/// The three files install.sh lays down, in the same shape: index.html and
/// styles.css at the top, the bundle under dist/. Same shape as tests/ui.rs.
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

/// A client that does not follow redirects.
///
/// Left on the default, a 3xx would be chased and whatever it landed on would
/// be reported instead. A test looking for 200 would then pass on a server
/// that answered something else entirely at the address under test.
fn client() -> ureq::Agent {
    ureq::AgentBuilder::new().redirects(0).build()
}

fn get(url: &str, authorization: Option<&str>) -> (u16, String) {
    let mut request = client().get(url);
    if let Some(value) = authorization {
        request = request.set("authorization", value);
    }
    match request.call() {
        Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
        Err(e) => panic!("{e}"),
    }
}

fn status(url: &str) -> u16 {
    get(url, None).0
}

fn status_with_header(url: &str, value: &str) -> u16 {
    get(url, Some(value)).0
}

/// Send `target` as the request target verbatim, with no client-side rewriting.
///
/// The traversal cases need the `..` to still be there when the bytes reach the
/// server, and `ureq` will not do that: it parses the URL through the `url`
/// crate, which resolves `..` first. Measured, not assumed. Asking ureq for
/// `/../../../etc/passwd` and for `/dist/../../../etc/passwd` puts
/// `GET /etc/passwd HTTP/1.1` on the wire in both cases, so a ureq-based
/// traversal test is really just a request for a file that was never there.
/// Writing the request line by hand is the same thing `curl --path-as-is` does.
fn raw_get(addr: SocketAddr, target: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let request = format!("GET {target} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).unwrap();
    stream.flush().unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let response = String::from_utf8_lossy(&raw).into_owned();
    let code = response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("no status line in response to {target}: {response:?}"));
    (code, response)
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

// The boundary between the two halves of a token-gated server that also serves
// the chat UI. Static files sit outside the gate on purpose: a browser has to
// load the page before it can present a token, and the bundle is public source
// either way. The API stays inside it. Until these tests existed that split
// held by inspection of one comment in api.rs, which is one innocent-looking
// refactor away from not holding.

/// The page and the bundle load with no credentials at all. If they did not,
/// a token-protected server would be a blank screen and nothing else.
#[tokio::test]
async fn static_files_load_without_credentials() {
    let dir = tempfile::tempdir().unwrap();
    ui_tree(dir.path());
    let addr = spawn_with_ui(Some(TOKEN), dir.path().to_path_buf()).await;

    let (root, bundle) = tokio::task::spawn_blocking(move || {
        (
            get(&format!("http://{addr}/"), None),
            get(&format!("http://{addr}/dist/main.js"), None),
        )
    })
    .await
    .unwrap();

    assert_eq!(root.0, 200, "GET / was refused: {}", root.1);
    assert!(
        root.1.contains("<title>zorp</title>"),
        "GET / did not return the page: {}",
        root.1
    );
    assert_eq!(bundle.0, 200, "GET /dist/main.js was refused: {}", bundle.1);
    assert!(
        bundle.1.contains("console.log"),
        "GET /dist/main.js did not return the bundle: {}",
        bundle.1
    );
}

/// Mounting the UI must not take the API out of the gate with it. This is the
/// half that matters: a reachable ungated API is agent-driven shell access to
/// the machine.
#[tokio::test]
async fn the_api_stays_gated_with_a_ui_mounted() {
    let dir = tempfile::tempdir().unwrap();
    ui_tree(dir.path());
    let addr = spawn_with_ui(Some(TOKEN), dir.path().to_path_buf()).await;

    let (health, sessions) = tokio::task::spawn_blocking(move || {
        (
            get(&format!("http://{addr}/api/health"), None),
            get(&format!("http://{addr}/api/sessions"), None),
        )
    })
    .await
    .unwrap();

    assert_eq!(health.0, 401, "GET /api/health was not gated: {}", health.1);
    assert_eq!(
        health.1, REFUSED,
        "the 401 on /api/health did not come from the token middleware"
    );
    assert_eq!(
        sessions.0, 401,
        "GET /api/sessions was not gated: {}",
        sessions.1
    );
    assert_eq!(
        sessions.1, REFUSED,
        "the 401 on /api/sessions did not come from the token middleware"
    );
}

/// The gate opens for the right token, UI or no UI. A gate that refused
/// everything would satisfy every assertion above and be useless.
#[tokio::test]
async fn the_token_still_opens_the_api_with_a_ui_mounted() {
    let dir = tempfile::tempdir().unwrap();
    ui_tree(dir.path());
    let addr = spawn_with_ui(Some(TOKEN), dir.path().to_path_buf()).await;

    let (code, body) = tokio::task::spawn_blocking(move || {
        get(
            &format!("http://{addr}/api/health"),
            Some(&format!("Bearer {TOKEN}")),
        )
    })
    .await
    .unwrap();

    assert_eq!(code, 200, "the right token was refused: {body}");
    assert!(body.contains("\"status\":\"ok\""), "got: {body}");
}

/// Serving static files outside the gate must not turn into serving the whole
/// filesystem outside the gate. Both shapes are covered: escaping straight
/// from the root, and escaping from inside the directory that really exists.
#[tokio::test]
async fn a_path_escaping_the_ui_directory_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    ui_tree(dir.path());
    let addr = spawn_with_ui(Some(TOKEN), dir.path().to_path_buf()).await;

    let responses = tokio::task::spawn_blocking(move || {
        [
            raw_get(addr, "/../../../etc/passwd"),
            raw_get(addr, "/dist/../../../etc/passwd"),
        ]
    })
    .await
    .unwrap();

    for (target, (code, response)) in ["/../../../etc/passwd", "/dist/../../../etc/passwd"]
        .iter()
        .zip(responses)
    {
        assert_eq!(code, 404, "{target} was not refused: {response}");
        assert!(
            !response.contains("root:"),
            "{target} served a password file: {response}"
        );
    }
}
