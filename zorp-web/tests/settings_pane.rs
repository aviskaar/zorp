//! The routes behind the settings pane.
//!
//! Four of them are new and three of those delete something. The tests
//! here are about the two ways this goes wrong rather than about the happy
//! paths: a secret leaving the process, and a delete taking more than it
//! was asked for.
//!
//! Every one of these actions is a person's. No tool calls any of them, and
//! `zorp-agent/src/agent.rs` has `no_tool_clears_state_or_resets_settings`
//! saying so from the other side.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;
use zorp_agent::{Message, Store};

/// The state paths are process wide, so these take turns.
static ENV: Mutex<()> = Mutex::const_new(());

async fn spawn() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, zorp_web::api::router())
            .await
            .unwrap();
    });
    addr
}

/// `ureq` blocks, and the server under test is on this runtime. Calling it
/// straight from an async test parks the executor and the request never
/// gets answered, so every call goes through `spawn_blocking`, the same as
/// `tests/skills.rs`.
async fn get(url: String) -> serde_json::Value {
    let body =
        tokio::task::spawn_blocking(move || ureq::get(&url).call().unwrap().into_string().unwrap())
            .await
            .unwrap();
    serde_json::from_str(&body).unwrap()
}

async fn put(url: String, body: &'static str) -> (u16, String) {
    tokio::task::spawn_blocking(move || {
        match ureq::put(&url)
            .set("content-type", "application/json")
            .send_string(body)
        {
            Ok(response) => {
                let status = response.status();
                (status, response.into_string().unwrap_or_default())
            }
            Err(ureq::Error::Status(code, response)) => {
                (code, response.into_string().unwrap_or_default())
            }
            Err(e) => panic!("{e}"),
        }
    })
    .await
    .unwrap()
}

/// The status as well as the body, for a route whose failure mode is a
/// status code with nothing in it.
async fn get_status(url: String) -> (u16, String) {
    tokio::task::spawn_blocking(move || match ureq::get(&url).call() {
        Ok(response) => {
            let status = response.status();
            (status, response.into_string().unwrap_or_default())
        }
        Err(ureq::Error::Status(code, response)) => {
            (code, response.into_string().unwrap_or_default())
        }
        Err(e) => panic!("{e}"),
    })
    .await
    .unwrap()
}

async fn delete(url: String) -> (u16, String) {
    tokio::task::spawn_blocking(move || match ureq::delete(&url).call() {
        Ok(response) => {
            let status = response.status();
            (status, response.into_string().unwrap_or_default())
        }
        Err(ureq::Error::Status(code, response)) => {
            (code, response.into_string().unwrap_or_default())
        }
        Err(e) => panic!("{e}"),
    })
    .await
    .unwrap()
}

struct Fixture {
    dir: tempfile::TempDir,
    addr: SocketAddr,
}

impl Fixture {
    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }
    fn path(&self, leaf: &str) -> PathBuf {
        self.dir.path().join(leaf)
    }
    fn write(&self, leaf: &str, body: &str) -> PathBuf {
        let path = self.path(leaf);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, body).unwrap();
        path
    }
}

async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZORP_STATE_DB", dir.path().join("sessions.db"));
    std::env::set_var("ZORP_RECALL_DB", dir.path().join("recall.db"));
    std::env::set_var("ZORP_HISTORY_FILE", dir.path().join("history"));
    std::env::set_var("ZORP_TRUST_FILE", dir.path().join("trust"));
    std::env::set_var("ZORP_CONFIG", dir.path().join("zorp.toml"));
    std::env::set_var("ZORP_WORKSPACE", dir.path());
    std::env::remove_var("ZORP_MCP_SERVERS");
    Fixture {
        dir,
        addr: spawn().await,
    }
}

fn seed_conversations(db: &Path, ids: &[&str]) {
    let mut store = Store::open_at(db).unwrap();
    store.create_project("p1", "research").unwrap();
    for id in ids {
        store.create_session(id, "task", "/repo", "m").unwrap();
        store.record_message(id, 0, &Message::user("hi")).unwrap();
        store.set_session_project(id, Some("p1")).unwrap();
    }
}

