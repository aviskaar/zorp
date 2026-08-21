//! `GET/PUT /api/settings`, `GET /api/settings/models`, `POST
//! /api/settings/test`.
//!
//! The precedence these enforce (a UI-saved setting beats the matching
//! `ZORP_*` env var, which beats the hardcoded default) is recorded in
//! `docs/DECISIONS.md`. See `zorp-web/src/settings.rs` for the resolution
//! logic these exercise from the outside.

mod common;

use std::net::SocketAddr;
use tokio::sync::Mutex;
use zorp_web::state::AppState;

/// Settings resolution reads real process env vars, and persistence reads
/// and writes a real file path (redirected per test via `ZORP_WEB_CONFIG`),
/// so tests that touch either cannot run concurrently with each other.
/// tokio's mutex, not std's, for the same reason `zorp-web/tests/turn.rs`
/// uses one: the guard is held across awaits on purpose, and tokio's does
/// not poison, so one failing test does not take the rest down with it.
static ENV: Mutex<()> = Mutex::const_new(());

/// Clears every model env var and points the settings file at a fresh temp
/// directory, so each test starts from a genuinely clean slate regardless of
/// what is exported in the shell running `cargo test` or left behind by a
/// previous test in this file.
struct Isolated {
    _dir: tempfile::TempDir,
}

impl Isolated {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ZORP_WEB_CONFIG", dir.path().join("web.toml"));
        for var in [
            "ZORP_PROVIDER",
            "ZORP_BASE_URL",
            "ZORP_MODEL",
            "ZORP_API_KEY",
            "ZORP_MAX_TOKENS",
        ] {
            std::env::remove_var(var);
        }
        Isolated { _dir: dir }
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

async fn get_async(url: String) -> (u16, String) {
    tokio::task::spawn_blocking(move || get(&url))
        .await
        .unwrap()
}

async fn put_async(url: String, body: String) -> (u16, String) {
    tokio::task::spawn_blocking(move || put(&url, &body))
        .await
        .unwrap()
}

#[tokio::test]
async fn clean_state_reports_defaults_and_not_configured() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let addr = spawn().await;

    let (status, body) = get_async(format!("http://{addr}/api/settings")).await;
    assert_eq!(status, 200, "body: {body}");
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");

    assert_eq!(json["configured"], false, "body: {body}");
    assert_eq!(json["provider"], "openai");
    assert_eq!(json["base_url"], "https://api.openai.com/v1");
    assert_eq!(json["model"], "gpt-4o");
    assert_eq!(json["has_api_key"], false);
    assert_eq!(json["provider_source"], "default");
    assert_eq!(json["base_url_source"], "default");
    assert_eq!(json["model_source"], "default");
    assert_eq!(json["api_key_source"], "default");
}

#[tokio::test]
async fn put_then_get_round_trips() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let addr = spawn().await;

    let put_body =
        r#"{"provider":"openai","base_url":"http://localhost:11434/v1","model":"qwen3:4b"}"#;
    let (put_status, put_resp) =
        put_async(format!("http://{addr}/api/settings"), put_body.to_string()).await;
    assert_eq!(put_status, 200, "body: {put_resp}");

    let (status, body) = get_async(format!("http://{addr}/api/settings")).await;
    assert_eq!(status, 200, "body: {body}");
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");

    assert_eq!(json["provider"], "openai");
    assert_eq!(json["base_url"], "http://localhost:11434/v1");
    assert_eq!(json["model"], "qwen3:4b");
    assert_eq!(json["base_url_source"], "ui");
    assert_eq!(json["model_source"], "ui");
    assert_eq!(json["configured"], true, "body: {body}");
}

