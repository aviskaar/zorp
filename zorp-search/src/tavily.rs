//! Tavily, the first provider behind `SearchProvider`.
//!
//! Tavily is one POST to one endpoint: a JSON body with the query, a bearer
//! token in the header, and a `results` array of already extracted text back.
//! Everything Tavily specific stays in this file.

use crate::{Query, SearchError, SearchProvider, SearchResult};
use serde_json::{json, Value};
use std::io::Read;
use std::time::Duration;

/// The only place a Tavily key is read from. Never a flavor manifest: the
/// manifest refuses unknown fields on purpose, and a key does not belong in a
/// file that gets committed.
pub const TAVILY_API_KEY_VAR: &str = "ZORP_TAVILY_API_KEY";

/// Tavily's public API root. Tests point the provider at a local socket.
pub const TAVILY_BASE_URL: &str = "https://api.tavily.com";

/// Overrides the Tavily endpoint. Exists so the CLI path can be exercised
/// against a local stub without spending API quota, and so anyone behind a
/// proxy or self-hosted gateway can point at it. Unset means the real API.
pub const TAVILY_BASE_URL_VAR: &str = "ZORP_TAVILY_BASE_URL";

const PROVIDER: &str = "tavily";

/// Pinned rather than left to Tavily's default, so a change on their side does
/// not quietly change what the evidence says. Exposing the knob can wait for
/// someone who needs it.
const SEARCH_DEPTH: &str = "basic";

/// How much of a non-2xx body goes into the error. Enough to carry Tavily's
/// explanation, small enough never to bloat a tool result.
const ERROR_BODY_CAP: u64 = 8 * 1024;

/// Tavily's search API behind the provider trait.
pub struct TavilyProvider {
    key: String,
    base_url: String,
    agent: ureq::Agent,
}

/// Written by hand, not derived. A derived `Debug` would print the key, and
/// `{:?}` on a provider is exactly the accident D6 is about.
impl std::fmt::Debug for TavilyProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TavilyProvider")
            .field("base_url", &self.base_url)
            .field("key", &"<redacted>")
            .finish()
    }
}

impl TavilyProvider {
    /// Read the key from `ZORP_TAVILY_API_KEY` and talk to the public API.
    /// A missing or empty key fails here, at construction, rather than on the
    /// first search.
    pub fn from_env() -> Result<Self, SearchError> {
        let key = std::env::var(TAVILY_API_KEY_VAR).unwrap_or_default();
        let base_url = std::env::var(TAVILY_BASE_URL_VAR)
            .ok()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| TAVILY_BASE_URL.to_string());
        Self::with_key_and_base_url(key, base_url)
    }

    /// The endpoint this provider will call. Carries no secret.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Explicit key and base URL, for tests and for anyone pointing at a proxy.
    pub fn with_key_and_base_url(
        key: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Result<Self, SearchError> {
        let key = key.into();
        if key.trim().is_empty() {
            return Err(SearchError::MissingApiKey {
                provider: PROVIDER.to_string(),
                var: TAVILY_API_KEY_VAR.to_string(),
            });
        }
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(30))
            .build();
        Ok(TavilyProvider {
            key,
            base_url: base_url.into(),
            agent,
        })
    }

    /// Strip the key out of any text that came back over the wire. Tavily
    /// echoes the rejected key in some error bodies, and an error message ends
    /// up in a tool result and in the trace. The key is never empty, so this
    /// cannot degenerate into replacing every position in the string.
    fn redact(&self, text: &str) -> String {
        text.replace(&self.key, "<redacted>")
    }

    fn transport(&self, message: String) -> SearchError {
        SearchError::Transport {
            provider: PROVIDER.to_string(),
            message: self.redact(&message),
        }
    }
}

impl SearchProvider for TavilyProvider {
    fn name(&self) -> &str {
        PROVIDER
    }

    fn search(&self, query: &Query) -> Result<Vec<SearchResult>, SearchError> {
        let url = format!("{}/search", self.base_url.trim_end_matches('/'));
        let mut body = json!({
            "query": query.text,
            "search_depth": SEARCH_DEPTH,
        });
        if let Some(max) = query.max_results {
            body["max_results"] = json!(max);
        }
        // The key goes in the header and only in the header, so no code path
        // can put it in a request body that later gets logged.
        let sent = self
            .agent
            .post(&url)
            .set("Authorization", &format!("Bearer {}", self.key))
            .send_json(body);
        let response = match sent {
            Ok(response) => response,
            Err(ureq::Error::Status(status, response)) => {
                return Err(SearchError::Status {
                    provider: PROVIDER.to_string(),
                    status,
                    body: self.redact(&read_capped(response)),
                })
            }
            Err(err) => return Err(self.transport(err.to_string())),
        };
        let value: Value = response
            .into_json()
            .map_err(|err| malformed(format!("body is not JSON: {err}")))?;
        parse_results(&value)
    }
}

