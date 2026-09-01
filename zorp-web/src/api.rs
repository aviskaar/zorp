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
        .route("/api/sessions/:id", get(get_session).delete(delete_session))
        .route("/api/sessions/:id/turn", post(start_turn))
        .route("/api/sessions/:id/stop", post(stop_turn))
        .route("/api/sessions/:id/panel", post(start_panel))
        .route("/api/panel/lenses", get(list_lenses))
        .route("/api/capabilities", get(capabilities))
        .route("/api/voice/status", get(crate::voice::status))
        .route("/api/voice/wait", post(crate::voice::wait))
        .route(
            "/api/voice/transcribe",
            post(crate::voice::transcribe)
                .layer(axum::extract::DefaultBodyLimit::max(25 * 1024 * 1024)),
        )
        .route("/api/sessions/:id/investigate", post(start_investigate))
        .route("/api/investigate/status", get(investigate_status))
        .route("/api/investigate/ledger", get(investigate_ledger))
        .route("/api/sessions/:id/events", get(stream_events))
        .route("/api/sessions/:id/approve", post(approve))
        .route(
            "/api/sessions/:id/auto-approve",
            get(get_auto_approve).post(set_auto_approve),
        )
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/settings/models", get(list_models).post(list_models))
        .route("/api/settings/test", post(test_connection))
        .route("/api/artifacts", get(list_artifacts))
        .route("/api/artifacts/raw", get(read_artifact))
        // Conversation search. The three routes exist in every build, so a
        // server without the `recall` feature answers "off, and here is
        // why" rather than a 404 the page has to interpret.
        .route("/api/recall/status", get(recall_status))
        .route("/api/recall/index", post(recall_index))
        .route("/api/recall/search", get(recall_search))
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

/// What this build can actually do, for a page that cannot see the build.
///
/// `web_search` is one capability. Three separate things decide whether that
/// tool exists, and the browser can observe none of them. Whether
/// `zorp-web` was compiled with the `search` feature is a fact about the
/// binary. Whether the policy permits the tool is a fact about the code the
/// binary runs. Whether the search provider found its key is a fact about
/// the environment the server was started in, and it can change without a
/// restart, so this is answered per request rather than at startup.
///
/// A separate route rather than another field on `/api/settings`, which is
/// a PUT-able resource of things the user chose. Nothing here is choosable
/// from the browser: it is read-only by nature, and putting it beside the
/// settings would invite an attempt to set it.
///
/// The policy comes from `turn::policy`, the same call a real turn makes, so
/// this reports on the policy the agent will actually run under.
/// Voice status comes from `voice::status_value`, the same function as the
/// dedicated status route, so it reports the runtime that will receive audio.
async fn capabilities(State(state): State<AppState>) -> Json<serde_json::Value> {
    let web_search = zorp_agent::web_search_availability(&turn::policy(state.own_port));
    let voice = crate::voice::status_value().await;
    Json(json!({
        "web_search": {
            "available": web_search.available,
            "detail": web_search.detail,
        },
        "voice": voice,
    }))
}

async fn create_session(State(state): State<AppState>) -> Json<serde_json::Value> {
    let id = zorp_agent::new_session_id();
    state.create(&id);
    Json(json!({"id": id}))
}

