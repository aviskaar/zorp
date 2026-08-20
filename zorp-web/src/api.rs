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
///
/// They are served `Cache-Control: no-cache`, which means revalidate before
/// reusing rather than do not store. None of these names carry a content
/// hash, so a rebuilt `dist/main.js` arrives at the URL the browser already
/// has a copy of, and with no `Cache-Control` at all a browser is free to
/// guess a freshness lifetime from `Last-Modified`. It does, and the result
/// is a page assembled from two different builds: a stale bundle running
/// against fresh markup, throwing errors about elements that were removed,
/// with nothing in the server log to explain it. `ServeDir` already answers
/// 304 to a conditional request, so the cost of always asking is one small
/// request per file on a connection to the same machine.
pub fn router_with_ui(state: AppState, ui_dir: Option<std::path::PathBuf>) -> Router {
    let api = api_router(state);
    match ui_dir {
        Some(dir) => api.fallback_service(tower_http::set_header::SetResponseHeader::overriding(
            tower_http::services::ServeDir::new(dir),
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-cache"),
        )),
        None => api,
    }
}

fn api_router(state: AppState) -> Router {
    // The UI is a separate artifact by design and may be served from another
    // origin, including `null` when index.html is opened straight off disk.
    // The POSTs send application/json, which is not a simple request, so
    // preflight has to be answered too.
    //
    // Which origins, though, has to be named rather than assumed. This was
    // `allow_origin(Any)`, and on the ordinary loopback install there is no
    // token either, so the two together meant any page the user happened to
    // visit could `POST /turn` and drive an agent that runs commands on this
    // machine, then read back what it produced. Nothing on the page would
    // show it happening.
    //
    // An empty list is the default and allows no cross-origin call at all.
    // That costs the normal install nothing: when this server serves the UI,
    // the page and the API share an origin and the browser runs no CORS
    // check. The container split names its origin with `--allow-origin`.
    //
    // The origins go in as one list rather than one call each. Passing a
    // single value sets a fixed `Access-Control-Allow-Origin` that goes out
    // whatever the request asked for, leaving the browser to notice the
    // mismatch; passing the list makes the server compare and answer only for
    // an origin actually on it. Repeated calls would also replace rather than
    // accumulate, so all but the last name would be quietly dropped.
    let allowed: Vec<axum::http::HeaderValue> = state
        .allowed_origins
        .iter()
        .filter_map(|origin| match origin.parse::<axum::http::HeaderValue>() {
            Ok(value) => Some(value),
            // A malformed origin is dropped rather than widening the list.
            // Failing open here would turn a typo into the hole above.
            Err(_) => {
                eprintln!("zorp-web: ignoring unparseable allowed origin {origin:?}");
                None
            }
        })
        .collect();
    let cors = CorsLayer::new()
        .allow_methods(Any)
        .allow_headers(Any)
        .allow_origin(allowed);

    Router::new()
        .route("/api/health", get(health))
        .route("/api/sessions", post(create_session).get(list_sessions))
        .route("/api/sessions/:id", get(get_session))
        .route("/api/sessions/:id/turn", post(start_turn))
        .route("/api/sessions/:id/stop", post(stop_turn))
        .route("/api/sessions/:id/events", get(stream_events))
        .route("/api/sessions/:id/approve", post(approve))
        .route(
            "/api/sessions/:id/auto-approve",
            get(get_auto_approve).post(set_auto_approve),
        )
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/settings/models", get(list_models))
        .route("/api/settings/test", post(test_connection))
        .route("/api/artifacts", get(list_artifacts))
        .route("/api/artifacts/raw", get(read_artifact))
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
                .map(|m| (&m.role, m.text()))
                // A turn where the model only called a tool has no text. The
                // browser draws tool activity from its own event kind, so
                // there is nothing here for it to render, and sending the row
                // anyway put a labelled bubble with an empty body into the
                // transcript every time the session was reopened.
                .filter(|(_, text)| !text.trim().is_empty())
                .map(|(role, text)| json!({"role": role, "content": text}))
                .collect();
            Json(json!({"messages": out})).into_response()
        }
        Err(e) => (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    }
}

