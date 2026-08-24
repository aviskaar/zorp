//! Browser voice routes, present in every build and enabled by `voice`.

use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
#[cfg(feature = "voice")]
use std::process::Child;

#[cfg(feature = "voice")]
pub trait VoiceBootstrap: Send + Sync {
    fn start(
        &self,
        client: &zorp_voice::QwenAsr,
        progress: &mut dyn FnMut(zorp_voice::SetupProgress),
    ) -> Result<Option<Child>, zorp_voice::SetupError>;
}

#[cfg(feature = "voice")]
struct LocalVoiceBootstrap;

#[cfg(feature = "voice")]
impl VoiceBootstrap for LocalVoiceBootstrap {
    fn start(
        &self,
        client: &zorp_voice::QwenAsr,
        progress: &mut dyn FnMut(zorp_voice::SetupProgress),
    ) -> Result<Option<Child>, zorp_voice::SetupError> {
        match client.ensure_runtime(progress)? {
            zorp_voice::BootstrapOutcome::Ready => Ok(None),
            zorp_voice::BootstrapOutcome::Started(child) => Ok(Some(child)),
        }
    }
}

#[cfg(feature = "voice")]
#[derive(Default)]
pub struct VoiceRuntime {
    process: Option<ManagedRuntime>,
}

#[cfg(feature = "voice")]
struct ManagedRuntime {
    endpoint: String,
    model: String,
    child: Child,
}

#[cfg(feature = "voice")]
impl VoiceRuntime {
    fn running_for(&mut self, client: &zorp_voice::QwenAsr) -> Result<bool, String> {
        let Some(process) = &mut self.process else {
            return Ok(false);
        };
        match process.child.try_wait() {
            Ok(None) => {
                if process.endpoint == client.endpoint() && process.model == client.model() {
                    Ok(true)
                } else {
                    Err("zorp-web already owns a voice runtime for a different configured endpoint or model".into())
                }
            }
            Ok(Some(_)) | Err(_) => {
                self.process = None;
                Ok(false)
            }
        }
    }

    fn set(&mut self, client: &zorp_voice::QwenAsr, child: Child) {
        self.process = Some(ManagedRuntime {
            endpoint: client.endpoint().to_string(),
            model: client.model().to_string(),
            child,
        });
    }

    fn exited(&mut self) -> bool {
        let Some(process) = &mut self.process else {
            return false;
        };
        match process.child.try_wait() {
            Ok(None) => false,
            Ok(Some(_)) | Err(_) => {
                self.process = None;
                true
            }
        }
    }
}

#[cfg(feature = "voice")]
impl Drop for VoiceRuntime {
    fn drop(&mut self) {
        if let Some(process) = &mut self.process {
            let _ = process.child.kill();
            let _ = process.child.wait();
        }
    }
}

#[cfg(not(feature = "voice"))]
const ABSENT: &str = "this zorp-web was built without the voice feature, so it cannot transcribe audio. Rebuild it with --features voice.";

pub(crate) async fn status_value() -> serde_json::Value {
    #[cfg(not(feature = "voice"))]
    {
        json!({
            "available": false,
            "runtime_reachable": false,
            "model_present": false,
            "setup_available": false,
            "endpoint": null,
            "model": null,
            "stage": null,
            "command": null,
            "detail": ABSENT,
        })
    }
    #[cfg(feature = "voice")]
    {
        match zorp_voice::QwenAsr::from_env() {
            Err(error) => json!({
                "available": true,
                "runtime_reachable": false,
                "model_present": false,
                "endpoint": null,
                "model": null,
                "stage": null,
                "setup_available": false,
                "command": null,
                "detail": error.to_string(),
            }),
            Ok(client) => {
                let setup = zorp_voice::VoiceSetup::from_env(&client);
                let setup_available = setup.is_ok();
                let status = tokio::task::spawn_blocking(move || client.status())
                    .await
                    .expect("voice status does not panic");
                json!({
                    "available": true,
                    "runtime_reachable": status.runtime_reachable,
                    "model_present": status.model_present,
                    "setup_available": setup_available,
                    "endpoint": status.endpoint,
                    "model": status.model,
                    "stage": status.stage,
                    "command": null,
                    "detail": status.detail,
                })
            }
        }
    }
}

pub(crate) async fn status() -> Json<serde_json::Value> {
    Json(status_value().await)
}

#[cfg(not(feature = "voice"))]
pub(crate) async fn wait(_headers: HeaderMap) -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, ABSENT).into_response()
}