/// Sessions come from the store, not from memory, so history survives a
/// server restart. In-memory sessions that have not recorded a message yet
/// are unioned in so a brand new chat appears in the sidebar immediately.
///
/// `title` is the generated one when there is one and the verbatim first
/// message when there is not. The fallback is the point: a titling call
/// that failed, was declined, or was never made leaves a sidebar that reads
/// exactly as it did before the feature existed. Only this endpoint reads
/// `display_title`; everything that must not be handed model-authored text,
/// the recall feed above all, still reads `task`.
async fn list_sessions(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut rows: Vec<serde_json::Value> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Ok(store) = zorp_agent::Store::open_default() {
        if let Ok(sessions) = store.sessions() {
            for s in sessions {
                seen.insert(s.id.clone());
                let title = s.display_title.unwrap_or(s.task);
                rows.push(json!({"id": s.id, "title": title, "status": s.status}));
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

/// Delete a conversation: its messages, its recorded file changes, and the
/// sidebar row itself.
///
/// A running turn is refused with the same 409 `start_turn` uses for a
/// second turn on a busy session, because the turn's own thread still holds
/// `id` and would otherwise go on writing to a session that no longer has a
/// row. Only the in-memory state is checked; a session this process has not
/// loaded cannot be running. On success the in-memory entry is dropped too,
/// so a stale backlog cannot reappear if the same id is ever reused.
async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Some(session) = state.get(&id) {
        if session.lock().unwrap().running {
            return (StatusCode::CONFLICT, "a turn is running on this session").into_response();
        }
    }
    let mut store = match zorp_agent::Store::open_default() {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match store.delete_session(&id) {
        Ok(existed) => {
            let removed_live = state.remove(&id).is_some();
            if existed || removed_live {
                StatusCode::NO_CONTENT.into_response()
            } else {
                (StatusCode::NOT_FOUND, "no such session").into_response()
            }
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
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
    /// Look at earlier conversations before answering this one.
    ///
    /// Absent means no, which is the answer for every caller that has not
    /// heard of this. Retrieval is per message rather than per session on
    /// purpose: it spends context and it puts text from old conversations,
    /// tool results and fetched pages included, in front of the model, so
    /// it is a thing to choose each time and not a mode to leave on.
    #[serde(default)]
    memory: bool,
}

#[derive(Deserialize)]
struct PanelBody {
    /// A short name for what is under review, shown in the report.
    label: String,
    /// The material itself.
    body: String,
    /// Which lenses to run, by name. Absent or empty runs the whole
    /// default panel.
    #[serde(default)]
    lenses: Vec<String>,
}

/// What the browser asks a Zorp mode run for.
///
/// The pre-registration trio is all-or-nothing, the same as the CLI's
/// three flags: required on the first attempt for a track, and after
/// that either left out or matching what is already on file.
/// `investigate::run` is what enforces the match. Leaving them out here
/// means "use what is already recorded".
#[cfg(feature = "research")]
#[derive(Deserialize)]
struct InvestigateBody {
    question: String,
    #[serde(default)]
    metric_name: Option<String>,
    #[serde(default)]
    kill_threshold: Option<f64>,
    #[serde(default)]
    threshold_direction: Option<String>,
}

/// The question whose ledger to read.
#[cfg(feature = "research")]
#[derive(Deserialize)]
struct LedgerQuery {
    question: String,
}

/// The lenses a panel can be built from.
///
/// A read of a code-defined list, so the browser can offer the choice
/// without being the thing that decides what a reviewer is told. A
/// browser that could send instructions could send one reviewer the
/// answer it wanted.
async fn list_lenses() -> Json<serde_json::Value> {
    let lenses: Vec<serde_json::Value> = zorp_agent::default_lenses()
        .iter()
        .map(|l| serde_json::json!({"name": l.name, "instruction": l.instruction}))
        .collect();
    Json(serde_json::json!({ "lenses": lenses }))
}

/// Launch a review panel on this session.
///
/// 202 and the same 409 as `start_turn`, for the same reason: a panel
/// occupies the session, and a panel interleaved with a turn would put
/// two conversations under one sequence counter. It answers the
/// existing stop endpoint too, so a panel is stoppable with the control
/// that is already on the page.
///
/// An empty body is refused rather than sent to five reviewers. Five
/// agents asked to review nothing produce five confident answers about
/// nothing, which costs five requests and reads exactly like a real
/// panel.
async fn start_panel(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PanelBody>,
) -> impl IntoResponse {
    let Some(session) = session_or_adopt(&state, &id) else {
        return (StatusCode::NOT_FOUND, "no such session").into_response();
    };
    if body.body.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "nothing to review: the material is empty",
        )
            .into_response();
    }
    if session.lock().unwrap().running {
        return (StatusCode::CONFLICT, "a turn is already running").into_response();
    }
    crate::panel::spawn_panel(
        session,
        crate::panel::PanelRequest {
            label: body.label,
            body: body.body,
            lenses: body.lenses,
        },
        state.settings,
    );
    StatusCode::ACCEPTED.into_response()
}

/// Zorp mode: one pre-registered `investigate` attempt, launched from
/// the browser.
///
/// There is no aryabhatta engine to call. aryabhatta is record plus
/// readers and ships no command on purpose; `investigate` is what writes
/// to it. So this endpoint runs one attempt, and the ledger endpoint
/// below reads back what landed.
///
/// 202 and the same 409 as `start_turn`, for the same reason: an attempt
/// occupies the session, and an attempt interleaved with a turn would
/// put two conversations under one sequence counter. It answers the
/// existing stop endpoint too, so the control already on the page
/// reaches it.
///
/// A person presses this. There is no tool that reaches it and there
/// must never be one: an attempt writes to a pre-registered evidence
/// record and to the aryabhatta ledger, so a model that could start one
/// could feed the record it is later read against.
#[cfg(feature = "research")]
async fn start_investigate(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<InvestigateBody>,
) -> impl IntoResponse {
    let Some(session) = session_or_adopt(&state, &id) else {
        return (StatusCode::NOT_FOUND, "no such session").into_response();
    };
    let request = crate::investigate::InvestigateRequest {
        question: body.question,
        metric_name: body.metric_name,
        kill_threshold: body.kill_threshold,
        threshold_direction: body.threshold_direction,
    };
    // Checked before the session is occupied. A request that cannot run
    // comes back as a refused request, not as an error frame on a stream
    // the browser has to be watching to see.
    if let Err(e) = crate::investigate::check_request(&request) {
        return (StatusCode::BAD_REQUEST, e.message()).into_response();
    }
    if session.lock().unwrap().running {
        return (StatusCode::CONFLICT, "a turn is already running").into_response();
    }
    crate::investigate::spawn_investigate(session, request, state.settings.clone());
    StatusCode::ACCEPTED.into_response()
}

/// The same endpoint on a server built without `research`.
///
/// 501 rather than a missing route. A browser that gets a 404 cannot
/// tell "this server does not do that" from "you typed the URL wrong",
/// and the page has to be able to say which one it is.
#[cfg(not(feature = "research"))]
async fn start_investigate(
    Path(_id): Path<String>,
    body: Option<Json<serde_json::Value>>,
) -> impl IntoResponse {
    let _ = body;
    (StatusCode::NOT_IMPLEMENTED, RESEARCH_ABSENT).into_response()
}

#[cfg(not(feature = "research"))]
const RESEARCH_ABSENT: &str = "this zorp-web was built without the research feature, so it cannot \
     run an investigation. Rebuild it with --features research.";

/// Whether Zorp mode can run here, and whether it will forecast.
///
/// Two facts the page cannot work out for itself. `available` is what
/// this binary was built with. `forecasting` is whether `ZORP_FORECAST`
/// is set in the server's environment, which is what decides whether an
/// attempt records an expectation at all.
///
/// Reported, never set. Forecasting costs a model call on every attempt
/// and is off unless the person running the server turned it on. A
/// browser control that flipped it would be one page changing what the
/// whole server does for everyone using it.
async fn investigate_status() -> Json<serde_json::Value> {
    #[cfg(feature = "research")]
    let forecasting = zorp_agent::investigate::forecasting_enabled();
    #[cfg(not(feature = "research"))]
    let forecasting = false;
    Json(json!({
        "available": cfg!(feature = "research"),
        "forecasting": forecasting,
    }))
}

/// Read back what a track's attempts recorded.
///
/// A read and nothing else. It opens no run record that is not already
/// there, it asks no model anything, and it names no column holding
/// model-authored text. Detection is code and interpreting is somebody
/// else's job, which is the split the whole subsystem rests on.
#[cfg(feature = "research")]
async fn investigate_ledger(Query(params): Query<LedgerQuery>) -> impl IntoResponse {
    if params.question.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "no question given").into_response();
    }
    let root = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match crate::investigate::read_ledger(&root, &params.question) {
        Ok(ledger) => Json(ledger).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[cfg(not(feature = "research"))]
async fn investigate_ledger() -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, RESEARCH_ABSENT).into_response()
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
    #[cfg(feature = "recall")]
    let recall_indexer = state.recall_indexer.clone();
    #[cfg(not(feature = "recall"))]
    let recall_indexer = ();
    turn::spawn_turn(
        session,
        id,
        body.message,
        body.memory,
        state.settings.clone(),
        state.own_port,
        recall_indexer,
    );
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
///
/// The listing goes out with a key when one is available, because a
/// locally protected endpoint (oMLX with `--api-key`) rejects an
/// anonymous listing and the panel then reads as "not connecting". A
/// POST body of `{"base_url": ..., "api_key": ...}` carries a candidate
/// key the panel has not saved yet; without one the stored key is used.
/// The candidate rides in a body, never the query string, so a secret
/// does not end up in URLs.
async fn list_models(
    State(state): State<AppState>,
    Query(query): Query<ModelsQuery>,
    body: axum::body::Bytes,
) -> Json<serde_json::Value> {
    let sent = serde_json::from_slice::<serde_json::Value>(&body).ok();
    let field = |name: &str| {
        sent.as_ref()
            .and_then(|v| v.get(name))
            .and_then(|u| u.as_str())
            .map(str::to_string)
            .filter(|u| !u.trim().is_empty())
    };
    let base_url = field("base_url").or(query.base_url).unwrap_or_default();
    let api_key = field("api_key").or_else(|| state.settings.lock().unwrap().api_key.clone());
    let result =
        tokio::task::spawn_blocking(move || settings::fetch_models(&base_url, api_key.as_deref()))
            .await
            .unwrap_or_else(|e| settings::ModelsResult {
                error: Some(format!("internal error: {e}")),
                ..settings::ModelsResult::default()
            });
    // `models` is the bare id list every existing caller reads and it does
    // not change. `details` is the same models with whatever else the
    // endpoint said about each, which for OpenRouter is what separates a
    // free model from a paid one.
    Json(json!({
        "models": result.models,
        "details": result.details,
        "error": result.error,
    }))
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
    let sent = serde_json::from_slice::<serde_json::Value>(&body).ok();
    let field = |name: &str| {
        sent.as_ref()
            .and_then(|v| v.get(name))
            .and_then(|u| u.as_str())
            .map(str::to_string)
            .filter(|u| !u.trim().is_empty())
    };

    // Resolved once, so a field the caller did not send falls back to the
    // configured value rather than to a hardcoded default. The whole point
    // is to test the settings that would actually be used.
    let resolved = state.settings.lock().unwrap().resolve();
    let candidate_base = field("base_url");
    if candidate_base.is_none() && !resolved.configured {
        return Json(json!({"ok": false, "reason": "no model is configured yet"}));
    }
    let base_url = candidate_base.unwrap_or(resolved.base_url);
    let model = field("model").unwrap_or(resolved.model);
    let provider = field("provider")
        .and_then(|p| p.parse().ok())
        .unwrap_or(resolved.provider);
    // The stored key is never returned by `resolve`, on purpose, so it is
    // read from the settings state directly here.
    let api_key = field("api_key").or_else(|| state.settings.lock().unwrap().api_key.clone());

    let outcome = tokio::task::spawn_blocking(move || {
        settings::probe_completion(&base_url, provider, &model, api_key.as_deref())
    })
    .await
    .unwrap_or_else(|e| Err(format!("internal error: {e}")));

    match outcome {
        Ok(()) => Json(json!({"ok": true})),
        Err(reason) => Json(json!({"ok": false, "reason": reason})),
    }
}

#[derive(Deserialize)]
struct ArtifactQuery {
    path: Option<String>,
    /// `text` asks for the readable text in a document rather than the
    /// document. Only a file with a reader behind it has a text form, so on
    /// anything else it is ignored rather than refused.
    ///
    /// This exists for the PDF, which is the one type with two useful
    /// answers: the file, for the browser's viewer, and the words in it, for
    /// a browser that has no viewer to give it to.
    #[serde(rename = "as")]
    form: Option<String>,
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
/// second-guessing the declared type, and the `sandbox` CSP means a served
/// file cannot reach the rest of this origin even if the browser does decide
/// to execute something in it. See `crate::artifacts` for the traversal
/// rules, and the header block at the bottom of this function for why a PDF
/// gets a different one word of that policy from an SVG.
async fn read_artifact(
    State(state): State<AppState>,
    Query(query): Query<ArtifactQuery>,
) -> axum::response::Response {
    use crate::artifacts::{self, Refusal};

    let Some(root) = state.workspace.clone() else {
        return refuse(
            StatusCode::NOT_FOUND,
            "this server was not started with a workspace",
        );
    };
    let requested = query.path.unwrap_or_default();
    if requested.is_empty() {
        return refuse(StatusCode::BAD_REQUEST, "no path given");
    }

    let resolved = tokio::task::spawn_blocking(move || artifacts::resolve(&root, &requested)).await;
    let path = match resolved {
        Ok(Ok(p)) => p,
        Ok(Err(Refusal::Outside)) => {
            return refuse(
                StatusCode::FORBIDDEN,
                "that path is outside this server's workspace",
            )
        }
        Ok(Err(Refusal::Missing)) => {
            return refuse(StatusCode::NOT_FOUND, "no such file in this workspace")
        }
        Ok(Err(Refusal::UnsupportedType)) => {
            return refuse(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "this endpoint does not serve that kind of file",
            )
        }
        Err(e) => {
            return refuse(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("internal error: {e}"),
            )
        }
    };

    // Size is checked before reading, not after, so an enormous file never
    // makes it into memory at all. What counts as enormous depends on what
    // the file is for: text is rendered by the page and a picture is not.
    let served = artifacts::served_as(&path).unwrap_or(artifacts::Served::Text);
    let cap = match served {
        artifacts::Served::Text => artifacts::MAX_TEXT_BYTES,
        artifacts::Served::Document(_) | artifacts::Served::Pdf => artifacts::MAX_DOCUMENT_BYTES,
        artifacts::Served::Image | artifacts::Served::Sandboxed => artifacts::MAX_BINARY_BYTES,
    };
    match std::fs::metadata(&path) {
        Ok(m) if m.len() > cap => {
            return refuse(
                StatusCode::PAYLOAD_TOO_LARGE,
                format!(
                    "that file is {} bytes, over the {cap} this pane will render",
                    m.len()
                ),
            )
        }
        Ok(_) => {}
        Err(e) => return refuse(StatusCode::NOT_FOUND, e.to_string()),
    }

    let body = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) => return refuse(StatusCode::NOT_FOUND, e.to_string()),
    };

    // Which of the two things this response carries. An office file is a zip
    // of XML and is only ever the text in it; a PDF is the file itself unless
    // something says otherwise. Two things say otherwise, and they are the
    // two ways the browser's viewer would have shown nothing: the caller
    // asked for the text because it has no viewer, or the file is not a PDF
    // at all and the viewer would draw a broken-document icon over it.
    let wants_text = query.form.as_deref() == Some("text");
    let form = match served {
        artifacts::Served::Document(_) => artifacts::Form::Markdown,
        artifacts::Served::Pdf if wants_text => artifacts::Form::Markdown,
        // Plain text and not markdown, because this one is read inside the
        // frame by the browser rather than by the page's renderer, and a
        // browser handed `text/markdown` offers to save it instead of
        // showing it.
        artifacts::Served::Pdf if !crate::pdf::looks_like_pdf(&body) => artifacts::Form::PlainText,
        _ => artifacts::Form::Bytes,
    };
    let mime = artifacts::response_type(&path, form).unwrap_or("application/octet-stream");

    // The readers run on a blocking thread. Reading is slow enough to matter
    // on a long document, and both are parsers pointed at a file a model
    // wrote or downloaded, so a panic in one has to end that request and
    // nothing else. `spawn_blocking` gives both: a panicking task comes back
    // as a join error rather than taking the server with it. See
    // `crate::documents` for the caps that make reading an archive safe.
    let body = match (form, artifacts::extraction(&path)) {
        (artifacts::Form::Bytes, _) | (_, None) => body,
        (_, Some(kind)) => {
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
                Ok(Ok(text)) => text.into_bytes(),
                // A file that is not really the format its name claims is an
                // ordinary outcome when a model wrote it, so it gets a sentence
                // rather than a 500 or a blank pane.
                Ok(Err(why)) => {
                    return refuse(StatusCode::UNPROCESSABLE_ENTITY, why);
                }
                Err(_) => {
                    return refuse(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "reading this document crashed the reader, so there is nothing to show",
                    );
                }
            }
        }
    };

    let mut headers = HeaderMap::new();
    headers.insert("content-type", mime.parse().unwrap());
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert("content-security-policy", sandbox_policy(served, form));
    // Inline, because the point is to show it in the pane. The sandbox above
    // is what makes that safe, not the disposition.
    headers.insert("content-disposition", "inline".parse().unwrap());
    (headers, body).into_response()
}

/// The `Content-Security-Policy` for one served file.
///
/// A bare `sandbox`, with no `allow-` token, is the default and the one every
/// type but a PDF gets. It puts the document in a unique origin with
/// scripting off, so script inside an SVG or an HTML file this server did not
/// write neither runs nor has a handle on the page that framed it.
///
/// A PDF gets one token more, and only one. The browser's PDF viewer is
/// itself a scripted document, and under a bare `sandbox` it does not start:
/// the pane showed a broken-document icon on grey, which is what the previous
/// attempt at this ran into. `sandbox allow-scripts` is what was measured to
/// work, in Chrome 151 on macOS, and it keeps the part that matters. Without
/// `allow-same-origin` the document is still in an opaque origin: reading
/// `parent.document`, `parent.location` or `localStorage` from inside one
/// throws `SecurityError`, `window.origin` is `null`, and the framing page's
/// title is untouched. That was measured too, with a hostile page served
/// under exactly this header, not argued from the spec.
///
/// The iframe that holds a PDF therefore carries no `sandbox` attribute,
/// because any value of it stops the viewer starting, including
/// `allow-scripts`. This header is the whole of the isolation for that frame,
/// which is the reason a PDF is its own `Served` variant rather than a third
/// `Sandboxed` one: the two must not drift into sharing a policy.
///
/// A PDF read for its text is text, so it goes back to the bare `sandbox`.
/// A refusal from this endpoint, under headers as inert as a served file's.
///
/// These land inside the pane's frame now. A `.pdf` the reader could not read
/// is answered right here, with no page in the middle to catch it, and the
/// frame that shows it carries no `sandbox` attribute of its own. So the
/// sentence goes out declared, `nosniff`ed and sandboxed like everything else
/// this endpoint sends, rather than as a bare body a browser is left to make
/// its own mind up about.
fn refuse(status: StatusCode, message: impl Into<String>) -> axum::response::Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        axum::http::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    headers.insert(
        "x-content-type-options",
        axum::http::HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        "content-security-policy",
        axum::http::HeaderValue::from_static("sandbox"),
    );
    (status, headers, message.into()).into_response()
}

