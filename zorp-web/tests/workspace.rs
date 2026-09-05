//! `GET/PUT /api/workspace` and `GET /api/workspace/browse`.
//!
//! The server works in a directory somebody chose, and in no directory at
//! all until they have. These are the outside view of that: what the page
//! reads before anything is set, what a save does, which paths are refused,
//! and what a turn gets when there is nowhere to run it. See
//! `zorp-web/src/workspace.rs` for the rules themselves.

use std::net::SocketAddr;
use tokio::sync::Mutex;
use zorp_web::state::AppState;

/// The workspace resolves through process env vars and a real settings file,
/// so these tests take turns. tokio's mutex for the same reason the other
/// suites use one: it is held across awaits and it does not poison.
static ENV: Mutex<()> = Mutex::const_new(());

/// A clean slate: no named workspace anywhere, and a settings file in a
/// temp directory rather than the developer's own.
struct Isolated {
    dir: tempfile::TempDir,
}

impl Isolated {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ZORP_WEB_CONFIG", dir.path().join("web.toml"));
        std::env::remove_var("ZORP_WORKSPACE");
        Isolated { dir }
    }

    fn path(&self) -> &std::path::Path {
        self.dir.path()
    }
}

async fn spawn() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = AppState::with_token(None);
    tokio::spawn(async move {
        axum::serve(listener, zorp_web::api::router_with_state(state))
            .await
            .unwrap();
    });
    addr
}

fn get(url: &str) -> (u16, String) {
    match ureq::get(url).call() {
        Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
        Err(e) => panic!("{e}"),
    }
}

