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

/// Refuses a state-changing request that a browser says came from another
/// page.
///
/// CORS stops a foreign page from reading what this server answers. It does
/// not stop the browser from sending a "simple" request, a POST with no
/// custom header and a `text/plain` body or none at all, and on the ordinary
/// loopback install there is no token to stop it either. A route whose
/// handler takes no JSON body would run for any page the person happened to
/// visit. A route that does take JSON is protected only because its
/// extractor refuses the content type, which is a side effect and not a
/// rule.
///
/// `Sec-Fetch-Site` is the signal, because a page cannot set it. It reads
/// `same-origin` for the UI's own requests even through the container's
/// nginx, which rewrites `Host` and attaches the token for anyone who
/// reaches it, so neither of those can decide this. It reads `same-site`,
/// not `same-origin`, for a page on another loopback port, which is
/// refused: a dev server is another program. A browser too old to send it
/// falls back to comparing `Origin` with `Host`. A request with neither
/// header is not from a browser page (curl, the CLI, the Mac app) and
/// passes. An origin named with `--allow-origin` passes either way.
pub async fn refuse_cross_site(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    use axum::http::{header, Method};
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        return next.run(request).await;
    }
    let headers = request.headers();
    let text = |name| headers.get(name).and_then(|v| v.to_str().ok());
    let origin = text(header::ORIGIN);
    let named = origin.is_some_and(|o| state.allowed_origins.iter().any(|a| a == o));
    let allowed = named
        || match text(header::HeaderName::from_static("sec-fetch-site")) {
            Some(site) => site == "same-origin" || site == "none",
            None => match origin {
                None => true,
                Some(origin) => text(header::HOST).is_some_and(|host| {
                    origin
                        .strip_prefix("http://")
                        .or_else(|| origin.strip_prefix("https://"))
                        == Some(host)
                }),
            },
        };
    if allowed {
        next.run(request).await
    } else {
        (
            StatusCode::FORBIDDEN,
            "refusing a request another page sent: only this server's own page may change anything",
        )
            .into_response()
    }
}