#[cfg(feature = "voice")]
pub(crate) async fn wait(
    axum::extract::State(state): axum::extract::State<crate::state::AppState>,
    headers: HeaderMap,
) -> axum::response::Response {
    use axum::response::sse::{Event, KeepAlive, Sse};
    use std::convert::Infallible;

    if !is_json(&headers) {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "voice readiness requests require Content-Type: application/json",
        )
            .into_response();
    }
    let client = match zorp_voice::QwenAsr::from_env() {
        Ok(client) => client,
        Err(error) => return (StatusCode::SERVICE_UNAVAILABLE, error.to_string()).into_response(),
    };
    let model = client.model().to_string();
    let runtime = state.voice_runtime.clone();
    let bootstrap: std::sync::Arc<dyn VoiceBootstrap> = state
        .voice_bootstrap
        .clone()
        .unwrap_or_else(|| std::sync::Arc::new(LocalVoiceBootstrap));
    let stream = async_stream::stream! {
        let mut runtime = runtime.lock().await;
        let checked = tokio::task::spawn_blocking(move || {
            let status = client.status();
            (client, status)
        }).await;
        let (client, initial) = match checked {
            Ok(value) => value,
            Err(_) => {
                yield Ok::<Event, Infallible>(wait_event("error", "error", &model, "Voice readiness check crashed."));
                return;
            }
        };
        if initial.runtime_reachable && initial.model_present {
            yield Ok::<Event, Infallible>(wait_event("ready", "ready", &model, "Voice input is ready."));
            return;
        }
        if initial.stage == Some(zorp_voice::SetupStage::Error) {
            yield Ok::<Event, Infallible>(wait_event("error", "error", &model, &initial.detail));
            return;
        }
        let running = match runtime.running_for(&client) {
            Ok(running) => running,
            Err(detail) => {
                eprintln!("zorp-web: {detail}");
                yield Ok::<Event, Infallible>(wait_event("error", "error", &model, "A different zorp-managed voice runtime is already running."));
                return;
            }
        };
        if !initial.runtime_reachable && !running {
            let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
            let bootstrap_client = client;
            let mut task = tokio::task::spawn_blocking(move || {
                bootstrap.start(&bootstrap_client, &mut |progress| {
                    let _ = progress_tx.send(progress);
                })
            });
            let outcome = loop {
                tokio::select! {
                    biased;
                    Some(progress) = progress_rx.recv() => {
                        let stage = stage_name(progress.stage);
                        yield Ok::<Event, Infallible>(wait_event("waiting", stage, &model, stage_detail(progress.stage)));
                    }
                    result = &mut task => break result,
                }
            };
            match outcome {
                Ok(Ok(Some(child))) => {
                    let owner = match zorp_voice::QwenAsr::from_env() {
                        Ok(client) => client,
                        Err(error) => {
                            eprintln!("zorp-web: voice configuration changed during setup: {error}");
                            yield Ok::<Event, Infallible>(wait_event("error", "error", &model, "Voice configuration changed during setup."));
                            return;
                        }
                    };
                    runtime.set(&owner, child);
                }
                Ok(Ok(None)) => {}
                Ok(Err(error)) => {
                    eprintln!("zorp-web: automatic voice setup failed: {error}");
                    yield Ok::<Event, Infallible>(wait_event("error", "error", &model, &error.to_string()));
                    return;
                }
                Err(error) => {
                    eprintln!("zorp-web: automatic voice setup crashed: {error}");
                    yield Ok::<Event, Infallible>(wait_event("error", "error", &model, &error.to_string()));
                    return;
                }
            }
        } else if initial.runtime_reachable
            && initial.stage == Some(zorp_voice::SetupStage::Ready)
        {
            yield Ok::<Event, Infallible>(wait_event("error", "error", &model, "A local runtime is answering, but it is not serving the configured Qwen3-ASR model."));
            return;
        }

        let mut client = match zorp_voice::QwenAsr::from_env() {
            Ok(client) => client,
            Err(error) => {
                eprintln!("zorp-web: voice configuration changed while waiting: {error}");
                yield Ok::<Event, Infallible>(wait_event("error", "error", &model, "Voice configuration changed while waiting for the model."));
                return;
            }
        };
        loop {
            if runtime.exited() {
                yield Ok::<Event, Infallible>(wait_event("error", "error", &model, "The local Qwen3-ASR runtime exited before the model became ready."));
                break;
            }
            let checked = tokio::task::spawn_blocking(move || {
                let status = client.status();
                (client, status)
            }).await;
            let (returned, status) = match checked {
                Ok(value) => value,
                Err(_) => {
                    yield Ok::<Event, Infallible>(wait_event("error", "error", &model, "Voice readiness check crashed."));
                    break;
                }
            };
            client = returned;
            if status.runtime_reachable && status.model_present {
                yield Ok::<Event, Infallible>(wait_event("ready", "ready", &model, "Voice input is ready."));
                break;
            }
            if status.stage == Some(zorp_voice::SetupStage::Error) {
                yield Ok::<Event, Infallible>(wait_event("error", "error", &model, &status.detail));
                break;
            }
            if status.runtime_reachable && status.stage == Some(zorp_voice::SetupStage::Ready) {
                yield Ok::<Event, Infallible>(wait_event("error", "error", &model, "The local runtime is not serving the configured Qwen3-ASR model."));
                break;
            }
            if let Some(stage) = status.stage {
                yield Ok::<Event, Infallible>(wait_event("waiting", stage_name(stage), &model, stage_detail(stage)));
            } else if status.runtime_reachable {
                yield Ok::<Event, Infallible>(wait_event("waiting", "loading", &model, stage_detail(zorp_voice::SetupStage::Loading)));
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    };
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
        .into_response()
}

#[cfg(feature = "voice")]
fn wait_event(status: &str, stage: &str, model: &str, detail: &str) -> axum::response::sse::Event {
    axum::response::sse::Event::default()
        .event("voice_model")
        .json_data(json!({"status": status, "stage": stage, "model": model, "detail": detail}))
        .expect("voice status events serialize")
}

#[cfg(feature = "voice")]
fn stage_name(stage: zorp_voice::SetupStage) -> &'static str {
    match stage {
        zorp_voice::SetupStage::CreatingEnvironment => "creating_environment",
        zorp_voice::SetupStage::Installing => "installing",
        zorp_voice::SetupStage::DownloadingModel => "downloading_model",
        zorp_voice::SetupStage::Loading => "loading",
        zorp_voice::SetupStage::Ready => "ready",
        zorp_voice::SetupStage::Error => "error",
    }
}