#[tokio::test]
async fn ui_setting_overrides_matching_env_var() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    std::env::set_var("ZORP_MODEL", "env-model");
    let addr = spawn().await;

    // Before any PUT, the env var alone beats the hardcoded default.
    let (_, body) = get_async(format!("http://{addr}/api/settings")).await;
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert_eq!(json["model"], "env-model", "body: {body}");
    assert_eq!(json["model_source"], "env", "body: {body}");

    let (put_status, put_resp) = put_async(
        format!("http://{addr}/api/settings"),
        r#"{"model":"ui-model"}"#.to_string(),
    )
    .await;
    assert_eq!(put_status, 200, "body: {put_resp}");

    let (_, body) = get_async(format!("http://{addr}/api/settings")).await;
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert_eq!(
        json["model"], "ui-model",
        "the UI setting should have beaten the env var, body: {body}"
    );
    assert_eq!(json["model_source"], "ui", "body: {body}");

    std::env::remove_var("ZORP_MODEL");
}

#[tokio::test]
async fn api_key_never_appears_in_a_response_body() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let addr = spawn().await;
    let secret = "sk-super-secret-value-xyz";

    let put_body = format!(r#"{{"api_key":"{secret}"}}"#);
    let (put_status, put_resp) = put_async(format!("http://{addr}/api/settings"), put_body).await;
    assert_eq!(put_status, 200, "body: {put_resp}");
    assert!(
        !put_resp.contains(secret),
        "the PUT response leaked the key: {put_resp}"
    );

    let (get_status, get_resp) = get_async(format!("http://{addr}/api/settings")).await;
    assert_eq!(get_status, 200, "body: {get_resp}");
    assert!(
        !get_resp.contains(secret),
        "the GET response leaked the key: {get_resp}"
    );
    assert!(get_resp.contains("\"has_api_key\":true"), "got: {get_resp}");
}

#[tokio::test]
async fn persisted_file_is_written_without_the_api_key() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let config_path = zorp_web::settings::config_path();
    let addr = spawn().await;
    let secret = "sk-should-not-be-on-disk";

    let put_body = format!(
        r#"{{"base_url":"http://localhost:11434/v1","model":"qwen3:4b","api_key":"{secret}"}}"#
    );
    let (status, resp) = put_async(format!("http://{addr}/api/settings"), put_body).await;
    assert_eq!(status, 200, "body: {resp}");

    let on_disk = std::fs::read_to_string(&config_path)
        .unwrap_or_else(|e| panic!("expected {} to exist: {e}", config_path.display()));
    assert!(
        !on_disk.contains(secret),
        "the API key ended up on disk: {on_disk}"
    );
    assert!(
        !on_disk.to_lowercase().contains("api_key") && !on_disk.to_lowercase().contains("apikey"),
        "the file names a key field at all: {on_disk}"
    );
    assert!(
        on_disk.contains("qwen3:4b"),
        "the non-secret fields were not persisted: {on_disk}"
    );
}

#[tokio::test]
async fn unknown_provider_is_a_400_not_a_panic() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let addr = spawn().await;

    let (status, body) = put_async(
        format!("http://{addr}/api/settings"),
        r#"{"provider":"bedrock"}"#.to_string(),
    )
    .await;
    assert_eq!(status, 400, "body: {body}");
    assert!(
        body.to_lowercase().contains("provider"),
        "message should mention the provider: {body}"
    );

    // The server must still be alive: a panic in the handler would have
    // taken the whole task down with it.
    let (health_status, _) = get_async(format!("http://{addr}/api/health")).await;
    assert_eq!(health_status, 200);
}

