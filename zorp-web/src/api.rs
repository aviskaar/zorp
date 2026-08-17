use axum::{routing::get, Json, Router};
use serde_json::json;

/// The HTTP surface. Split from `main` so integration tests can serve it on an
/// ephemeral port without going through the binary's argument parsing.
pub fn router() -> Router {
    Router::new().route("/api/health", get(health))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}