#[cfg(feature = "voice")]
fn stage_detail(stage: zorp_voice::SetupStage) -> &'static str {
    match stage {
        zorp_voice::SetupStage::CreatingEnvironment => "Creating a private voice environment.",
        zorp_voice::SetupStage::Installing => "Installing the pinned local voice runtime.",
        zorp_voice::SetupStage::DownloadingModel => "Downloading the local Qwen3-ASR model.",
        zorp_voice::SetupStage::Loading => "Loading the local Qwen3-ASR model.",
        zorp_voice::SetupStage::Ready => "Voice input is ready.",
        zorp_voice::SetupStage::Error => "The local Qwen3-ASR runtime could not load the model.",
    }
}

#[cfg(feature = "voice")]
fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
}

#[cfg(not(feature = "voice"))]
pub(crate) async fn transcribe(_headers: HeaderMap, _audio: Bytes) -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, ABSENT).into_response()
}

#[cfg(feature = "voice")]
pub(crate) async fn transcribe(headers: HeaderMap, audio: Bytes) -> axum::response::Response {
    let media_type = match headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
    {
        Some(value) => value.to_string(),
        None => {
            return (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "missing audio content type",
            )
                .into_response();
        }
    };
    if audio.is_empty() {
        return (StatusCode::BAD_REQUEST, "the recording was empty").into_response();
    }
    let client = match zorp_voice::QwenAsr::from_env() {
        Ok(client) => client,
        Err(error) => return (StatusCode::SERVICE_UNAVAILABLE, error.to_string()).into_response(),
    };
    let result = tokio::task::spawn_blocking(move || client.transcribe(&audio, &media_type)).await;
    match result {
        Ok(Ok(transcript)) => Json(transcript).into_response(),
        Ok(Err(error)) => voice_failure(error).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "voice transcription crashed".to_string(),
        )
            .into_response(),
    }
}

#[cfg(feature = "voice")]
fn voice_failure(error: zorp_voice::VoiceError) -> (StatusCode, String) {
    use zorp_voice::VoiceError;
    let status = match &error {
        VoiceError::UnsupportedMedia { .. } => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        VoiceError::Unreachable { .. }
        | VoiceError::OffDevice(_)
        | VoiceError::Redirected { .. } => StatusCode::SERVICE_UNAVAILABLE,
        VoiceError::Status { .. } | VoiceError::Malformed { .. } => StatusCode::BAD_GATEWAY,
        _ => StatusCode::BAD_GATEWAY,
    };
    (status, error.to_string())
}
