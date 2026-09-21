//! Developer Mode pretraining and local MLX supervision API.

use axum::extract::{Path as AxumPath, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use zorp_train::environment::TrainingEnvironment;
use zorp_train::manifest::{TokenizerConfig, TrainingJobConfig};
use zorp_train::recipe::{calculate_parameters, default_qwen_recipe};
use zorp_train::registry::ModelRegistry;
use zorp_train::supervisor::TrainingSupervisor;

pub struct DevState {
    pub env: TrainingEnvironment,
    pub supervisor: TrainingSupervisor,
    pub registry: ModelRegistry,
}

impl Default for DevState {
    fn default() -> Self {
        let env_dir = TrainingEnvironment::default_dir();
        let models_dir = env_dir
            .parent()
            .unwrap_or(&env_dir)
            .join("training")
            .join("models");
        Self {
            env: TrainingEnvironment::new(env_dir),
            supervisor: TrainingSupervisor::new(),
            registry: ModelRegistry::new(models_dir),
        }
    }
}

pub fn router<S>(state: Arc<DevState>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/status", get(get_status))
        .route("/environment/setup", post(setup_environment))
        .route("/recipes", get(get_recipes))
        .route("/tokenizer/train", post(train_tokenizer))
        .route("/tokenizer/inspect", post(inspect_tokenizer))
        .route("/train/start", post(start_train))
        .route("/train/pause", post(pause_train))
        .route("/train/resume", post(resume_train))
        .route("/train/stop", post(stop_train))
        .route("/models", get(list_models))
        .route("/models/:id/serve", post(serve_model))
        .route("/train/stream", get(train_stream))
        .with_state(state)
}

async fn get_status(State(state): State<Arc<DevState>>) -> Json<serde_json::Value> {
    let s = state.env.status();
    Json(serde_json::json!({
        "environment_status": s,
        "python_path": state.env.python_path(),
    }))
}

async fn setup_environment(State(state): State<Arc<DevState>>) -> Json<serde_json::Value> {
    let res = tokio::task::spawn_blocking(move || state.env.bootstrap())
        .await
        .unwrap_or_else(|e| Err(format!("task join error: {e}")));
    match res {
        Ok(()) => Json(serde_json::json!({ "status": "ok" })),
        Err(e) => Json(serde_json::json!({ "status": "error", "error": e })),
    }
}

async fn get_recipes() -> Json<serde_json::Value> {
    let qwen = default_qwen_recipe();
    let breakdown = calculate_parameters(&qwen);
    Json(serde_json::json!({
        "recipes": [qwen],
        "default_breakdown": breakdown,
    }))
}

async fn inspect_tokenizer(
    State(state): State<Arc<DevState>>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let text = body.get("text").and_then(|v| v.as_str()).unwrap_or("");
    let path = PathBuf::from(
        body.get("tokenizer_dir")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
    );
    match zorp_train::tokenizer::inspect_tokens(&state.env, &path, text) {
        Ok(res) => Json(serde_json::json!({ "result": res })),
        Err(e) => Json(serde_json::json!({ "error": e })),
    }
}

async fn train_tokenizer(
    State(state): State<Arc<DevState>>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let dataset = PathBuf::from(
        body.get("dataset_path")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
    );
    let output = PathBuf::from(
        body.get("output_dir")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
    );
    let config: TokenizerConfig = match body.get("config") {
        Some(c) => match serde_json::from_value(c.clone()) {
            Ok(cfg) => cfg,
            Err(e) => {
                return Json(
                    serde_json::json!({ "status": "error", "error": format!("invalid tokenizer config: {e}") }),
                )
            }
        },
        None => TokenizerConfig {
            name: "default".to_string(),
            vocab_size: 4096,
            special_tokens: vec![
                "<|endoftext|>".to_string(),
                "<|im_start|>".to_string(),
                "<|im_end|>".to_string(),
                "<|pad|>".to_string(),
            ],
        },
    };
    let res = tokio::task::spawn_blocking(move || {
        zorp_train::tokenizer::train_tokenizer(&state.env, &config, &dataset, &output)
    })
    .await
    .unwrap_or_else(|e| Err(format!("task join error: {e}")));
    match res {
        Ok(()) => Json(serde_json::json!({ "status": "ok" })),
        Err(e) => Json(serde_json::json!({ "status": "error", "error": e })),
    }
}

async fn start_train(
    State(state): State<Arc<DevState>>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let run_dir = PathBuf::from(
        body.get("run_dir")
            .and_then(|v| v.as_str())
            .unwrap_or(".zorp/training/run"),
    );
    let config: TrainingJobConfig = match body.get("config") {
        Some(c) => match serde_json::from_value(c.clone()) {
            Ok(cfg) => cfg,
            Err(e) => {
                return Json(
                    serde_json::json!({ "status": "error", "error": format!("invalid training job config: {e}") }),
                )
            }
        },
        None => return Json(serde_json::json!({ "status": "error", "error": "missing config" })),
    };
    let recipe = body
        .get("recipe")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    match state
        .supervisor
        .start_job(&state.env, &config, &run_dir, recipe)
        .await
    {
        Ok(_) => Json(serde_json::json!({ "status": "ok" })),
        Err(e) => Json(serde_json::json!({ "status": "error", "error": e })),
    }
}

async fn pause_train(State(state): State<Arc<DevState>>) -> Json<serde_json::Value> {
    match state.supervisor.pause() {
        Ok(()) => Json(serde_json::json!({ "status": "ok" })),
        Err(e) => Json(serde_json::json!({ "status": "error", "error": e })),
    }
}

async fn resume_train(State(state): State<Arc<DevState>>) -> Json<serde_json::Value> {
    match state.supervisor.resume() {
        Ok(()) => Json(serde_json::json!({ "status": "ok" })),
        Err(e) => Json(serde_json::json!({ "status": "error", "error": e })),
    }
}

async fn stop_train(State(state): State<Arc<DevState>>) -> Json<serde_json::Value> {
    match state.supervisor.stop() {
        Ok(()) => Json(serde_json::json!({ "status": "ok" })),
        Err(e) => Json(serde_json::json!({ "status": "error", "error": e })),
    }
}

async fn list_models(State(state): State<Arc<DevState>>) -> Json<serde_json::Value> {
    let models = state.registry.list_checkpoints();
    Json(serde_json::json!({ "models": models }))
}

/// What a request naming a checkpoint the registry never listed gets.
const NO_SUCH_CHECKPOINT: &str = "no such checkpoint in the registry";

/// Serve a checkpoint the registry listed, and only one it listed.
///
/// The id is resolved by looking it up in the listing rather than by
/// joining it onto the models directory. Serving starts a Python process
/// pointed at the directory, so a joined id would let `../` walk out of
/// the models directory and an id taken as a path would let an absolute
/// one skip it altogether, which is a request choosing what this machine
/// runs inference over.
async fn serve_model(
    State(state): State<Arc<DevState>>,
    AxumPath(id): AxumPath<String>,
) -> Json<serde_json::Value> {
    let Some(p) = state.registry.resolve_checkpoint(&id) else {
        return Json(serde_json::json!({ "status": "error", "error": NO_SUCH_CHECKPOINT }));
    };
    match state.registry.serve_checkpoint(&state.env, &p).await {
        Ok(port) => Json(serde_json::json!({
            "status": "ok",
            "port": port,
            "base_url": format!("http://127.0.0.1:{port}/v1")
        })),
        Err(e) => Json(serde_json::json!({ "status": "error", "error": e })),
    }
}

async fn train_stream(
    State(state): State<Arc<DevState>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = state.supervisor.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|res| {
        res.ok().and_then(|event| {
            serde_json::to_string(&event)
                .ok()
                .map(|data| Ok(Event::default().data(data)))
        })
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