/// A session's live state, adopting one the store knows about but this
/// process has not seen.
///
/// Live state is a process-local map and the store outlives the process, so
/// after a restart every session in the sidebar had no entry here. Both the
/// turn endpoint and the event stream answered "no such session", which meant
/// the transcript rendered perfectly and the composer was dead: the server
/// refusing to talk about the conversation it was visibly showing you.
///
/// Only sessions the store recognizes are adopted. An id nobody has heard of
/// still gets a 404, which matters most on the event stream: a browser stops
/// retrying on a 404 and an empty stream that ends at once is a reconnect
/// loop by another name.
fn session_or_adopt(
    state: &AppState,
    id: &str,
) -> Option<std::sync::Arc<std::sync::Mutex<crate::state::SessionState>>> {
    if let Some(session) = state.get(id) {
        return Some(session);
    }
    let stored = zorp_agent::Store::open_default()
        .ok()
        .and_then(|store| store.session_status(id).ok())
        .flatten()
        .is_some();
    stored.then(|| state.create(id))
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
    let Some(session) = session_or_adopt(&state, &id) else {
        return (StatusCode::NOT_FOUND, "no such session").into_response();
    };
    // Two turns on one agent would interleave into a corrupt transcript.
    if session.lock().unwrap().running {
        return (StatusCode::CONFLICT, "a turn is already running").into_response();
    }
    turn::spawn_turn(session, id, body.message, state.settings.clone());
    StatusCode::ACCEPTED.into_response()
}

