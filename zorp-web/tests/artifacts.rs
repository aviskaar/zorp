//! `GET /api/artifacts` and `GET /api/artifacts/raw`.
//!
//! These endpoints hand file contents to a browser, so the tests that matter
//! most are the ones that try to read something outside the workspace. See
//! `docs/superpowers/specs/2026-08-17-artifact-pane-design.md`.

use std::net::SocketAddr;
use std::path::Path;
use zorp_web::state::AppState;

/// A workspace with a couple of artifacts in it, plus a secret one directory
/// up that nothing served from the workspace should ever be able to reach.
struct Workspace {
    dir: tempfile::TempDir,
}

impl Workspace {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("workspace");
        std::fs::create_dir_all(root.join(".zorp/tracks/t1")).unwrap();
        std::fs::write(
            root.join(".zorp/tracks/t1/draft.md"),
            "# Draft\n\nLatency improved.\n",
        )
        .unwrap();
        std::fs::write(root.join("notes.md"), "hello\n").unwrap();
        std::fs::write(root.join("paper.pdf"), b"%PDF-1.4\nfake\n").unwrap();
        std::fs::write(root.join("secret.env"), "TOKEN=hunter2\n").unwrap();
        // The thing traversal would be after: outside the workspace root.
        std::fs::write(dir.path().join("outside.md"), "you should not see this\n").unwrap();
        Workspace { dir }
    }

    fn root(&self) -> std::path::PathBuf {
        self.dir.path().join("workspace")
    }
}

async fn spawn(root: &Path) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = AppState::with_token(None).with_workspace(root.to_path_buf());
    tokio::spawn(async move {
        axum::serve(listener, zorp_web::api::router_with_state(state))
            .await
            .unwrap();
    });
    addr
}

fn get(url: &str) -> (u16, String) {
    match ureq::AgentBuilder::new()
        .redirects(0)
        .build()
        .get(url)
        .call()
    {
        Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
        Err(e) => panic!("{e}"),
    }
}

async fn get_async(url: String) -> (u16, String) {
    tokio::task::spawn_blocking(move || get(&url))
        .await
        .unwrap()
}

/// The whole reason this endpoint needs care. A path that climbs out of the
/// workspace must be refused, and the refusal must not leak the contents on
/// the way out.
#[tokio::test]
async fn a_path_that_climbs_out_of_the_workspace_is_refused() {
    let ws = Workspace::new();
    let addr = spawn(&ws.root()).await;

    for attempt in [
        "../outside.md",
        "../../etc/passwd",
        ".zorp/../../outside.md",
        "/etc/passwd",
    ] {
        let encoded = attempt.replace("/", "%2F").replace("..", "%2E%2E");
        let (status, body) =
            get_async(format!("http://{addr}/api/artifacts/raw?path={encoded}")).await;
        assert!(
            status == 403 || status == 404,
            "{attempt} was not refused, got {status}: {body}"
        );
        assert!(
            !body.contains("you should not see this") && !body.contains("root:"),
            "{attempt} leaked contents: {body}"
        );
    }
}

/// An absolute path that happens to point inside the workspace is still not
/// how this endpoint is addressed. Everything is relative to the root.
#[tokio::test]
async fn an_ordinary_file_inside_the_workspace_is_served() {
    let ws = Workspace::new();
    let addr = spawn(&ws.root()).await;

    let (status, body) = get_async(format!(
        "http://{addr}/api/artifacts/raw?path=.zorp%2Ftracks%2Ft1%2Fdraft.md"
    ))
    .await;
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("Latency improved"), "body: {body}");
}

/// An extension allowlist, not a denylist. An unknown type served as
/// `text/html` would be a cross-site scripting hole, so unknown types are
/// refused rather than guessed at.
#[tokio::test]
async fn a_file_type_that_is_not_on_the_allowlist_is_refused() {
    let ws = Workspace::new();
    let addr = spawn(&ws.root()).await;

    let (status, body) =
        get_async(format!("http://{addr}/api/artifacts/raw?path=secret.env")).await;
    assert_eq!(status, 415, "body: {body}");
    assert!(
        !body.contains("hunter2"),
        "the refusal served the file anyway: {body}"
    );
}