fn sandbox_policy(
    served: crate::artifacts::Served,
    form: crate::artifacts::Form,
) -> axum::http::HeaderValue {
    use crate::artifacts::{Form, Served};
    let policy = match (served, form) {
        (Served::Pdf, Form::Bytes) => "sandbox allow-scripts",
        _ => "sandbox",
    };
    axum::http::HeaderValue::from_static(policy)
}

/* ------------------------------------------------------------------ */
/* conversation search                                                 */
/* ------------------------------------------------------------------ */

/// Whether this server can search the conversations it holds, and what it
/// would use to do it.
///
/// Answered by every build. A page that got a 404 here would have to guess
/// whether the feature is off or the server is old, and the two want
/// different words on screen.
#[cfg(not(feature = "recall"))]
async fn recall_status() -> Json<serde_json::Value> {
    Json(json!({
        "available": false,
        "reason": "this server was built without the `recall` feature, so conversation search is off",
        "endpoint": null,
        "model": null,
        "conversations": 0,
        "store_conversations": 0,
        "chunks": 0,
        "running": false,
        "ready": false,
        "memory": false,
    }))
}

#[cfg(feature = "recall")]
async fn recall_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let indexer = state.recall_indexer.clone();
    let status = tokio::task::spawn_blocking(move || crate::recall::status(indexer.as_ref()))
        .await
        .expect("recall status does not panic");
    Json(json!({
        "available": status.available,
        "reason": status.reason,
        "endpoint": status.endpoint,
        "model": status.model,
        "conversations": status.conversations,
        "store_conversations": status.store_conversations,
        "chunks": status.chunks,
        "running": status.running,
        "ready": status.ready,
        // Whether a turn can be told to read this index, which is a
        // separate build-time choice from whether the sidebar can search
        // it. The page needs it to decide whether to offer the box, and
        // answering it in every build keeps the browser from guessing.
        "memory": cfg!(feature = "memory"),
    }))
}

