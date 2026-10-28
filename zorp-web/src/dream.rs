//! Dreaming mode in the browser: the "While you were away" card and the
//! routes behind it, plus the background pass.
//!
//! All of it is local. A suggestion is a row in the session store the two
//! surfaces already share (`zorp_agent::dream`), detection is code, and the
//! wording is the code-only [`TemplatePhraser`]. Nothing here reads a
//! model-authored column, nothing here is reachable by a model, and the
//! automatic pass is off until a person sets `ZORP_DREAM`. See ADR 0144 and
//! the `dream` bullet in `CLAUDE.md`.
//!
//! The routes live in every build, the same way the project and recall-status
//! routes do: a suggestion is a store row and has nothing to do with which
//! features this binary was compiled with. "Off by default" is the worker
//! being off, not the routes being absent, so a person can run a pass by hand
//! from the card or the CLI without turning the timer on.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
use zorp_agent::dream;

use crate::state::AppState;

/// How often the background pass runs when `ZORP_DREAM` turns it on, in
/// seconds. Beside `ZORP_RECALL_SWEEP_SECS`; 0 disables the timer while
/// leaving the manual pass available.
pub const SWEEP_VAR: &str = "ZORP_DREAM_SWEEP_SECS";
const DEFAULT_SWEEP_SECS: u64 = 900;

/// How many conversations one pass reads back. The same conservative cap the
/// CLI uses: enough history to find a pattern, not the whole store on a
/// machine with thousands of threads.
const GATHER_LIMIT: usize = 1000;

