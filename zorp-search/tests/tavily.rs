mod common;

use common::{captured, mock, mock_capture, mock_hangup};
use serde_json::Value;
use std::sync::Mutex;
use zorp_search::{Query, SearchError, SearchProvider, TavilyProvider};

/// Serializes the env-mutating tests. Every other test passes the key in.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Take the env lock, ignoring poisoning. A failing env test would otherwise
/// turn its neighbour into a confusing PoisonError instead of a real result.
fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

const OK_BODY: &str = r#"{
  "query": "rust",
  "results": [
    {"title": "First", "url": "https://one.example/a", "content": "one snippet", "score": 0.91},
    {"title": "Second", "url": "https://two.example/b", "content": "two snippet"}
  ]
}"#;

fn provider(base: &str) -> TavilyProvider {
    TavilyProvider::with_key_and_base_url("test-key-123", base).unwrap()
}

#[test]
fn name_is_tavily() {
    assert_eq!(provider("http://127.0.0.1:1").name(), "tavily");
}

#[test]
fn well_formed_response_parses_into_results() {
    let base = mock(200, "application/json", OK_BODY);
    let results = provider(&base).search(&Query::new("rust")).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].title, "First");
    assert_eq!(results[0].url, "https://one.example/a");
    // Tavily calls the extract `content`; zorp calls it `snippet`.
    assert_eq!(results[0].snippet, "one snippet");
    assert_eq!(results[0].score, Some(0.91));
    assert_eq!(results[1].title, "Second");
    assert_eq!(results[1].score, None);
}