#[cfg(not(feature = "recall"))]
async fn recall_index() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        "this server was built without the `recall` feature, so there is nothing to index",
    )
}

/// Bring the index up to date with the store.
///
/// Kept for scripts and tests that need to force a pass. A running server
/// sends it through the same worker as startup, periodic and per-session
/// indexing, so no two passes can interleave.
#[cfg(feature = "recall")]
async fn recall_index(State(state): State<AppState>) -> impl IntoResponse {
    let indexer = state.recall_indexer.clone();
    match tokio::task::spawn_blocking(move || match indexer {
        Some(indexer) => indexer.sweep(),
        None => crate::recall::reindex(),
    })
    .await
    {
        Ok(Ok(report)) => Json(json!({
            "indexed": report.indexed,
            "skipped": report.skipped,
            "removed": report.removed,
            "chunks": report.chunks,
        }))
        .into_response(),
        Ok(Err(e)) => recall_failure(e).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "indexing crashed".to_string(),
        )
            .into_response(),
    }
}

#[cfg(feature = "recall")]
#[derive(Deserialize)]
struct RecallSearch {
    #[serde(default)]
    q: String,
    limit: Option<usize>,
}

#[cfg(not(feature = "recall"))]
async fn recall_search() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        "this server was built without the `recall` feature, so there is nothing to search",
    )
}