/// `GET /api/dreams`: the open suggestions, newest first, each with the ids of
/// the conversations it came from so the card can link to them.
///
/// Answers in every build. `enabled` reports whether the automatic pass is on
/// (`ZORP_DREAM`), which is a fact about the environment the server was
/// started in, so the card can say "run a pass" rather than wait forever for a
/// timer nobody turned on.
pub async fn list() -> impl IntoResponse {
    let store = match zorp_agent::Store::open_default() {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match store.list_suggestions(Some("new"), 50) {
        Ok(rows) => {
            let suggestions: Vec<serde_json::Value> = rows
                .into_iter()
                .map(|row| {
                    json!({
                        "id": row.id,
                        "kind": row.kind,
                        // The one model-authored field. The page puts it on
                        // through `textContent`, labelled as a suggestion, the
                        // same as every other string a model wrote.
                        "wording": row.wording,
                        "evidence_ids": row.evidence_ids,
                        "created": row.created,
                    })
                })
                .collect();
            Json(json!({
                "available": true,
                "enabled": dream_enabled(),
                "suggestions": suggestions,
            }))
            .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// `POST /api/dreams/run`: run one pass now and report how many suggestions it
/// added. A person's click, the same as the CLI `zorp-agent dreams run`; no
/// model reaches it.
pub async fn run() -> impl IntoResponse {
    tokio::task::spawn_blocking(run_pass_blocking)
        .await
        .unwrap_or_else(|_| Err("the dream pass crashed".to_string()))
        .map(|recorded| Json(json!({ "recorded": recorded })))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

/// `POST /api/dreams/:id/accept`: create the real object the suggestion
/// proposes and mark it accepted. A project needs no workspace; a flavor is
/// written under `<workspace>/.zorp/flavors` and is untrusted until a person
/// trusts it (ADR 0123), so a server with no workspace chosen refuses a
/// flavor accept with the same words a turn gets.
pub async fn accept(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let flavor_dir = state
        .workspace_root()
        .map(|root| root.join(".zorp").join("flavors"));
    let outcome = tokio::task::spawn_blocking(move || accept_blocking(&id, flavor_dir))
        .await
        .unwrap_or_else(|_| Err(AcceptError::Internal("accepting crashed".into())));
    match outcome {
        Ok(dream::AcceptOutcome::Project { name, filed }) => {
            Json(json!({"created": "project", "name": name, "filed": filed})).into_response()
        }
        Ok(dream::AcceptOutcome::Flavor { path }) => Json(json!({
            "created": "flavor",
            "path": path.display().to_string(),
            // Said out loud so the card can: a dream-written flavor grants
            // nothing and is not trusted until a person trusts it.
            "trusted": false,
        }))
        .into_response(),
        Err(AcceptError::NoSuchSuggestion) => {
            (StatusCode::NOT_FOUND, "no such suggestion").into_response()
        }
        Err(AcceptError::NoWorkspace) => (
            StatusCode::CONFLICT,
            crate::workspace::NO_WORKSPACE.to_string(),
        )
            .into_response(),
        Err(AcceptError::Internal(e)) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

/// `POST /api/dreams/:id/dismiss`: record the suggestion dismissed so its
/// pattern is not raised again.
pub async fn dismiss(Path(id): Path<String>) -> impl IntoResponse {
    tokio::task::spawn_blocking(move || {
        let store = zorp_agent::Store::open_default().map_err(|e| e.to_string())?;
        if store.suggestion(&id).map_err(|e| e.to_string())?.is_none() {
            return Err("no such suggestion".to_string());
        }
        store
            .set_suggestion_state(&id, "dismissed")
            .map_err(|e| e.to_string())
    })
    .await
    .unwrap_or_else(|_| Err("dismissing crashed".to_string()))
    .map(|()| StatusCode::NO_CONTENT)
    .map_err(|e| {
        let status = if e == "no such suggestion" {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        (status, e)
    })
}

enum AcceptError {
    NoSuchSuggestion,
    NoWorkspace,
    Internal(String),
}

/// The blocking body of accept: open the store, look the suggestion up, and
/// create its object. A flavor kind needs a workspace; a project kind does
/// not, so the workspace is only required when the kind will write a file.
fn accept_blocking(
    id: &str,
    flavor_dir: Option<std::path::PathBuf>,
) -> Result<dream::AcceptOutcome, AcceptError> {
    let mut store =
        zorp_agent::Store::open_default().map_err(|e| AcceptError::Internal(e.to_string()))?;
    let suggestion = store
        .suggestion(id)
        .map_err(|e| AcceptError::Internal(e.to_string()))?
        .ok_or(AcceptError::NoSuchSuggestion)?;
    // Only a flavor accept writes a file, so only it needs a workspace.
    let dir = if suggestion.kind == "unfiled_cluster" {
        std::path::PathBuf::new()
    } else {
        flavor_dir.ok_or(AcceptError::NoWorkspace)?
    };
    dream::accept(&mut store, &suggestion, &dir).map_err(|e| AcceptError::Internal(e.to_string()))
}

fn run_pass_blocking() -> Result<usize, String> {
    let store = zorp_agent::Store::open_default().map_err(|e| e.to_string())?;
    let records = dream::gather(&store, GATHER_LIMIT).map_err(|e| e.to_string())?;
    let recorded = dream::run_pass(
        &store,
        &records,
        &dream::TemplatePhraser,
        dream::DEFAULT_MAX_SUGGESTIONS,
    )
    .map_err(|e| e.to_string())?;
    Ok(recorded.len())
}

/// Whether the automatic pass is on. `ZORP_DREAM` unset, empty or `0` is off,
/// the same shape `ZORP_RECALL_SWEEP_SECS` treats `0`.
fn dream_enabled() -> bool {
    match std::env::var(dream::DREAM_VAR) {
        Ok(v) => !v.is_empty() && v != "0",
        Err(_) => false,
    }
}

/// Start the background dream pass if `ZORP_DREAM` turns it on.
///
/// Beside the recall sweep (ADR 0079) and off by default: a feature that
/// reasons over a person's whole history is theirs to enable. It runs the same
/// code-only pass the manual route runs, serialized by its own timer, and logs
/// the first failure then stays quiet, the way the recall worker does. It
/// touches only the store, so it needs no workspace and no model.
pub fn spawn_worker() {
    if !dream_enabled() {
        return;
    }
    let secs = std::env::var(SWEEP_VAR)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_SWEEP_SECS);
    if secs == 0 {
        return;
    }
    tokio::spawn(async move {
        let mut logged_failure = false;
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(secs));
        // The first tick fires at once; skip straight to the interval so the
        // pass does not run during server startup.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            match tokio::task::spawn_blocking(run_pass_blocking).await {
                Ok(Ok(_)) => {
                    if logged_failure {
                        eprintln!("zorp-web: dream pass recovered");
                        logged_failure = false;
                    }
                }
                Ok(Err(e)) => {
                    if !logged_failure {
                        eprintln!("zorp-web: dream pass failed: {e}");
                        logged_failure = true;
                    }
                }
                Err(_) => {}
            }
        }
    });
}