/* ------------------------------------------------------------------ */
/* what is held                                                        */
/* ------------------------------------------------------------------ */

#[tokio::test]
async fn the_data_listing_names_every_file_and_the_variable_that_moves_it() {
    let _env = ENV.lock().await;
    let fx = fixture().await;
    fx.write("recall.db", "index bytes");

    let body = get(fx.url("/api/data")).await;
    let files = body["files"].as_array().unwrap();
    assert_eq!(files.len(), 5, "{body}");

    let index = files.iter().find(|f| f["label"] == "search index").unwrap();
    assert_eq!(index["exists"], true);
    assert_eq!(index["bytes"], 11);
    assert_eq!(index["env_var"], "ZORP_RECALL_DB");
    assert!(!index["what"].as_str().unwrap().is_empty());

    // Absent and empty are different answers.
    let history = files
        .iter()
        .find(|f| f["label"] == "input history")
        .unwrap();
    assert_eq!(history["exists"], false);
    assert!(history["bytes"].is_null(), "{history}");

    // The sentence about the key is in front of whoever is about to reset.
    assert!(
        body["reset_note"]
            .as_str()
            .unwrap()
            .contains("ZORP_API_KEY"),
        "{body}"
    );
}

/* ------------------------------------------------------------------ */
/* clearing                                                            */
/* ------------------------------------------------------------------ */

#[tokio::test]
async fn clearing_conversations_takes_the_projects_with_them() {
    let _env = ENV.lock().await;
    let fx = fixture().await;
    seed_conversations(&fx.path("sessions.db"), &["s1", "s2"]);

    let (status, body) = delete(fx.url("/api/sessions")).await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("\"deleted_sessions\":2"), "{body}");

    let store = Store::open_at(&fx.path("sessions.db")).unwrap();
    assert!(store.sessions().unwrap().is_empty());
    assert!(store.projects().unwrap().is_empty());
}

/// The one that would be unforgivable. A settings reset that reached into
/// the conversations, or into a workspace, is the action nobody could undo.
#[tokio::test]
async fn resetting_settings_takes_the_settings_and_nothing_else() {
    let _env = ENV.lock().await;
    let fx = fixture().await;
    fx.write("zorp.toml", "model = \"local\"\n");
    fx.write("trust", "abc123\n");
    fx.write("history", "what was typed\n");
    fx.write("scratch/report.html", "<p>the person's own file</p>");
    seed_conversations(&fx.path("sessions.db"), &["s1"]);

    let (status, body) = delete(fx.url("/api/settings")).await;
    assert_eq!(status, 200, "{body}");

    assert!(!fx.path("zorp.toml").exists(), "the settings file survived");
    assert!(!fx.path("trust").exists(), "the trust file survived");
    assert!(fx.path("history").exists(), "reset took the input history");
    assert!(
        fx.path("scratch/report.html").exists(),
        "reset reached into the workspace"
    );
    let store = Store::open_at(&fx.path("sessions.db")).unwrap();
    assert_eq!(
        store.sessions().unwrap().len(),
        1,
        "reset took a conversation"
    );

    // And it says what it could not do, rather than implying the key is gone.
    let answered: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(
        answered["note"].as_str().unwrap().contains("cannot"),
        "{body}"
    );
}

#[tokio::test]
async fn deleting_the_search_index_leaves_the_conversations_it_was_built_from() {
    let _env = ENV.lock().await;
    let fx = fixture().await;
    fx.write("recall.db", "embeddings");
    seed_conversations(&fx.path("sessions.db"), &["s1"]);

    let (status, body) = delete(fx.url("/api/recall/index")).await;
    assert_eq!(status, 200, "{body}");
    assert!(!fx.path("recall.db").exists());
    let store = Store::open_at(&fx.path("sessions.db")).unwrap();
    assert_eq!(store.sessions().unwrap().len(), 1);
}