#[cfg(feature = "recall")]
async fn recall_search(Query(params): Query<RecallSearch>) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(crate::recall::DEFAULT_LIMIT);
    match tokio::task::spawn_blocking(move || crate::recall::search(&params.q, limit)).await {
        Ok(Ok(hits)) => {
            let rows: Vec<serde_json::Value> = hits
                .into_iter()
                .map(|h| {
                    json!({
                        "id": h.conversation_id,
                        "title": h.title,
                        "seq": h.seq,
                        "role": h.role,
                        "snippet": h.snippet,
                        "score": h.score,
                    })
                })
                .collect();
            Json(json!({ "hits": rows })).into_response()
        }
        Ok(Err(e)) => recall_failure(e).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "searching crashed".to_string(),
        )
            .into_response(),
    }
}

/// One place deciding what a recall failure looks like on the wire.
///
/// 503 for anything to do with the embedder, including a configured
/// endpoint that is not on this machine, because in both cases the thing
/// that would produce a vector is not available and the answer is the same:
/// nothing was searched and nothing was sent anywhere. The message is
/// passed through whole, since it is the only thing the page can show and
/// it is already written to be read by a person.
#[cfg(feature = "recall")]
fn recall_failure(e: crate::recall::RecallError) -> (StatusCode, String) {
    use crate::recall::RecallError;
    let status = match e {
        RecallError::Embed(_) => StatusCode::SERVICE_UNAVAILABLE,
        RecallError::EmptyQuery => StatusCode::BAD_REQUEST,
        RecallError::Busy => StatusCode::CONFLICT,
        RecallError::Index(_) | RecallError::Store(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, e.to_string())
}
