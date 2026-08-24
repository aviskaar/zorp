//! Browser voice routes, present in every build and enabled by `voice`.

use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;

#[cfg(not(feature = "voice"))]
const ABSENT: &str = "this zorp-web was built without the voice feature, so it cannot transcribe audio. Rebuild it with --features voice.";

pub(crate) async fn status_value() -> serde_json::Value {
    #[cfg(not(feature = "voice"))]
    {
        json!({
            "available": false,
            "runtime_reachable": false,
            "model_present": false,
            "endpoint": null,
            "model": null,
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
                "command": null,
                "detail": error.to_string(),
            }),
            Ok(client) => {
                let command = client.start_command();
                let status = tokio::task::spawn_blocking(move || client.status())
                    .await
                    .expect("voice status does not panic");
                json!({
                    "available": true,
                    "runtime_reachable": status.runtime_reachable,
                    "model_present": status.model_present,
                    "endpoint": status.endpoint,
                    "model": status.model,
                    "command": command,
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
pub(crate) async fn wait(headers: HeaderMap) -> axum::response::Response {
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
    let stream = async_stream::stream! {
        let mut client = client;
        loop {
            let checked = tokio::task::spawn_blocking(move || {
                let status = client.status();
                (client, status)
            }).await;
            let (returned, status) = match checked {
                Ok(value) => value,
                Err(_) => {
                    yield Ok::<Event, Infallible>(wait_event("error", &model, "voice readiness check crashed"));
                    break;
                }
            };
            client = returned;
            if status.runtime_reachable && status.model_present {
                yield Ok::<Event, Infallible>(wait_event("ready", &model, &status.detail));
                break;
            }
            yield Ok::<Event, Infallible>(wait_event("waiting", &model, &status.detail));
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    };
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
        .into_response()
}

#[cfg(feature = "voice")]
fn wait_event(status: &str, model: &str, detail: &str) -> axum::response::sse::Event {
    axum::response::sse::Event::default()
        .event("voice_model")
        .json_data(json!({"status": status, "model": model, "detail": detail}))
        .expect("voice status events serialize")
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