/// Stop the turn that is running on this session.
///
/// 202 rather than 200: the stop is a request, not a completed act. The run
/// ends on its own thread a moment later, and the browser learns that it did
/// from the `stopped` and `done` events on the stream, the same place it
/// learns about every other way a turn can end. Answering 200 here would
/// invite a caller to treat the response as the end of the turn, and it is
/// not: an in-flight tool call still has to unwind first.
///
/// The 409 for a session with nothing running is not pedantry. The browser
/// only shows a stop control while it believes a turn is live, so a 409 means
/// its belief is stale, and it can use the answer to put itself back to idle
/// instead of waiting for a `done` that already came and went.
async fn stop_turn(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let Some(session) = state.get(&id) else {
        return (StatusCode::NOT_FOUND, "no such session").into_response();
    };
    if session.lock().unwrap().stop() {
        StatusCode::ACCEPTED.into_response()
    } else {
        (StatusCode::CONFLICT, "no turn is running").into_response()
    }
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

#[derive(Deserialize)]
struct AutoApproveBody {
    on: bool,
}

/// Stand this session's approvals down, or put them back up.
///
/// Off for every new session, on only because this request said so, and gone
/// when the session is. It is the same standing yes the CLI's `/approve`
/// command gives a chat session, with the difference that this one can be
/// taken back in the middle of a run: the flag is read at each approval rather
/// than baked into the agent when the turn started.
///
/// What it cannot do is widen the policy. `Policy::decide` runs first and its
/// `Deny` never reaches the approver at all, so the hard denylist refuses
/// exactly the same commands with this on as with it off. This changes who
/// answers the questions the policy asks, not which questions get asked.
///
/// A pending approval is deliberately left pending. Turning the mode on is not
/// a decision about the specific call already on the user's screen; the
/// browser sends that decision itself, as a separate and visible act.
///
/// Like every route here it sits behind the token gate, which means on
/// loopback it is reachable by anything already able to reach the API. That
/// grants nothing new: the same caller can `POST .../turn` and drive the agent
/// directly, which is strictly more than standing one session's approvals
/// down. The narrower worry is the agent turning this on for itself with an
/// approved `run_command`, and the answer is that it cannot do it unseen. The
/// approval card shows the whole command, and the moment the flag flips the
/// page it flipped is wearing a red banner nobody asked for.
async fn set_auto_approve(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<AutoApproveBody>,
) -> impl IntoResponse {
    let Some(session) = state.get(&id) else {
        return (StatusCode::NOT_FOUND, "no such session").into_response();
    };
    let flag = session.lock().unwrap().auto_approve.clone();
    flag.store(body.on, std::sync::atomic::Ordering::SeqCst);
    Json(json!({"auto_approve": body.on})).into_response()
}

/// What this session is currently doing about approvals.
///
/// The browser asks on every reconnect and every session switch, because a
/// mode that is on has to be visible on the page and a reloaded tab knows
/// nothing until it asks.
async fn get_auto_approve(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(session) = state.get(&id) else {
        return (StatusCode::NOT_FOUND, "no such session").into_response();
    };
    let on = session
        .lock()
        .unwrap()
        .auto_approve
        .load(std::sync::atomic::Ordering::SeqCst);
    Json(json!({"auto_approve": on})).into_response()
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
    // once is a reconnect loop by another name. Say plainly that there is
    // nothing to stream: a browser stops retrying on a 404. A session the
    // store knows about is not unknown, only unopened, so it is adopted.
    let Some(session) = session_or_adopt(&state, &id) else {
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

/// Check that an endpoint answers at all. Reuses the models probe: a 2xx
/// JSON response to `/models` is good enough evidence that the base URL is a
/// real, reachable OpenAI-compatible server without spending a real
/// completion call (and its tokens) just to say so.
///
/// A body of `{"base_url": "..."}` tests that candidate and stores nothing.
/// Without it, the saved configuration is tested instead, which is what a
/// bare `curl -X POST` gets. The candidate form exists because the panel's
/// Test button otherwise had to save the form before it could test it, so a
/// button that reads like a question overwrote the stored config to ask it,
/// and an address that turned out to be wrong took the working one with it.
///
/// The body is read as bytes and parsed here rather than through a `Json`
/// extractor because an absent body is the normal case, not a rejection.
async fn test_connection(
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> Json<serde_json::Value> {
    let candidate = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| {
            v.get("base_url")
                .and_then(|u| u.as_str())
                .map(str::to_string)
        })
        .filter(|u| !u.trim().is_empty());

    let base_url = match candidate {
        Some(url) => url,
        None => {
            let resolved = state.settings.lock().unwrap().resolve();
            if !resolved.configured {
                return Json(json!({"ok": false, "reason": "no model is configured yet"}));
            }
            resolved.base_url
        }
    };
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

#[derive(Deserialize)]
struct ArtifactQuery {
    path: Option<String>,
}

/// List the files under the workspace that `read_artifact` would serve.
///
/// Empty rather than an error when no workspace is configured: a UI asking
/// "what is there" and being told "nothing" is a working answer, while a 500
/// would make the pane look broken on a server that simply has the feature
/// switched off.
async fn list_artifacts(State(state): State<AppState>) -> Json<serde_json::Value> {
    let Some(root) = state.workspace.clone() else {
        return Json(json!({"files": [], "truncated": false}));
    };
    let listing = tokio::task::spawn_blocking(move || crate::artifacts::list(&root))
        .await
        .unwrap_or(crate::artifacts::Listing {
            files: Vec::new(),
            truncated: false,
        });
    Json(serde_json::to_value(listing).unwrap_or_default())
}

/// Serve one file, by a path relative to the workspace root.
///
/// The headers are not decoration. `nosniff` stops the browser
/// second-guessing the declared type, and `sandbox` means a served file
/// cannot reach the rest of this origin even if the browser does decide to
/// execute something in it, which is what makes it safe to drop a PDF into
/// an iframe here. See `crate::artifacts` for the traversal rules.
async fn read_artifact(
    State(state): State<AppState>,
    Query(query): Query<ArtifactQuery>,
) -> axum::response::Response {
    use crate::artifacts::{self, Refusal};

    let Some(root) = state.workspace.clone() else {
        return (
            StatusCode::NOT_FOUND,
            "this server was not started with a workspace",
        )
            .into_response();
    };
    let requested = query.path.unwrap_or_default();
    if requested.is_empty() {
        return (StatusCode::BAD_REQUEST, "no path given").into_response();
    }

    let resolved = tokio::task::spawn_blocking(move || artifacts::resolve(&root, &requested)).await;
    let path = match resolved {
        Ok(Ok(p)) => p,
        Ok(Err(Refusal::Outside)) => {
            return (
                StatusCode::FORBIDDEN,
                "that path is outside this server's workspace",
            )
                .into_response()
        }
        Ok(Err(Refusal::Missing)) => {
            return (StatusCode::NOT_FOUND, "no such file in this workspace").into_response()
        }
        Ok(Err(Refusal::UnsupportedType)) => {
            return (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "this endpoint does not serve that kind of file",
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("internal error: {e}"),
            )
                .into_response()
        }
    };

    // Size is checked before reading, not after, so an enormous file never
    // makes it into memory at all. What counts as enormous depends on what
    // the file is for: text is rendered by the page and a picture is not.
    let served = artifacts::served_as(&path).unwrap_or(artifacts::Served::Text);
    let mime = artifacts::content_type(&path).unwrap_or("application/octet-stream");
    let cap = match served {
        artifacts::Served::Text => artifacts::MAX_TEXT_BYTES,
        artifacts::Served::Document(_) => artifacts::MAX_DOCUMENT_BYTES,
        artifacts::Served::Image | artifacts::Served::Sandboxed => artifacts::MAX_BINARY_BYTES,
    };
    match std::fs::metadata(&path) {
        Ok(m) if m.len() > cap => {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                format!(
                    "that file is {} bytes, over the {cap} this pane will render",
                    m.len()
                ),
            )
                .into_response()
        }
        Ok(_) => {}
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    }

    let body = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };

    // An office file is a zip of XML and a PDF is a list of glyph placements.
    // Neither is something the browser will show, and neither is something
    // this server wants to hand a browser to interpret, so both are read here
    // and only their text goes out. See `crate::documents` for the caps that
    // make reading an archive safe and `crate::pdf` for why a PDF is read at
    // all rather than framed.
    //
    // Both readers run on a blocking thread. Reading is slow enough to matter
    // on a long document, and both are parsers pointed at a file a model
    // wrote or downloaded, so a panic in one has to end that request and
    // nothing else. `spawn_blocking` gives both: a panicking task comes back
    // as a join error rather than taking the server with it.
    let body = match served {
        artifacts::Served::Document(kind) => {
            let read = tokio::task::spawn_blocking(move || match kind {
                artifacts::Extraction::Office(kind) => {
                    crate::documents::to_markdown(kind, &body).map_err(|e| e.to_string())
                }
                artifacts::Extraction::Pdf => {
                    crate::pdf::to_markdown(&body).map_err(|e| e.to_string())
                }
            })
            .await;
            match read {
                Ok(Ok(markdown)) => markdown.into_bytes(),
                // A file that is not really the format its name claims is an
                // ordinary outcome when a model wrote it, so it gets a sentence
                // rather than a 500 or a blank pane.
                Ok(Err(why)) => {
                    return (StatusCode::UNPROCESSABLE_ENTITY, why).into_response();
                }
                Err(_) => {
                    return (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "reading this document crashed the reader, so there is nothing to show",
                    )
                        .into_response();
                }
            }
        }
        _ => body,
    };

    let mut headers = HeaderMap::new();
    headers.insert("content-type", mime.parse().unwrap());
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    // A bare `sandbox`, with no `allow-` token. That is what makes it safe to
    // point an iframe at a PDF, an SVG or an HTML file this server did not
    // write: the document loads into a unique origin with scripting off, so
    // script inside it neither runs nor has a handle on the page that framed
    // it. Every token added here gives some of that back.
    headers.insert("content-security-policy", "sandbox".parse().unwrap());
    // Inline, because the point is to show it in the pane. The sandbox above
    // is what makes that safe, not the disposition.
    headers.insert("content-disposition", "inline".parse().unwrap());
    (headers, body).into_response()
}