#[tokio::test]
async fn deleting_an_index_that_was_never_built_is_not_an_error() {
    let _env = ENV.lock().await;
    let fx = fixture().await;
    let (status, _) = delete(fx.url("/api/recall/index")).await;
    assert_eq!(status, 200);
}

/* ------------------------------------------------------------------ */
/* MCP                                                                 */
/* ------------------------------------------------------------------ */

/// The whole reason the listing is redacted. Both maps are where a token
/// goes, and a settings page is exactly where somebody would paste a
/// screenshot from.
#[tokio::test]
async fn the_mcp_listing_never_carries_an_env_or_header_value() {
    let _env = ENV.lock().await;
    let fx = fixture().await;
    fx.write(
        ".zorp/mcp.toml",
        r#"
[[server]]
name = "github"
transport = "streamable_http"
url = "https://api.example.com/mcp"
trust = "sandbox"
headers = { Authorization = "Bearer REALSECRET" }

[[server]]
name = "filesystem"
transport = "stdio"
command = "npx"
args = ["-y", "server-filesystem"]
trust = "sandbox"
env = { GITHUB_TOKEN = "ghp_REALSECRET" }
"#,
    );

    let body = get(fx.url("/api/mcp")).await;
    let text = body.to_string();
    assert!(!text.contains("REALSECRET"), "{text}");
    assert!(!text.contains("ghp_"), "{text}");
    assert!(!text.contains("Bearer"), "{text}");

    // The useful half survives.
    let servers = body["servers"].as_array().unwrap();
    assert_eq!(servers.len(), 2, "{body}");
    let fs = servers.iter().find(|s| s["name"] == "filesystem").unwrap();
    assert_eq!(fs["env_keys"][0], "GITHUB_TOKEN");
    assert_eq!(fs["command"], "npx");
    let gh = servers.iter().find(|s| s["name"] == "github").unwrap();
    assert_eq!(gh["header_keys"][0], "Authorization");
}

/// A listing that showed configured servers without saying this build loads
/// none would read as "these are working", which is the opposite of true.
#[tokio::test]
async fn the_mcp_listing_says_this_build_loads_none_of_them() {
    let _env = ENV.lock().await;
    let fx = fixture().await;
    fx.write(
        ".zorp/mcp.toml",
        "[[server]]\nname = \"fs\"\ntransport = \"stdio\"\ncommand = \"npx\"\ntrust = \"sandbox\"\n",
    );

    let body = get(fx.url("/api/mcp")).await;
    assert_eq!(body["loads_servers"], false, "{body}");
    assert_eq!(body["servers"][0]["loaded"], false, "{body}");
    assert!(
        body["why"].as_str().unwrap().contains("no MCP tools"),
        "{body}"
    );
    // And where it looked, so an empty list is explicable.
    assert!(body["sources"].as_array().unwrap().len() >= 2, "{body}");
}

/// A server that silently vanishes is a tool that silently stops existing,
/// so a file that does not parse is a warning and not an empty list.
#[tokio::test]
async fn a_broken_mcp_file_is_a_warning_rather_than_silence() {
    let _env = ENV.lock().await;
    let fx = fixture().await;
    fx.write(".zorp/mcp.toml", "[[server]\nname = broken");

    let body = get(fx.url("/api/mcp")).await;
    assert!(!body["warning"].is_null(), "{body}");
}

#[tokio::test]
async fn no_mcp_configuration_is_an_empty_list_and_not_an_error() {
    let _env = ENV.lock().await;
    let fx = fixture().await;
    let body = get(fx.url("/api/mcp")).await;
    assert!(body["servers"].as_array().unwrap().is_empty(), "{body}");
    assert!(body["warning"].is_null(), "{body}");
}

/* ------------------------------------------------------------------ */
/* doctor                                                              */
/* ------------------------------------------------------------------ */

