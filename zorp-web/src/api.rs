use crate::state::AppState;
use crate::turn;
use axum::extract::{Path, State};
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
    turn::spawn_turn(session, id, body.message);
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

/// Stream a session's events, replaying anything the client missed.
///
/// `Last-Event-ID` is set by the browser automatically on reconnect, so a
/// dropped connection resumes rather than losing the run.
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

    let session = state.get(&id);
    let stream = async_stream::stream! {
        let mut next = resume_from;
        loop {
            let (batch, finished) = match &session {
                Some(s) => {
                    let guard = s.lock().unwrap();
                    let batch: Vec<_> = guard
                        .backlog
                        .iter()
                        .filter(|e| e.seq >= next)
                        .cloned()
                        .collect();
                    (batch, guard.finished())
                }
                None => (Vec::new(), true),
            };
            for event in batch {
                next = event.seq + 1;
                let data = serde_json::to_string(&event).unwrap_or_default();
                yield Ok::<_, Infallible>(
                    SseEvent::default().id(event.seq.to_string()).data(data),
                );
            }
            if finished {
                break;
            }
            tokio::time::sleep(POLL).await;
        }
    };

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}
