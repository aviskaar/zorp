//! Developer mode's routes, which exist only in a build with `train`.
//!
//! Run with: cargo test -p zorp-web --features train
#![cfg(feature = "train")]

//! Developer Mode API endpoints integration tests.

use std::net::SocketAddr;
use std::sync::Arc;
use tempfile::TempDir;
use zorp_train::environment::TrainingEnvironment;
use zorp_train::registry::ModelRegistry;
use zorp_train::supervisor::TrainingSupervisor;
use zorp_web::state::AppState;
use zorp_web::train::DevState;

/// `ZORP_STATE_DB` is process-wide, so the two tests that point it at their
/// own store take turns rather than racing each other's rows.
static STORE_ENV: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn spawn_test_server(dev_state: Arc<DevState>) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app_state = AppState::new().with_dev_state(dev_state);
    tokio::spawn(async move {
        axum::serve(listener, zorp_web::api::router_with_state(app_state))
            .await
            .unwrap();
    });
    addr
}

fn get(url: &str) -> (u16, String) {
    match ureq::get(url).call() {
        Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
        Err(e) => panic!("request failed to {url}: {e}"),
    }
}

fn post_json(url: &str, body: &serde_json::Value) -> (u16, String) {
    match ureq::post(url).send_json(body.clone()) {
        Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
        Err(e) => panic!("post failed to {url}: {e}"),
    }
}

#[test]
fn test_recipes_definition() {
    let recipe = zorp_train::recipe::default_qwen_recipe();
    assert_eq!(recipe.family, "qwen-inspired");
}