/// The sibling of `the_api_key_is_never_in_the_output`, on the HTTP shape.
/// A report meant to be pasted into a bug report is the last place a
/// credential may appear.
#[tokio::test]
async fn the_doctor_report_never_carries_the_api_key() {
    let _env = ENV.lock().await;
    std::env::set_var("ZORP_API_KEY", "sk-REALSECRET-0123456789");
    let fx = fixture().await;

    let text = get(fx.url("/api/doctor")).await.to_string();
    std::env::remove_var("ZORP_API_KEY");

    assert!(!text.contains("REALSECRET"), "{text}");
    assert!(!text.contains("sk-"), "{text}");
    assert!(!text.contains("0123456789"), "{text}");
    // Set or not set, which is the whole contract.
    assert!(text.contains("api key"), "{text}");
}

/// `?probe=1` is the spelling this route's own "not checked" line tells a
/// reader to use, and the one in `docs/DECISIONS.md`.
///
/// It used to answer 400, because a `bool` field is `serde_urlencoded`'s
/// bool and that accepts `true` and `false` and nothing else. So the
/// documented way to ask for a probe was the one way that could not work,
/// and the failure was a status code with no body saying which parameter
/// it disliked.
#[tokio::test]
async fn the_probe_flag_accepts_the_spelling_the_docs_and_the_report_use() {
    let _env = ENV.lock().await;
    let fx = fixture().await;

    for query in ["?probe=1", "?probe=true", "?probe"] {
        let (status, _) = get_status(fx.url(&format!("/api/doctor{query}"))).await;
        assert_eq!(status, 200, "GET /api/doctor{query} was refused");
    }

    // And an unprobed report still says it did not probe rather than
    // guessing, which is the behaviour the opt in exists for.
    let body = get(fx.url("/api/doctor")).await;
    let endpoint = body["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["label"] == "endpoint reachable");
    if let Some(check) = endpoint {
        assert!(
            check["detail"]
                .as_str()
                .unwrap_or_default()
                .contains("not checked"),
            "{check}"
        );
    }
}

/// A key typed into the settings pane is a key that is set.
///
/// The check read `ZORP_API_KEY` and nothing else, so the pane somebody had
/// just finished configuring reported "not set" against a remote endpoint
/// and the whole report came back unhealthy. The report is the surface that
/// tells a person whether their configuration works, so a false negative
/// there sends them to fix something that is not broken.
#[tokio::test]
async fn a_key_configured_in_the_browser_reads_as_set() {
    let _env = ENV.lock().await;
    let previous = std::env::var("ZORP_API_KEY").ok();
    std::env::remove_var("ZORP_API_KEY");
    let fx = fixture().await;

    let (status, _) = put(
        fx.url("/api/settings"),
        r#"{"base_url":"https://api.openai.com/v1","model":"m","api_key":"sk-TYPED-IN-THE-PANE"}"#,
    )
    .await;
    assert_eq!(status, 200, "the settings write was refused");

    let body = get(fx.url("/api/doctor")).await;
    let text = body.to_string();
    if let Some(p) = previous {
        std::env::set_var("ZORP_API_KEY", p);
    }

    let key_check = body["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["label"] == "api key")
        .expect("the report has an api key line")
        .clone();

    assert_ne!(key_check["health"], "bad", "{key_check}");
    assert!(
        key_check["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("is set"),
        "{key_check}"
    );
    // And still nothing of the key itself, which is the older rule.
    assert!(!text.contains("TYPED-IN-THE-PANE"), "{text}");
    assert!(!text.contains("sk-"), "{text}");
}

#[tokio::test]
async fn the_doctor_report_says_what_the_build_has_and_where_its_state_is() {
    let _env = ENV.lock().await;
    let fx = fixture().await;

    let body = get(fx.url("/api/doctor")).await;
    let labels: Vec<&str> = body["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["label"].as_str().unwrap())
        .collect();
    for expected in [
        "features",
        "endpoint",
        "model",
        "api key",
        "conversations",
        "workspace",
    ] {
        assert!(
            labels.contains(&expected),
            "{expected} missing from {labels:?}"
        );
    }
    assert!(!body["version"].as_str().unwrap().is_empty(), "{body}");
}