#[tokio::test]
async fn model_listing_parses_the_openai_shape() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let base = common::mock_script(vec![r#"{"data":[{"id":"qwen3:4b"},{"id":"llama3"}]}"#]);
    let addr = spawn().await;

    let query = format!("http://{addr}/api/settings/models?base_url={base}");
    let (status, body) = get_async(query).await;
    assert_eq!(status, 200, "body: {body}");
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    let models: Vec<&str> = json["models"]
        .as_array()
        .unwrap_or_else(|| panic!("no models array in {body}"))
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(models, vec!["qwen3:4b", "llama3"], "body: {body}");
}

#[tokio::test]
async fn an_unreachable_base_url_yields_200_with_an_empty_list() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let addr = spawn().await;

    // Nothing listens on port 1: a refused connection fails fast instead of
    // hanging the test out to a timeout.
    let query = format!("http://{addr}/api/settings/models?base_url=http://127.0.0.1:1");
    let (status, body) = get_async(query).await;
    assert_eq!(status, 200, "an unreachable endpoint must not 500: {body}");
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert_eq!(json["models"], serde_json::json!([]), "body: {body}");
    assert!(
        json["error"].as_str().is_some_and(|s| !s.is_empty()),
        "should explain why, body: {body}"
    );
}

#[tokio::test]
async fn test_connection_reports_ok_against_a_reachable_endpoint() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let base = common::mock_script(vec![r#"{"data":[{"id":"m"}]}"#]);
    let addr = spawn().await;

    let (put_status, put_resp) = put_async(
        format!("http://{addr}/api/settings"),
        format!(r#"{{"base_url":"{base}","model":"m"}}"#),
    )
    .await;
    assert_eq!(put_status, 200, "body: {put_resp}");

    let (status, body) = tokio::task::spawn_blocking(move || {
        match ureq::post(&format!("http://{addr}/api/settings/test")).call() {
            Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
            Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
            Err(e) => panic!("{e}"),
        }
    })
    .await
    .unwrap();
    assert_eq!(status, 200, "body: {body}");
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert_eq!(json["ok"], true, "body: {body}");
}

/// Testing a candidate endpoint must not save it.
///
/// The panel's Test button used to save the form and then test what it had
/// just saved, because the endpoint only ever looked at stored state. That
/// makes a read-sounding verb overwrite `~/.config/zorp/web.toml`, so trying
/// an address that turns out to be wrong destroys the working one that was
/// there. A body on the POST now names the candidate, and nothing about it
/// is stored.
#[tokio::test]
async fn testing_a_candidate_endpoint_does_not_overwrite_the_saved_one() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let addr = spawn().await;

    let good = common::mock_script(vec![r#"{"data":[{"id":"qwen3:4b"}]}"#]);
    let (put_status, put_resp) = put_async(
        format!("http://{addr}/api/settings"),
        format!(r#"{{"base_url":"{good}","model":"qwen3:4b"}}"#),
    )
    .await;
    assert_eq!(put_status, 200, "body: {put_resp}");

    // Nothing listens here. This is the candidate a user types by mistake.
    let (status, body) = tokio::task::spawn_blocking(move || {
        match ureq::post(&format!("http://{addr}/api/settings/test"))
            .set("content-type", "application/json")
            .send_string(r#"{"base_url":"http://127.0.0.1:9/v1"}"#)
        {
            Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
            Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
            Err(e) => panic!("{e}"),
        }
    })
    .await
    .unwrap();
    assert_eq!(status, 200, "body: {body}");
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert_eq!(
        json["ok"], false,
        "the unreachable candidate was reported as working: {body}"
    );

    // The point of the test: the good URL is still what is saved.
    let (get_status, get_body) = get_async(format!("http://{addr}/api/settings")).await;
    assert_eq!(get_status, 200, "body: {get_body}");
    let saved: serde_json::Value = serde_json::from_str(&get_body).expect("valid JSON");
    assert_eq!(
        saved["base_url"], good,
        "testing a candidate overwrote the saved base URL: {get_body}"
    );
}

/// A POST with no body keeps testing whatever is saved, so `curl -X POST`
/// and any older client still work.
#[tokio::test]
async fn testing_with_no_body_still_checks_the_saved_endpoint() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let addr = spawn().await;

    let good = common::mock_script(vec![r#"{"data":[{"id":"qwen3:4b"}]}"#]);
    let (put_status, put_resp) = put_async(
        format!("http://{addr}/api/settings"),
        format!(r#"{{"base_url":"{good}","model":"qwen3:4b"}}"#),
    )
    .await;
    assert_eq!(put_status, 200, "body: {put_resp}");

    let (status, body) = tokio::task::spawn_blocking(move || {
        match ureq::post(&format!("http://{addr}/api/settings/test")).call() {
            Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
            Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
            Err(e) => panic!("{e}"),
        }
    })
    .await
    .unwrap();
    assert_eq!(status, 200, "body: {body}");
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert_eq!(json["ok"], true, "body: {body}");
}

/// Test Connection used to answer `ok` for any key, including none.
///
/// It probed `GET /models` with no Authorization header at all, and a
/// provider whose listing is public (OpenRouter's is) answers 200 to an
/// anonymous request. So the one button whose whole job is "are these
/// settings right" could not fail on the most common way for them to be
/// wrong. Verified against the real endpoint with a deliberately invalid
/// key before this was changed: `{"ok": true}`.
///
/// The probe now makes a real, minimal completion, because that is the
/// only request that exercises what the button claims to check: the key,
/// the address, the model name, and the provider all at once.
#[tokio::test]
async fn test_connection_sends_the_api_key() {
    let _guard = ENV.lock().await;
    let _iso = Isolated::new();
    let (base, requests) = common::mock_capture(
        200,
        "application/json",
        r#"{"choices":[{"message":{"role":"assistant","content":"hi"}}]}"#,
    );
    let addr = spawn().await;

    let body = format!(
        r#"{{"base_url":"{base}","model":"m","api_key":"sk-secret-probe","provider":"openai"}}"#
    );
    let _ = tokio::task::spawn_blocking(move || {
        ureq::post(&format!("http://{addr}/api/settings/test"))
            .set("content-type", "application/json")
            .send_string(&body)
            .map(|r| r.into_string().unwrap_or_default())
            .unwrap_or_else(|e| e.to_string())
    })
    .await
    .unwrap();

    let seen = requests
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the probe never reached the upstream");
    assert!(
        seen.contains("sk-secret-probe"),
        "the probe did not send the api key, so it cannot tell a good one from a bad one:\n{seen}"
    );
    assert!(
        seen.to_lowercase().contains("authorization: bearer"),
        "the key was not sent as a bearer token:\n{seen}"
    );
    // The endpoint matters as much as the header. A listing endpoint that
    // answers anonymously (OpenRouter's does) cannot validate a key no
    // matter what is sent with it, so the probe has to call the thing it
    // is claiming to have tested.
    assert!(
        seen.starts_with("POST /chat/completions"),
        "the probe hit a listing endpoint instead of the completion one, \
         so a public listing would still report success:\n{seen}"
    );
}

/// The other half, and the one that actually bit: an upstream that rejects
/// the credentials has to turn into `ok: false`. A probe that reports
/// success on a 401 is worse than no probe, because it tells the operator
/// the thing they just got wrong is right.
#[tokio::test]
async fn test_connection_fails_when_the_upstream_rejects_the_key() {
    let _guard = ENV.lock().await;
    let _iso = Isolated::new();
    let (base, _requests) = common::mock_capture(
        401,
        "application/json",
        r#"{"error":{"message":"invalid api key"}}"#,
    );
    let addr = spawn().await;

    let body =
        format!(r#"{{"base_url":"{base}","model":"m","api_key":"wrong","provider":"openai"}}"#);
    let answer = tokio::task::spawn_blocking(move || {
        ureq::post(&format!("http://{addr}/api/settings/test"))
            .set("content-type", "application/json")
            .send_string(&body)
            .map(|r| r.into_string().unwrap_or_default())
            .unwrap_or_else(|e| e.to_string())
    })
    .await
    .unwrap();

    assert!(
        answer.contains("\"ok\":false"),
        "a 401 from the model endpoint was reported as a working connection: {answer}"
    );
    assert!(
        answer.contains("401"),
        "the failure did not say what went wrong: {answer}"
    );
}