#[test]
fn empty_results_array_is_an_empty_vec_not_an_error() {
    // An honest "nothing found" is a success. It is the failed request that
    // must not turn into an empty Vec, not this.
    let base = mock(200, "application/json", r#"{"results": []}"#);
    let results = provider(&base).search(&Query::new("rust")).unwrap();
    assert!(results.is_empty());
}

#[test]
fn missing_results_key_is_a_malformed_response_error() {
    // The whole point of D7: a response zorp cannot read is an error, never
    // silently zero results.
    let base = mock(200, "application/json", r#"{"query": "rust"}"#);
    let err = provider(&base).search(&Query::new("rust")).unwrap_err();
    assert!(
        matches!(err, SearchError::MalformedResponse { .. }),
        "wrong variant: {err:?}"
    );
    let msg = err.to_string();
    assert!(msg.contains("tavily"), "provider not named in: {msg}");
    assert!(msg.contains("results"), "field not named in: {msg}");
}

#[test]
fn results_that_is_not_an_array_is_a_malformed_response_error() {
    let base = mock(200, "application/json", r#"{"results": "nope"}"#);
    let err = provider(&base).search(&Query::new("rust")).unwrap_err();
    assert!(
        matches!(err, SearchError::MalformedResponse { .. }),
        "wrong variant: {err:?}"
    );
}

#[test]
fn body_that_is_not_json_is_a_malformed_response_error() {
    let base = mock(200, "text/html", "<html>we are down</html>");
    let err = provider(&base).search(&Query::new("rust")).unwrap_err();
    assert!(
        matches!(err, SearchError::MalformedResponse { .. }),
        "wrong variant: {err:?}"
    );
}

#[test]
fn result_missing_a_url_is_a_malformed_response_error() {
    // A result with no URL cannot be cited, so guessing a blank one would put
    // a useless row into an evidence record.
    let base = mock(
        200,
        "application/json",
        r#"{"results": [{"title": "First", "content": "one"}]}"#,
    );
    let err = provider(&base).search(&Query::new("rust")).unwrap_err();
    assert!(
        matches!(err, SearchError::MalformedResponse { .. }),
        "wrong variant: {err:?}"
    );
    let msg = err.to_string();
    assert!(msg.contains("url"), "field not named in: {msg}");
}

#[test]
fn non_200_is_a_status_error_naming_the_status() {
    let base = mock(429, "application/json", r#"{"detail": "rate limited"}"#);
    let err = provider(&base).search(&Query::new("rust")).unwrap_err();
    assert!(
        matches!(err, SearchError::Status { status: 429, .. }),
        "wrong variant: {err:?}"
    );
    let msg = err.to_string();
    assert!(msg.contains("429"), "status not named in: {msg}");
    assert!(msg.contains("tavily"), "provider not named in: {msg}");
    assert!(msg.contains("rate limited"), "body dropped from: {msg}");
}

#[test]
fn a_dropped_connection_is_a_transport_error() {
    let base = mock_hangup();
    let err = provider(&base).search(&Query::new("rust")).unwrap_err();
    assert!(
        matches!(err, SearchError::Transport { .. }),
        "wrong variant: {err:?}"
    );
    assert!(
        err.to_string().contains("tavily"),
        "provider not named in: {err}"
    );
}

#[test]
fn the_key_is_sent_as_a_bearer_header() {
    let (base, rx) = mock_capture(200, "application/json", OK_BODY);
    provider(&base).search(&Query::new("rust")).unwrap();
    let request = captured(&rx).to_lowercase();
    assert!(
        request.contains("authorization: bearer test-key-123"),
        "no bearer header in: {request}"
    );
}

#[test]
fn the_query_and_max_results_reach_the_wire() {
    let (base, rx) = mock_capture(200, "application/json", OK_BODY);
    let query = Query::new("rust ownership").with_max_results(3);
    provider(&base).search(&query).unwrap();
    let request = captured(&rx);
    let body = request.split("\r\n\r\n").nth(1).expect("no request body");
    let sent: Value = serde_json::from_str(body).expect("body is not json");
    assert_eq!(sent["query"], "rust ownership");
    assert_eq!(sent["max_results"], 3);
    assert_eq!(sent["search_depth"], "basic");
    // The key travels in the header, never in the body.
    assert!(!body.contains("test-key-123"), "key in body: {body}");
}

#[test]
fn max_results_is_omitted_when_unset() {
    let (base, rx) = mock_capture(200, "application/json", OK_BODY);
    provider(&base).search(&Query::new("rust")).unwrap();
    let request = captured(&rx);
    let body = request.split("\r\n\r\n").nth(1).expect("no request body");
    let sent: Value = serde_json::from_str(body).expect("body is not json");
    assert!(sent.get("max_results").is_none(), "sent: {sent}");
}

#[test]
fn the_key_never_reaches_error_output() {
    // Tavily's own 401 body echoes the key it rejected. That must not survive
    // into an error message, which ends up in a tool result and a trace.
    let body = r#"{"detail":"invalid api key: test-key-123"}"#;
    let base = mock(401, "application/json", body);
    let err = provider(&base).search(&Query::new("rust")).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("401"), "status not named in: {msg}");
    assert!(!msg.contains("test-key-123"), "key leaked into: {msg}");
    assert!(!format!("{err:?}").contains("test-key-123"), "key in Debug");
}

#[test]
fn the_key_never_reaches_debug_output() {
    let printed = format!("{:?}", provider("http://127.0.0.1:1"));
    assert!(
        !printed.contains("test-key-123"),
        "key leaked into: {printed}"
    );
}

#[test]
fn a_missing_env_key_errors_naming_the_variable() {
    let _guard = env_lock();
    let saved = std::env::var("ZORP_TAVILY_API_KEY").ok();
    std::env::remove_var("ZORP_TAVILY_API_KEY");
    let err = TavilyProvider::from_env().unwrap_err();
    if let Some(value) = saved {
        std::env::set_var("ZORP_TAVILY_API_KEY", value);
    }
    assert!(
        matches!(err, SearchError::MissingApiKey { .. }),
        "wrong variant: {err:?}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("ZORP_TAVILY_API_KEY"),
        "variable not named in: {msg}"
    );
    assert!(msg.contains("tavily"), "provider not named in: {msg}");
}

#[test]
fn an_empty_env_key_errors_naming_the_variable() {
    let _guard = env_lock();
    let saved = std::env::var("ZORP_TAVILY_API_KEY").ok();
    std::env::set_var("ZORP_TAVILY_API_KEY", "");
    let err = TavilyProvider::from_env().unwrap_err();
    match saved {
        Some(value) => std::env::set_var("ZORP_TAVILY_API_KEY", value),
        None => std::env::remove_var("ZORP_TAVILY_API_KEY"),
    }
    assert!(
        matches!(err, SearchError::MissingApiKey { .. }),
        "wrong variant: {err:?}"
    );
    assert!(
        err.to_string().contains("ZORP_TAVILY_API_KEY"),
        "variable not named in: {err}"
    );
}

#[test]
fn an_empty_explicit_key_is_rejected_too() {
    let err = TavilyProvider::with_key_and_base_url("", "http://127.0.0.1:1").unwrap_err();
    assert!(
        matches!(err, SearchError::MissingApiKey { .. }),
        "wrong variant: {err:?}"
    );
}

#[test]
fn the_provider_is_usable_behind_the_trait_object() {
    let base = mock(200, "application/json", OK_BODY);
    let provider: Box<dyn SearchProvider> = Box::new(provider(&base));
    assert_eq!(provider.name(), "tavily");
    assert_eq!(provider.search(&Query::new("rust")).unwrap().len(), 2);
}
