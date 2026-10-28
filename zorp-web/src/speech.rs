//! Browser text-to-speech routes, present in every build and enabled by the
//! `voice` feature (the same feature the speech client ships under).
//!
//! `GET /api/speech/status` says whether a local Ollama speech runtime is
//! reachable, whether the model is pulled, and the voices it offers.
//! `GET /api/speech/voices` lists them for a model. `POST /api/speech/chunks`
//! splits long text on sentence boundaries so the browser can start playback
//! on the first chunk, reusing the one chunker in `zorp-voice` rather than a
//! second copy in TypeScript. `POST /api/speech` synthesizes one chunk and
//! returns the audio.
//!
//! Every route exists in every build and answers 501 without the feature, the
//! way the voice routes do, so the page can say why speech is off. The text
//! never leaves loopback: that is the client's rule, enforced in `zorp-voice`
//! and proved by its canary tests.

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;

#[cfg(not(feature = "voice"))]
const ABSENT: &str = "this zorp-web was built without the voice feature, so it cannot speak. Rebuild it with --features voice.";

#[derive(serde::Deserialize)]
pub struct SpeakBody {
    pub text: String,
    /// Overrides the configured model and voice for this one request.
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub voice: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct ChunkBody {
    pub text: String,
}

#[derive(serde::Deserialize)]
pub struct VoicesQuery {
    #[serde(default)]
    pub model: Option<String>,
}

/// The capabilities view, shared with `/api/capabilities` so the two cannot
/// disagree about what the browser is told. Reads only; never speaks.
pub(crate) async fn status_value(settings: &crate::state::SettingsHandle) -> serde_json::Value {
    #[cfg(not(feature = "voice"))]
    {
        let _ = settings;
        json!({
            "available": false,
            "runtime_reachable": false,
            "model_present": false,
            "endpoint": null,
            "model": null,
            "voices": [],
            "detail": ABSENT,
        })
    }
    #[cfg(feature = "voice")]
    {
        let (model, voice) = {
            let guard = settings.lock().unwrap();
            (
                guard.speech_model_resolved().0,
                guard.speech_voice_resolved().0,
            )
        };
        match build_client(&model) {
            Err(error) => json!({
                "available": true,
                "runtime_reachable": false,
                "model_present": false,
                "endpoint": null,
                "model": model,
                "voice": voice,
                "voices": [],
                "detail": error.to_string(),
            }),
            Ok(client) => {
                let status = tokio::task::spawn_blocking(move || client.status())
                    .await
                    .expect("speech status task panicked");
                json!({
                    "available": true,
                    "runtime_reachable": status.runtime_reachable,
                    "model_present": status.model_present,
                    "endpoint": status.endpoint,
                    "model": status.model,
                    "voice": voice,
                    "voices": status.voices,
                    "detail": status.detail,
                })
            }
        }
    }
}

pub async fn status(
    axum::extract::State(state): axum::extract::State<crate::state::AppState>,
) -> Json<serde_json::Value> {
    Json(status_value(&state.settings).await)
}

#[cfg(not(feature = "voice"))]
pub async fn voices(
    axum::extract::State(_state): axum::extract::State<crate::state::AppState>,
    axum::extract::Query(_query): axum::extract::Query<VoicesQuery>,
) -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, ABSENT).into_response()
}

#[cfg(feature = "voice")]
pub async fn voices(
    axum::extract::State(state): axum::extract::State<crate::state::AppState>,
    axum::extract::Query(query): axum::extract::Query<VoicesQuery>,
) -> impl IntoResponse {
    let model = query
        .model
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| state.settings.lock().unwrap().speech_model_resolved().0);
    match build_client(&model) {
        Err(error) => speech_failure(error).into_response(),
        Ok(client) => {
            let model2 = model.clone();
            let result = tokio::task::spawn_blocking(move || client.voices(&model2))
                .await
                .expect("speech voices task panicked");
            match result {
                Ok(voices) => Json(json!({ "model": model, "voices": voices })).into_response(),
                Err(error) => speech_failure(error).into_response(),
            }
        }
    }
}

#[cfg(not(feature = "voice"))]
pub async fn chunks(Json(_body): Json<ChunkBody>) -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, ABSENT).into_response()
}

#[cfg(feature = "voice")]
pub async fn chunks(Json(body): Json<ChunkBody>) -> impl IntoResponse {
    let chunks = zorp_voice::chunk_for_speech(&body.text, zorp_voice::SPEECH_CHAR_LIMIT);
    Json(json!({ "chunks": chunks })).into_response()
}

#[cfg(not(feature = "voice"))]
pub async fn speak(
    axum::extract::State(_state): axum::extract::State<crate::state::AppState>,
    Json(_body): Json<SpeakBody>,
) -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, ABSENT).into_response()
}

#[cfg(feature = "voice")]
pub async fn speak(
    axum::extract::State(state): axum::extract::State<crate::state::AppState>,
    Json(body): Json<SpeakBody>,
) -> impl IntoResponse {
    if body.text.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "there was no text to speak").into_response();
    }
    let (model, voice) = {
        let guard = state.settings.lock().unwrap();
        (
            body.model
                .filter(|m| !m.trim().is_empty())
                .unwrap_or_else(|| guard.speech_model_resolved().0),
            body.voice
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| guard.speech_voice_resolved().0),
        )
    };
    let client = match build_client(&model) {
        Ok(client) => client,
        Err(error) => return speech_failure(error).into_response(),
    };
    let text = body.text;
    let (model2, voice2) = (model.clone(), voice.clone());
    let result = tokio::task::spawn_blocking(move || client.speak(&text, &model2, &voice2))
        .await
        .expect("speech synthesis task panicked");
    match result {
        Ok(audio) => (
            [(axum::http::header::CONTENT_TYPE, audio.media_type)],
            audio.bytes,
        )
            .into_response(),
        Err(error) => speech_failure(error).into_response(),
    }
}

/// Build a speech client from the resolved model and the endpoint from the
/// environment (or the loopback default). An off-device endpoint is refused
/// here before any request, by the client's own guard.
#[cfg(feature = "voice")]
fn build_client(model: &str) -> Result<zorp_voice::OllamaSpeech, zorp_voice::SpeechError> {
    let url = std::env::var(zorp_voice::SPEECH_URL_VAR)
        .ok()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| zorp_voice::DEFAULT_SPEECH_URL.to_string());
    zorp_voice::OllamaSpeech::at(&url, model)
}

#[cfg(feature = "voice")]
fn speech_failure(error: zorp_voice::SpeechError) -> (StatusCode, String) {
    use zorp_voice::SpeechError;
    let status = match error {
        SpeechError::OffDevice(_) => StatusCode::BAD_REQUEST,
        SpeechError::Empty => StatusCode::BAD_REQUEST,
        SpeechError::Unreachable { .. } | SpeechError::Redirected { .. } => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        SpeechError::Status { .. } | SpeechError::Malformed { .. } => StatusCode::BAD_GATEWAY,
        _ => StatusCode::BAD_GATEWAY,
    };
    (status, error.to_string())
}
