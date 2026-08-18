use crate::settings;
use crate::state::AppState;
use crate::turn;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event as SseEvent, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::convert::Infallible;
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};

/// How often the stream checks for new events. A chat UI does not need
/// sub-100ms latency, and polling a backlog is far less machinery than a
/// broadcast channel with its own lagging and reconnect semantics.
const POLL: Duration = Duration::from_millis(80);

pub fn router() -> Router {
    router_with_state(AppState::new())
}

pub fn router_with_state(state: AppState) -> Router {
    router_with_ui(state, None)
}

/// The API, optionally with the chat UI mounted underneath it.
///
/// The UI stays a separate artifact: the container split serves it from
/// nginx and `ui_dir` is then `None`. But an installed `zorp-web` has the
/// files sitting right there, and until this existed it served none of
/// them, so `zorp-web` followed by opening the printed URL gave a 404 on
/// every asset while `/api/health` cheerfully returned 200.
///
/// Static files are deliberately outside the token gate. The browser has to
/// load the page before it can present a token, and the bundle is public
/// source either way. The API keeps its gate.
pub fn router_with_ui(state: AppState, ui_dir: Option<std::path::PathBuf>) -> Router {
    let api = api_router(state);
    match ui_dir {
        Some(dir) => api.fallback_service(tower_http::services::ServeDir::new(dir)),
        None => api,
    }
}

fn api_router(state: AppState) -> Router {
    // The UI is a separate artifact by design and may be served from another
    // origin, including `null` when index.html is opened straight off disk.
    // The POSTs send application/json, which is not a simple request, so
    // preflight has to be answered too.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/api/health", get(health))
        .route("/api/sessions", post(create_session).get(list_sessions))
        .route("/api/sessions/:id", get(get_session))
        .route("/api/sessions/:id/turn", post(start_turn))
        .route("/api/sessions/:id/events", get(stream_events))
        .route("/api/sessions/:id/approve", post(approve))
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/settings/models", get(list_models))
        .route("/api/settings/test", post(test_connection))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::auth::require_token,
        ))
        .layer(cors)
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

async fn create_session(State(state): State<AppState>) -> Json<serde_json::Value> {
    let id = zorp_agent::new_session_id();
    state.create(&id);
    Json(json!({"id": id}))
}

/// Sessions come from the store, not from memory, so history survives a
/// server restart. In-memory sessions that have not recorded a message yet
/// are unioned in so a brand new chat appears in the sidebar immediately.
async fn list_sessions(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut rows: Vec<serde_json::Value> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Ok(store) = zorp_agent::Store::open_default() {
        if let Ok(sessions) = store.sessions() {
            for s in sessions {
                seen.insert(s.id.clone());
                rows.push(json!({"id": s.id, "title": s.task, "status": s.status}));
            }
        }
    }
    for id in state.ids() {
        if seen.insert(id.clone()) {
            rows.push(json!({"id": id, "title": "New chat", "status": "running"}));
        }
    }
    Json(json!(rows))
}