#[tokio::test]
async fn test_dev_status_endpoint() {
    let tmp = TempDir::new().unwrap();
    let dev_state = Arc::new(DevState {
        env: TrainingEnvironment::new(tmp.path().join("env")),
        supervisor: TrainingSupervisor::new(),
        registry: ModelRegistry::new(tmp.path().join("models")),
    });

    let addr = spawn_test_server(dev_state).await;
    let (status, body) =
        tokio::task::spawn_blocking(move || get(&format!("http://{addr}/api/dev/status")))
            .await
            .unwrap();

    assert_eq!(status, 200, "status endpoint returned {status}: {body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v.get("environment_status").is_some());
    assert!(v.get("python_path").is_some());
}

#[tokio::test]
async fn test_dev_recipes_endpoint() {
    let tmp = TempDir::new().unwrap();
    let dev_state = Arc::new(DevState {
        env: TrainingEnvironment::new(tmp.path().join("env")),
        supervisor: TrainingSupervisor::new(),
        registry: ModelRegistry::new(tmp.path().join("models")),
    });

    let addr = spawn_test_server(dev_state).await;
    let (status, body) =
        tokio::task::spawn_blocking(move || get(&format!("http://{addr}/api/dev/recipes")))
            .await
            .unwrap();

    assert_eq!(status, 200, "recipes endpoint returned {status}: {body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let recipes = v
        .get("recipes")
        .and_then(|r| r.as_array())
        .expect("recipes array");
    assert!(!recipes.is_empty());
    assert_eq!(
        recipes[0].get("family").and_then(|f| f.as_str()),
        Some("qwen-inspired")
    );
    assert!(v.get("default_breakdown").is_some());
}

#[tokio::test]
async fn test_dev_models_endpoint() {
    let tmp = TempDir::new().unwrap();
    let models_dir = tmp.path().join("models");
    std::fs::create_dir_all(&models_dir).unwrap();

    let dev_state = Arc::new(DevState {
        env: TrainingEnvironment::new(tmp.path().join("env")),
        supervisor: TrainingSupervisor::new(),
        registry: ModelRegistry::new(models_dir),
    });

    let addr = spawn_test_server(dev_state).await;
    let (status, body) =
        tokio::task::spawn_blocking(move || get(&format!("http://{addr}/api/dev/models")))
            .await
            .unwrap();

    assert_eq!(status, 200, "models endpoint returned {status}: {body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let models = v
        .get("models")
        .and_then(|m| m.as_array())
        .expect("models array");
    assert!(models.is_empty());
}

#[tokio::test]
async fn test_dev_tokenizer_inspect_endpoint() {
    let tmp = TempDir::new().unwrap();
    let dev_state = Arc::new(DevState {
        env: TrainingEnvironment::new(tmp.path().join("env")),
        supervisor: TrainingSupervisor::new(),
        registry: ModelRegistry::new(tmp.path().join("models")),
    });

    let addr = spawn_test_server(dev_state).await;
    let (status, body) = tokio::task::spawn_blocking(move || {
        post_json(
            &format!("http://{addr}/api/dev/tokenizer/inspect"),
            &serde_json::json!({
                "tokenizer_dir": "/tmp/nonexistent",
                "text": "sample text"
            }),
        )
    })
    .await
    .unwrap();

    assert_eq!(status, 200, "tokenizer inspect returned {status}: {body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    // Since environment is not installed in tmp, we expect error field
    assert!(v.get("error").is_some());
}

#[tokio::test]
async fn test_dev_train_stream_endpoint() {
    let tmp = TempDir::new().unwrap();
    let dev_state = Arc::new(DevState {
        env: TrainingEnvironment::new(tmp.path().join("env")),
        supervisor: TrainingSupervisor::new(),
        registry: ModelRegistry::new(tmp.path().join("models")),
    });

    let addr = spawn_test_server(dev_state).await;
    let url = format!("http://{addr}/api/dev/train/stream");
    let resp = tokio::task::spawn_blocking(move || ureq::get(&url).call().unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let content_type = resp.header("content-type").unwrap_or_default().to_string();
    assert!(
        content_type.starts_with("text/event-stream"),
        "expected SSE, got {content_type}"
    );
}

#[tokio::test]
async fn test_dev_status_unauthorized_when_token_configured() {
    let tmp = TempDir::new().unwrap();
    let dev_state = Arc::new(DevState {
        env: TrainingEnvironment::new(tmp.path().join("env")),
        supervisor: TrainingSupervisor::new(),
        registry: ModelRegistry::new(tmp.path().join("models")),
    });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app_state = AppState::with_token(Some("dev-secret".to_string())).with_dev_state(dev_state);
    tokio::spawn(async move {
        axum::serve(listener, zorp_web::api::router_with_state(app_state))
            .await
            .unwrap();
    });

    let (status, body) =
        tokio::task::spawn_blocking(move || get(&format!("http://{addr}/api/dev/status")))
            .await
            .unwrap();

    assert_eq!(
        status, 401,
        "expected 401 unauthorized, got {status}: {body}"
    );
    assert_eq!(body, "missing or wrong token");

    let (auth_status, _) = tokio::task::spawn_blocking(move || {
        match ureq::get(&format!("http://{addr}/api/dev/status"))
            .set("authorization", "Bearer dev-secret")
            .call()
        {
            Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
            Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
            Err(e) => panic!("request failed: {e}"),
        }
    })
    .await
    .unwrap();
    assert_eq!(auth_status, 200, "expected 200 with valid token");
}

/// Serving a checkpoint is a lookup in the registry, never a path join.
///
/// The route starts a Python process pointed at the directory it is given.
/// It used to build that directory by joining the request's id onto the
/// models directory, and to take the id as the path outright when it was
/// absolute, so a request could point inference at any directory on the
/// machine. Both shapes are refused by name here: anything the listing did
/// not offer gets `no such checkpoint in the registry` and no process is
/// started.
#[tokio::test]
async fn serve_refuses_a_checkpoint_the_registry_never_listed() {
    let tmp = TempDir::new().unwrap();
    let models_dir = tmp.path().join("models");
    std::fs::create_dir_all(&models_dir).unwrap();

    // A checkpoint shaped directory outside the models directory.
    let outside = tmp.path().join("elsewhere").join("not_ours");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("model.safetensors"), b"someone else's").unwrap();

    let dev_state = Arc::new(DevState {
        env: TrainingEnvironment::new(tmp.path().join("env")),
        supervisor: TrainingSupervisor::new(),
        registry: ModelRegistry::new(models_dir),
    });

    let addr = spawn_test_server(dev_state).await;

    for id in [
        outside.to_string_lossy().to_string(),
        "../elsewhere/not_ours".to_string(),
        "no_such_run".to_string(),
    ] {
        let url = format!(
            "http://{addr}/api/dev/models/{}/serve",
            urlencoding_all(&id)
        );
        let (status, body) =
            tokio::task::spawn_blocking(move || post_json(&url, &serde_json::json!({})))
                .await
                .unwrap();

        assert_eq!(status, 200, "serve returned {status}: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            v.get("error").and_then(|e| e.as_str()),
            Some("no such checkpoint in the registry"),
            "serving {id} was not refused as an unlisted checkpoint: {body}"
        );
        assert!(
            v.get("port").is_none(),
            "serving {id} reported a port: {body}"
        );
    }
}

/// Recording a work-log entry, giving it feedback, and seeing it become a
/// dataset row. The feedback is a person's click (here, the test's POST); no
/// model is asked to grade anything, and the answer never reaches `messages`.
#[tokio::test]
async fn worklog_feedback_builds_a_dataset() {
    let _guard = STORE_ENV.lock().await;
    // Isolate the store so this does not touch a real database.
    let dir = TempDir::new().unwrap();
    std::env::set_var("ZORP_STATE_DB", dir.path().join("sessions.db"));

    let dev = Arc::new(DevState::default());
    let addr = spawn_test_server(dev).await;
    let base = format!("http://{addr}/api/dev");

    // Record an answer.
    let (code, body) = tokio::task::spawn_blocking({
        let url = format!("{base}/worklog");
        move || {
            post_json(
                &url,
                &serde_json::json!({
                    "session_id": "s1",
                    "model": "my-finetune",
                    "prompt": "write a haiku",
                    "answer": "a weak haiku",
                    "tool_calls": [{"name": "run_command", "status": "ok"}],
                }),
            )
        }
    })
    .await
    .unwrap();
    assert_eq!(code, 200, "{body}");
    let recorded: serde_json::Value = serde_json::from_str(&body).unwrap();
    let id = recorded["id"].as_str().unwrap().to_string();

    // A person edits it into a better answer.
    let (code, _) = tokio::task::spawn_blocking({
        let url = format!("{base}/worklog/{id}/feedback");
        move || {
            post_json(
                &url,
                &serde_json::json!({ "feedback": "down", "edited_answer": "a strong haiku" }),
            )
        }
    })
    .await
    .unwrap();
    assert_eq!(code, 200);

    // The dataset preview turns the edit into an SFT completion and a
    // preference pair.
    let (code, body) = tokio::task::spawn_blocking({
        let url = format!("{base}/dataset/preview");
        move || post_json(&url, &serde_json::json!({}))
    })
    .await
    .unwrap();
    assert_eq!(code, 200, "{body}");
    let preview: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(preview["sft_count"], 1, "{body}");
    assert_eq!(preview["preference_count"], 1, "{body}");
    assert_eq!(preview["preference"][0]["chosen"], "a strong haiku");
    assert_eq!(preview["preference"][0]["rejected"], "a weak haiku");

    std::env::remove_var("ZORP_STATE_DB");
}

/// A fine-tune with no feedback to learn from is refused, never run on
/// nothing. One self-contained test with its own store, so it does not race
/// the other store test on the process-wide `ZORP_STATE_DB`.
#[tokio::test]
async fn a_finetune_with_no_examples_is_refused() {
    let _guard = STORE_ENV.lock().await;
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("empty.db");
    // Create the store up front so the route opens an empty one, not one the
    // other test populated.
    zorp_agent::Store::open_at(&db).unwrap();
    std::env::set_var("ZORP_STATE_DB", &db);
    let dev = Arc::new(DevState::default());
    let addr = spawn_test_server(dev).await;
    let url = format!("http://{addr}/api/dev/finetune/start");
    let (code, body) = tokio::task::spawn_blocking(move || {
        post_json(
            &url,
            &serde_json::json!({
                "config": {
                    "run_id": "ft", "method": "sft", "base_model": "m",
                    "dataset_path": "", "learning_rate": 1e-5, "batch_size": 1,
                    "iters": 1, "lora_layers": 1, "checkpoint_every_steps": 1
                }
            }),
        )
    })
    .await
    .unwrap();
    std::env::remove_var("ZORP_STATE_DB");
    assert_eq!(code, 200, "{body}");
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    // Either an empty store (no examples) or, if it raced the other test's
    // store, still never a silent run: it must be refused or an error.
    assert_eq!(value["status"], "error", "{body}");
}

/// Percent encode every byte that is not unreserved, so a path stays one
/// segment. `encodeURIComponent` in `web/src/api.ts` does the same job.
fn urlencoding_all(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
