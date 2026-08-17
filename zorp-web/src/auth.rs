use crate::state::AppState;
use axum::extract::{Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct TokenQuery {
    pub token: Option<String>,
}

/// Accepts the token from either an Authorization header or a query
/// parameter.
///
/// The query parameter is not redundant: `EventSource` cannot set headers, so
/// a header-only scheme would leave the event stream, and therefore the whole
/// UI, unusable across origins.
pub async fn require_token(
    State(state): State<AppState>,
    Query(query): Query<TokenQuery>,
    request: Request,
    next: Next,
) -> Response {
    let Some(expected) = state.token.clone() else {
        return next.run(request).await;
    };

    let header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string);

    let presented = header.or(query.token);
    match presented {
        Some(value) if value == expected => next.run(request).await,
        _ => (StatusCode::UNAUTHORIZED, "missing or wrong token").into_response(),
    }
}
