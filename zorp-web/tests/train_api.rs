//! Developer Mode API endpoints integration tests.

use std::net::SocketAddr;
use std::sync::Arc;
use tempfile::TempDir;
use zorp_train::environment::TrainingEnvironment;
use zorp_train::registry::ModelRegistry;
use zorp_train::supervisor::TrainingSupervisor;
use zorp_web::state::AppState;
use zorp_web::train::DevState;

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
    let (status, body) = tokio::task::spawn_blocking(move || {
        get(&format!("http://{addr}/api/dev/status"))
    })
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
    let (status, body) = tokio::task::spawn_blocking(move || {
        get(&format!("http://{addr}/api/dev/recipes"))
    })
    .await
    .unwrap();

    assert_eq!(status, 200, "recipes endpoint returned {status}: {body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let recipes = v.get("recipes").and_then(|r| r.as_array()).expect("recipes array");
    assert!(!recipes.is_empty());
    assert_eq!(recipes[0].get("family").and_then(|f| f.as_str()), Some("qwen-inspired"));
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
    let (status, body) = tokio::task::spawn_blocking(move || {
        get(&format!("http://{addr}/api/dev/models"))
    })
    .await
    .unwrap();

    assert_eq!(status, 200, "models endpoint returned {status}: {body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let models = v.get("models").and_then(|m| m.as_array()).expect("models array");
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
    let resp = tokio::task::spawn_blocking(move || {
        ureq::get(&url).call().unwrap()
    })
    .await
    .unwrap();

    assert_eq!(resp.status(), 200);
    let content_type = resp.header("content-type").unwrap_or_default().to_string();
    assert!(content_type.starts_with("text/event-stream"), "expected SSE, got {content_type}");
}