/// A served file must not be able to act as a document in this origin. Both
/// headers matter: `nosniff` stops the browser second-guessing the declared
/// type, and the sandbox CSP stops a hostile PDF reaching the rest of the
/// page.
#[tokio::test]
async fn served_files_carry_the_headers_that_stop_them_becoming_active() {
    let ws = Workspace::new();
    let addr = spawn(&ws.root()).await;
    let url = format!("http://{addr}/api/artifacts/raw?path=paper.pdf");

    let (content_type, nosniff, csp) = tokio::task::spawn_blocking(move || {
        let r = ureq::get(&url).call().unwrap();
        (
            r.header("content-type").unwrap_or_default().to_string(),
            r.header("x-content-type-options")
                .unwrap_or_default()
                .to_string(),
            r.header("content-security-policy")
                .unwrap_or_default()
                .to_string(),
        )
    })
    .await
    .unwrap();

    assert_eq!(content_type, "application/pdf");
    assert_eq!(nosniff, "nosniff");
    assert!(csp.contains("sandbox"), "csp was {csp:?}");
}

/// The listing is what the pane's file list is built from. It finds the
/// artifacts and leaves out the noise.
#[tokio::test]
async fn the_listing_finds_artifacts_and_skips_the_noise() {
    let ws = Workspace::new();
    std::fs::create_dir_all(ws.root().join("target/debug")).unwrap();
    std::fs::write(ws.root().join("target/debug/junk.md"), "build output").unwrap();
    std::fs::create_dir_all(ws.root().join("node_modules/pkg")).unwrap();
    std::fs::write(ws.root().join("node_modules/pkg/readme.md"), "dep").unwrap();
    let addr = spawn(&ws.root()).await;

    let (status, body) = get_async(format!("http://{addr}/api/artifacts")).await;
    assert_eq!(status, 200, "body: {body}");
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    let paths: Vec<String> = json["files"]
        .as_array()
        .expect("files array")
        .iter()
        .map(|f| f["path"].as_str().unwrap_or_default().to_string())
        .collect();

    assert!(
        paths.iter().any(|p| p.contains("draft.md")),
        "draft.md missing from {paths:?}"
    );
    assert!(
        paths.iter().any(|p| p == "notes.md"),
        "notes.md missing from {paths:?}"
    );
    assert!(
        paths.iter().any(|p| p == "paper.pdf"),
        "paper.pdf missing from {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.starts_with("target/")),
        "build output was listed: {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.starts_with("node_modules/")),
        "dependencies were listed: {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.ends_with(".env")),
        "a non-artifact file was listed: {paths:?}"
    );
}

/// A file that vanished between listing and opening is a 404 the pane can
/// explain, not an empty body that looks like an empty file.
#[tokio::test]
async fn a_file_that_is_not_there_is_a_404_rather_than_an_empty_body() {
    let ws = Workspace::new();
    let addr = spawn(&ws.root()).await;

    let (status, body) = get_async(format!("http://{addr}/api/artifacts/raw?path=gone.md")).await;
    assert_eq!(status, 404, "body: {body}");
    assert!(!body.is_empty(), "a 404 with no explanation");
}

/// A symlink is the other way out of the workspace, and it is not caught by
/// looking for `..` in the path. The check has to happen after resolving.
#[cfg(unix)]
#[tokio::test]
async fn a_symlink_pointing_out_of_the_workspace_is_refused() {
    let ws = Workspace::new();
    std::os::unix::fs::symlink(
        ws.dir.path().join("outside.md"),
        ws.root().join("escape.md"),
    )
    .unwrap();
    let addr = spawn(&ws.root()).await;

    let (status, body) = get_async(format!("http://{addr}/api/artifacts/raw?path=escape.md")).await;
    assert!(
        status == 403 || status == 404,
        "a symlink out of the workspace was followed, got {status}: {body}"
    );
    assert!(
        !body.contains("you should not see this"),
        "the symlink leaked its target: {body}"
    );
}