fn put(url: &str, body: &str) -> (u16, String) {
    match ureq::put(url)
        .set("content-type", "application/json")
        .send_string(body)
    {
        Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
        Err(e) => panic!("{e}"),
    }
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

fn json(body: &str) -> serde_json::Value {
    serde_json::from_str(body).unwrap()
}

/// A server nobody has given a directory to says so, plainly. The page reads
/// `configured` and opens its picker; nothing about the answer implies the
/// server picked somewhere on its own.
#[tokio::test]
async fn nothing_chosen_reports_nothing_chosen() {
    let _env = ENV.lock().await;
    let _isolated = Isolated::new();
    let addr = spawn().await;

    let (status, body) =
        tokio::task::spawn_blocking(move || get(&format!("http://{addr}/api/workspace")))
            .await
            .unwrap();

    assert_eq!(status, 200);
    let body = json(&body);
    assert_eq!(body["configured"], false);
    assert_eq!(body["source"], "none");
    assert!(body["path"].is_null(), "{body}");
    assert!(body["scratch"].is_null(), "{body}");
}

/// The whole point: choosing a directory in the browser, and the server
/// working in it afterwards. The scratch path comes back with it, because
/// that is where generated files go.
#[tokio::test]
async fn a_saved_workspace_comes_back_on_the_next_read() {
    let _env = ENV.lock().await;
    let isolated = Isolated::new();
    let work = isolated.path().join("research");
    std::fs::create_dir(&work).unwrap();
    let canonical = work.canonicalize().unwrap();
    let addr = spawn().await;

    let sent = format!(r#"{{"path":"{}"}}"#, work.display());
    let (status, body) =
        tokio::task::spawn_blocking(move || put(&format!("http://{addr}/api/workspace"), &sent))
            .await
            .unwrap();
    assert_eq!(status, 200, "{body}");
    let saved = json(&body);
    assert_eq!(saved["configured"], true);
    assert_eq!(saved["path"], canonical.display().to_string());

    let (status, body) =
        tokio::task::spawn_blocking(move || get(&format!("http://{addr}/api/workspace")))
            .await
            .unwrap();
    assert_eq!(status, 200);
    let body = json(&body);
    assert_eq!(body["configured"], true);
    assert_eq!(body["source"], "saved");
    assert_eq!(body["path"], canonical.display().to_string());
    assert_eq!(
        body["scratch"],
        canonical.join("scratch").display().to_string()
    );
}

/// Every refusal is a sentence somebody can act on, and none of the three
/// paths here is one the agent should ever be pointed at.
#[tokio::test]
async fn a_path_that_is_not_a_directory_is_refused_with_a_reason() {
    let _env = ENV.lock().await;
    let isolated = Isolated::new();
    let file = isolated.path().join("notes.md");
    std::fs::write(&file, "hi").unwrap();
    let missing = isolated.path().join("not-here");
    let addr = spawn().await;

    let cases = vec![
        ("relative/path".to_string(), "absolute"),
        (file.display().to_string(), "not a directory"),
        (missing.display().to_string(), "cannot be opened"),
    ];
    let refusals = tokio::task::spawn_blocking(move || {
        cases
            .into_iter()
            .map(|(path, expected)| {
                let sent = format!(r#"{{"path":"{path}"}}"#);
                let (status, body) = put(&format!("http://{addr}/api/workspace"), &sent);
                (path, expected, status, body)
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap();

    for (path, expected, status, body) in refusals {
        assert_eq!(status, 400, "{path} was accepted: {body}");
        assert!(
            body.contains(expected),
            "unhelpful refusal for {path}: {body}"
        );
    }

    // Nothing was stored on the way past.
    let (_, body) =
        tokio::task::spawn_blocking(move || get(&format!("http://{addr}/api/workspace")))
            .await
            .unwrap();
    assert_eq!(json(&body)["configured"], false);
}

/// The refusal the browser matches on. Status and text both, because the
/// page tells the two apart from a real failure by exactly this.
#[tokio::test]
async fn a_turn_with_no_workspace_is_refused_by_status_and_sentence() {
    let _env = ENV.lock().await;
    let _isolated = Isolated::new();
    let addr = spawn().await;

    let (status, body) = tokio::task::spawn_blocking(move || {
        let (_, created) = post(&format!("http://{addr}/api/sessions"), "{}");
        let id = json(&created)["id"].as_str().unwrap().to_string();
        post(
            &format!("http://{addr}/api/sessions/{id}/turn"),
            r#"{"message":"go"}"#,
        )
    })
    .await
    .unwrap();

    assert_eq!(status, 409);
    assert_eq!(body, "no workspace chosen");
}

/// The picker's list: directories, and only directories. A file in the same
/// place is not offered, and neither is a dotted directory unless the caller
/// asked for hidden ones.
#[tokio::test]
async fn browse_lists_directories_only() {
    let _env = ENV.lock().await;
    let isolated = Isolated::new();
    std::fs::create_dir(isolated.path().join("papers")).unwrap();
    std::fs::create_dir(isolated.path().join(".git")).unwrap();
    std::fs::write(isolated.path().join("notes.md"), "hi").unwrap();
    let root = isolated.path().to_path_buf();
    let addr = spawn().await;

    let (status, body) = tokio::task::spawn_blocking({
        let root = root.clone();
        move || {
            get(&format!(
                "http://{addr}/api/workspace/browse?path={}",
                root.display()
            ))
        }
    })
    .await
    .unwrap();
    assert_eq!(status, 200, "{body}");
    let body = json(&body);
    let names: Vec<&str> = body["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["papers"], "{body}");
    assert!(body["parent"].is_string(), "{body}");

    let (_, body) = tokio::task::spawn_blocking(move || {
        get(&format!(
            "http://{addr}/api/workspace/browse?path={}&hidden=1",
            root.display()
        ))
    })
    .await
    .unwrap();
    let body = json(&body);
    let names: Vec<&str> = body["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec![".git", "papers"], "{body}");
}

/// A relative path is the caller's mistake and a missing one is a stale
/// bookmark. They are different answers because the page says different
/// things about them.
#[tokio::test]
async fn browse_refuses_a_relative_path_and_reports_a_missing_one() {
    let _env = ENV.lock().await;
    let _isolated = Isolated::new();
    let addr = spawn().await;

    let (relative, missing) = tokio::task::spawn_blocking(move || {
        (
            get(&format!(
                "http://{addr}/api/workspace/browse?path=relative/dir"
            )),
            get(&format!(
                "http://{addr}/api/workspace/browse?path=/no/such/directory/anywhere"
            )),
        )
    })
    .await
    .unwrap();

    assert_eq!(relative.0, 400, "{}", relative.1);
    assert!(relative.1.contains("absolute"), "{}", relative.1);
    assert_eq!(missing.0, 404, "{}", missing.1);
}