/// Replay a conversation from the store.
async fn get_session(Path(id): Path<String>) -> impl IntoResponse {
    let store = match zorp_agent::Store::open_default() {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match store.load_messages(&id) {
        Ok(messages) => {
            let out: Vec<serde_json::Value> = messages
                .iter()
                .filter(|m| m.role == "user" || m.role == "assistant")
                // Message content is structured to carry images; the browser
                // transcript wants the text of each turn.
                .map(|m| json!({"role": m.role, "content": m.text()}))
                .collect();
            Json(json!({"messages": out})).into_response()
        }
        Err(e) => (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct TurnBody {
    message: String,
}

async fn start_turn(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TurnBody>,
) -> impl IntoResponse {
    let Some(session) = state.get(&id) else {
        return (StatusCode::NOT_FOUND, "no such session").into_response();
    };
    // Two turns on one agent would interleave into a corrupt transcript.
    if session.lock().unwrap().running {
        return (StatusCode::CONFLICT, "a turn is already running").into_response();
    }
    turn::spawn_turn(session, id, body.message, state.settings.clone());
    StatusCode::ACCEPTED.into_response()
}

#[derive(Deserialize)]
struct ApproveBody {
    allow: bool,
}

async fn approve(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ApproveBody>,
) -> impl IntoResponse {
    let Some(session) = state.get(&id) else {
        return (StatusCode::NOT_FOUND, "no such session").into_response();
    };
    let approver = session.lock().unwrap().approver.clone();
    match approver {
        Some(a) if a.resolve(body.allow) => StatusCode::OK.into_response(),
        // Nothing was waiting. A stale click from a reloaded page looks like
        // this, and it is not an error worth failing the request over.
        _ => (StatusCode::CONFLICT, "nothing is awaiting approval").into_response(),
    }
}

/// Stream a session's events for as long as the browser is listening.
///
/// The stream is deliberately long lived. It belongs to the session, not to a
/// turn, so a finished turn does not end it: the next message streams down the
/// connection that is already open. Ending it per turn looked harmless and was
/// not, because `EventSource` reconnects on its own whenever the server ends
/// the response. An idle tab on a finished conversation opened a new
/// connection every few seconds forever and sat on a "reconnecting" badge with
/// nothing wrong.
///
/// `Last-Event-ID` is set by the browser automatically on a real reconnect, so
/// a connection that genuinely dropped resumes rather than losing the run.
async fn stream_events(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let resume_from = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .map(|seq| seq + 1)
        .unwrap_or(0);

    // An unknown session is not an empty stream. An empty stream that ends at
    // once is a reconnect loop by another name, which is what opening a stored
    // session from the sidebar used to be after a restart. Say plainly that
    // there is nothing to stream: a browser stops retrying on a 404.
    let Some(session) = state.get(&id) else {
        return (StatusCode::NOT_FOUND, "no such session").into_response();
    };

    let stream = async_stream::stream! {
        // The backlog only ever grows, so remembering how far along it we are
        // keeps each tick proportional to what is new rather than to the whole
        // conversation, which now matters: this loop runs for hours.
        let mut walked = 0usize;
        loop {
            let batch: Vec<_> = {
                let guard = session.lock().unwrap();
                let batch = guard.backlog[walked..].to_vec();
                walked = guard.backlog.len();
                batch
            };
            for event in batch {
                if event.seq < resume_from {
                    continue;
                }
                let data = serde_json::to_string(&event).unwrap_or_default();
                yield Ok::<_, Infallible>(
                    SseEvent::default().id(event.seq.to_string()).data(data),
                );
            }
            tokio::time::sleep(POLL).await;
        }
    };

    Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

/// The effective model configuration plus, for each field, where it came
/// from. `has_api_key` is the only thing said about the key itself: the
/// `Resolved` type this serializes has no field that could carry it.
async fn get_settings(State(state): State<AppState>) -> Json<serde_json::Value> {
    let resolved = state.settings.lock().unwrap().resolve();
    Json(serde_json::to_value(resolved).unwrap_or_default())
}

/// Validate and store a settings change. An unknown provider string is a 400
/// with a readable message, not a panic; everything else is accepted as
/// given. The updated, non-secret fields are persisted to disk immediately,
/// so a restart keeps whatever was last saved; a failure to write is logged
/// rather than failing the request, since the in-memory state (what the next
/// turn actually uses) is already correct either way.
async fn put_settings(
    State(state): State<AppState>,
    Json(body): Json<settings::PutSettings>,
) -> impl IntoResponse {
    let resolved = {
        let mut guard = state.settings.lock().unwrap();
        if let Err(message) = guard.apply(&body) {
            return (StatusCode::BAD_REQUEST, message).into_response();
        }
        let persisted = guard.to_persisted();
        if let Err(e) = settings::save(&persisted) {
            eprintln!(
                "zorp-web: could not persist settings to {}: {e}",
                settings::config_path().display()
            );
        }
        guard.resolve()
    };
    Json(serde_json::to_value(resolved).unwrap_or_default()).into_response()
}

#[derive(Deserialize)]
struct ModelsQuery {
    base_url: Option<String>,
}

/// Proxy `GET {base_url}/models`. Never a 500: an unreachable or non-JSON
/// endpoint is a normal, expected outcome for a settings panel probing
/// whatever the user just typed, and is reported as an empty list with a
/// reason instead. See `settings::fetch_models` for the SSRF-shape note.
async fn list_models(Query(query): Query<ModelsQuery>) -> Json<serde_json::Value> {
    let base_url = query.base_url.unwrap_or_default();
    let result = tokio::task::spawn_blocking(move || settings::fetch_models(&base_url))
        .await
        .unwrap_or_else(|e| settings::ModelsResult {
            models: Vec::new(),
            error: Some(format!("internal error: {e}")),
        });
    Json(json!({"models": result.models, "error": result.error}))
}

/// Check that the currently configured endpoint answers at all. Reuses the
/// models probe: a 2xx JSON response to `/models` is good enough evidence
/// that the base URL is a real, reachable OpenAI-compatible server without
/// spending a real completion call (and its tokens) just to say so.
async fn test_connection(State(state): State<AppState>) -> Json<serde_json::Value> {
    let resolved = state.settings.lock().unwrap().resolve();
    if !resolved.configured {
        return Json(json!({"ok": false, "reason": "no model is configured yet"}));
    }
    let base_url = resolved.base_url;
    let result = tokio::task::spawn_blocking(move || settings::fetch_models(&base_url))
        .await
        .unwrap_or_else(|e| settings::ModelsResult {
            models: Vec::new(),
            error: Some(format!("internal error: {e}")),
        });
    match result.error {
        None => Json(json!({"ok": true})),
        Some(reason) => Json(json!({"ok": false, "reason": reason})),
    }
}