/// Read a bounded prefix of a response body as text.
fn read_capped(response: ureq::Response) -> String {
    let mut bytes = Vec::new();
    let _ = response
        .into_reader()
        .take(ERROR_BODY_CAP)
        .read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).trim().to_string()
}

/// Turn Tavily's response into results. A response without a readable
/// `results` array is an error, not an empty list: an empty list is a real
/// answer ("nothing matched") and a broken response is not, and a novelty
/// score computed from the two would mean different things.
fn parse_results(value: &Value) -> Result<Vec<SearchResult>, SearchError> {
    let array = value
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| malformed("no `results` array in the response"))?;
    let mut results = Vec::with_capacity(array.len());
    for (index, item) in array.iter().enumerate() {
        results.push(SearchResult {
            title: string_field(item, "title", index)?,
            url: string_field(item, "url", index)?,
            // Tavily's `content` is the extracted text; zorp calls it a
            // snippet. Extraction can come back empty for a page that is
            // still a real, citable hit, so a missing snippet degrades that
            // one result instead of failing the search.
            snippet: optional_string_field(item, "content"),
            score: item.get("score").and_then(Value::as_f64),
        });
    }
    Ok(results)
}

/// A result missing one of these cannot be cited, so it fails loudly instead
/// of arriving with a blank field. Applied to `title` and `url` only; see
/// `optional_string_field` for the snippet.
fn string_field(item: &Value, field: &str, index: usize) -> Result<String, SearchError> {
    item.get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| malformed(format!("result {index} has no string `{field}`")))
}

/// Read a field that carries context rather than identity. Absent, null, or
/// non-string all become empty, since none of them makes the hit uncitable.
fn optional_string_field(item: &Value, field: &str) -> String {
    item.get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn malformed(message: impl Into<String>) -> SearchError {
    SearchError::MalformedResponse {
        provider: PROVIDER.to_string(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Env vars are process-wide; serialize the tests that set them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Without an override there is no way to exercise the CLI path against
    /// anything but the live API, which makes the failure scenarios in the
    /// UAT plan untestable and every dry run cost real quota.
    #[test]
    fn from_env_honors_a_base_url_override() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var(TAVILY_API_KEY_VAR, "k");
        std::env::set_var(TAVILY_BASE_URL_VAR, "http://127.0.0.1:9/x");
        let provider = TavilyProvider::from_env().unwrap();
        assert_eq!(provider.base_url(), "http://127.0.0.1:9/x");
        std::env::remove_var(TAVILY_BASE_URL_VAR);
        let provider = TavilyProvider::from_env().unwrap();
        assert_eq!(provider.base_url(), TAVILY_BASE_URL);
        std::env::remove_var(TAVILY_API_KEY_VAR);
    }

    #[test]
    fn redact_removes_the_key_from_a_body() {
        let provider = TavilyProvider::with_key_and_base_url("sekrit", TAVILY_BASE_URL).unwrap();
        let redacted = provider.redact(r#"{"detail":"bad key: sekrit"}"#);
        assert!(!redacted.contains("sekrit"), "leaked: {redacted}");
        assert!(redacted.contains("<redacted>"), "no marker: {redacted}");
    }

    #[test]
    fn parse_results_reads_a_well_formed_array() {
        let value = json!({"results": [
            {"title": "t", "url": "u", "content": "c", "score": 0.5}
        ]});
        let results = parse_results(&value).unwrap();
        assert_eq!(results[0].snippet, "c");
        assert_eq!(results[0].score, Some(0.5));
    }

    /// Tavily can return a hit whose text extraction produced nothing. The
    /// URL is what makes a result citable, so a missing snippet degrades that
    /// one result rather than failing the whole search and leaving validate
    /// with no evidence at all.
    #[test]
    fn parse_results_tolerates_a_missing_snippet_but_keeps_the_other_results() {
        let value = json!({"results": [
            {"title": "a", "url": "u1"},
            {"title": "b", "url": "u2", "content": "text"}
        ]});
        let results = parse_results(&value).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].snippet, "");
        assert_eq!(results[1].snippet, "text");
    }

    /// A result with no URL cannot be cited, so it is still an error.
    #[test]
    fn parse_results_still_rejects_a_result_with_no_url() {
        let value = json!({"results": [{"title": "a", "content": "c"}]});
        assert!(parse_results(&value).is_err());
    }

    #[test]
    fn parse_results_rejects_a_missing_array() {
        let err = parse_results(&json!({"query": "q"})).unwrap_err();
        assert!(matches!(err, SearchError::MalformedResponse { .. }));
    }
}
