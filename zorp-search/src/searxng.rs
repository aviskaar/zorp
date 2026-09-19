//! SearXNG, the second provider behind `SearchProvider`.
//!
//! SearXNG is a metasearch engine somebody runs themselves: one GET to
//! `/search` with `format=json`, no account and no key, and a `results`
//! array that already holds a title, a URL and an extract per hit. It is
//! here so web search does not have to mean handing every query to a vendor.
//! Everything SearXNG specific stays in this file, and nothing above the
//! trait changed to add it, which is what the trait's comment promised.
//!
//! This is not a loopback capability and must not be described as one. A
//! local instance happens to listen on loopback, but the same variable can
//! name a public instance, and either way the instance forwards the query to
//! whichever engines its operator enabled. `web_search` is the built-in whose
//! whole job is to leave this machine, and that stays true here. For the same
//! reason the base URL is allowed to be plain HTTP and to resolve to a private
//! address: an instance on the operator's own network is the point of it.

use crate::{Query, SearchError, SearchProvider, SearchResult};
use serde_json::Value;
use std::io::Read;
use std::time::Duration;

/// Where a local instance listens by default. `docker run -p 8888:8080
/// searxng/searxng` puts one exactly here, so the common setup needs no
/// configuration at all.
pub const SEARXNG_BASE_URL: &str = "http://localhost:8888";

/// Overrides the instance. Unset or blank means `SEARXNG_BASE_URL`. Read from
/// the environment and never from a flavor manifest, for the reason the Tavily
/// key is: a workspace file the model can write must not move where queries
/// go.
pub const SEARXNG_BASE_URL_VAR: &str = "ZORP_SEARXNG_BASE_URL";

// There is deliberately no key variable and no `MissingApiKey` for this
// provider, and no new `SearchError` variant either. SearXNG takes no key,
// and with a default base URL there is nothing that can be missing, so
// construction cannot fail. An instance that is not running is found out the
// only honest way, by asking it, and that is `SearchError::Transport`. Adding
// a "not configured" variant back would invent a state this provider does not
// have.

const PROVIDER: &str = "searxng";

/// How much of a non-2xx body goes into the error, as in `tavily.rs`.
const ERROR_BODY_CAP: u64 = 8 * 1024;

/// SearXNG's instance behind the provider trait.
#[derive(Debug)]
pub struct SearxngProvider {
    base_url: String,
    agent: ureq::Agent,
}

impl SearxngProvider {
    /// Read the instance from `ZORP_SEARXNG_BASE_URL`, falling back to
    /// `http://localhost:8888`. Never fails: see the comment above `PROVIDER`.
    pub fn from_env() -> Self {
        let base_url = std::env::var(SEARXNG_BASE_URL_VAR)
            .ok()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| SEARXNG_BASE_URL.to_string());
        Self::with_base_url(base_url)
    }

    /// An explicit instance, for tests and for anyone not using the default.
    ///
    /// Builds its own agent with its own timeouts rather than borrowing one,
    /// the way `TavilyProvider` does and for the reason `docs/DECISIONS.md`
    /// (2026-08-22) gives: an agent with no read timeout is a process that
    /// can sit on a silent socket for hours. The numbers match Tavily's. An
    /// instance fans each query out to several engines and waits on the
    /// slowest, and its own per-engine timeout is a few seconds, so thirty
    /// is generous without being unbounded.
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(30))
            .build();
        SearxngProvider {
            base_url: base_url.into(),
            agent,
        }
    }

    /// The instance this provider will call.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl SearchProvider for SearxngProvider {
    fn name(&self) -> &str {
        PROVIDER
    }

    fn search(&self, query: &Query) -> Result<Vec<SearchResult>, SearchError> {
        let url = format!("{}/search", self.base_url.trim_end_matches('/'));
        // `query` percent-encodes, so the search text cannot break out of
        // its parameter. SearXNG has no result count parameter, it returns a
        // page of whatever its engines gave back, so the cap is applied
        // after parsing instead of being sent.
        let sent = self
            .agent
            .get(&url)
            .query("q", &query.text)
            .query("format", "json")
            .call();
        let response = match sent {
            Ok(response) => response,
            Err(ureq::Error::Status(status, response)) => {
                return Err(SearchError::Status {
                    provider: PROVIDER.to_string(),
                    status,
                    body: status_body(status, read_capped(response)),
                })
            }
            Err(err) => {
                return Err(SearchError::Transport {
                    provider: PROVIDER.to_string(),
                    message: err.to_string(),
                })
            }
        };
        let value: Value = response
            .into_json()
            .map_err(|err| malformed(format!("body is not JSON: {err}")))?;
        let mut results = parse_results(&value)?;
        if let Some(max) = query.max_results {
            results.truncate(max as usize);
        }
        Ok(results)
    }
}

/// The body that goes into a `Status` error. A stock SearXNG install serves
/// HTML only and answers `format=json` with a bare 403, which reads like an
/// authentication problem when it is a one line settings change. So a 403
/// says what to change, alongside whatever the instance sent.
fn status_body(status: u16, body: String) -> String {
    if status != 403 {
        return body;
    }
    let hint = "the instance may not allow JSON output; add `json` to \
                `search.formats` in its settings.yml";
    if body.is_empty() {
        hint.to_string()
    } else {
        format!("{body} ({hint})")
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

/// Turn SearXNG's response into results.
///
/// Same rules as Tavily's parser, for the same reasons: no readable `results`
/// array is an error and not an empty list, a hit with no title or URL cannot
/// be cited and fails loudly, and a missing extract degrades only that hit.
/// `score` is taken when the instance reports a number and left `None` when
/// it does not; SearXNG's scale is its own and orders results, nothing more.
///
/// One rule is SearXNG's own. An empty `results` beside a non-empty
/// `unresponsive_engines` cannot be read as "nothing matched": engines that
/// might have had the answer failed, and nothing came back from the rest.
/// Handing that back as an empty `Vec` would let a caller read an outage as
/// a novel idea, which is the one mistake the trait's "a failed request is
/// an `Err`" exists to prevent. So it is a `Transport` error, which is what
/// it is one hop further out. Results with some engines unresponsive are
/// still results, and come back as such.
fn parse_results(value: &Value) -> Result<Vec<SearchResult>, SearchError> {
    let array = value
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| malformed("no `results` array in the response"))?;
    if array.is_empty() {
        if let Some(failed) = unresponsive_engines(value) {
            return Err(SearchError::Transport {
                provider: PROVIDER.to_string(),
                message: format!("no results, and these engines did not answer: {failed}"),
            });
        }
    }
    let mut results = Vec::with_capacity(array.len());
    for (index, item) in array.iter().enumerate() {
        results.push(SearchResult {
            title: string_field(item, "title", index)?,
            url: string_field(item, "url", index)?,
            // SearXNG's `content` is the engine's extract. Some engines send
            // none for a hit that is still a real, citable page.
            snippet: item
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            score: item.get("score").and_then(Value::as_f64),
        });
    }
    Ok(results)
}

/// The engines an instance reports as having failed, as one readable line,
/// or `None` when it reports none. SearXNG sends each as a two element array
/// of engine name and reason; anything else is shown as it came.
fn unresponsive_engines(value: &Value) -> Option<String> {
    let engines = value.get("unresponsive_engines")?.as_array()?;
    if engines.is_empty() {
        return None;
    }
    let described: Vec<String> = engines
        .iter()
        .map(|engine| match engine.as_array().map(Vec::as_slice) {
            Some([Value::String(name), Value::String(reason)]) => format!("{name} ({reason})"),
            _ => engine.to_string(),
        })
        .collect();
    Some(described.join(", "))
}

fn string_field(item: &Value, field: &str, index: usize) -> Result<String, SearchError> {
    item.get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| malformed(format!("result {index} has no string `{field}`")))
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
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;

    /// A SearXNG `format=json` response, trimmed to two hits but otherwise in
    /// the shape an instance sends: the per-hit `engine`, `engines`,
    /// `positions`, `parsed_url` and `category` fields zorp ignores, and the
    /// top level `answers`, `infoboxes` and `suggestions` it ignores too.
    /// The second hit has no `score` and no `content`, which is what a
    /// result from an engine that reports neither looks like.
    const RECORDED: &str = include_str!("testdata/searxng_response.json");

    /// Env vars are process-wide; serialize the tests that set them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn maps_a_recorded_response() {
        let value: Value = serde_json::from_str(RECORDED).unwrap();
        let results = parse_results(&value).unwrap();
        assert_eq!(results.len(), 2);

        assert_eq!(
            results[0].title,
            "SearXNG, a privacy-respecting metasearch engine"
        );
        assert_eq!(results[0].url, "https://docs.searxng.org/");
        assert!(
            results[0]
                .snippet
                .starts_with("SearXNG is a free internet metasearch"),
            "{:?}",
            results[0].snippet
        );
        assert_eq!(results[0].score, Some(4.0));

        assert_eq!(results[1].url, "https://github.com/searxng/searxng");
        assert_eq!(results[1].score, None, "an instance with no score is None");
        assert_eq!(results[1].snippet, "");
    }

    /// A score that is present but not a number is not a score. The hit is
    /// still a hit.
    #[test]
    fn a_non_numeric_score_is_none_and_the_hit_survives() {
        let value = serde_json::json!({"results": [
            {"title": "t", "url": "u", "content": "c", "score": "high"}
        ]});
        let results = parse_results(&value).unwrap();
        assert_eq!(results[0].score, None);
        assert_eq!(results[0].snippet, "c");
    }

    #[test]
    fn an_honest_empty_answer_is_an_empty_vec() {
        let value = serde_json::json!({"results": [], "unresponsive_engines": []});
        assert!(parse_results(&value).unwrap().is_empty());
    }

    /// Every engine failed, so the empty list means nothing about the query.
    #[test]
    fn no_results_because_every_engine_failed_is_an_error() {
        let value = serde_json::json!({
            "results": [],
            "unresponsive_engines": [["google", "timeout"], ["duckduckgo", "CAPTCHA"]]
        });
        match parse_results(&value).unwrap_err() {
            SearchError::Transport { provider, message } => {
                assert_eq!(provider, "searxng");
                assert!(message.contains("google (timeout)"), "{message}");
                assert!(message.contains("duckduckgo (CAPTCHA)"), "{message}");
            }
            other => panic!("expected Transport, got {other:?}"),
        }
    }

    #[test]
    fn a_result_with_no_url_is_an_error() {
        let value = serde_json::json!({"results": [{"title": "a", "content": "c"}]});
        assert!(matches!(
            parse_results(&value).unwrap_err(),
            SearchError::MalformedResponse { .. }
        ));
    }

    #[test]
    fn a_missing_array_is_an_error() {
        let err = parse_results(&serde_json::json!({"query": "q"})).unwrap_err();
        assert!(matches!(err, SearchError::MalformedResponse { .. }));
    }

    /// Nothing listening is `Transport`, never `Ok(vec![])`. The port comes
    /// from a listener bound and then dropped, so it was free a moment ago
    /// and nothing is on it now.
    #[test]
    fn an_unreachable_instance_is_a_transport_error() {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let provider = SearxngProvider::with_base_url(format!("http://127.0.0.1:{port}"));
        match provider.search(&Query::new("rust")) {
            Err(SearchError::Transport { provider, .. }) => assert_eq!(provider, "searxng"),
            other => panic!("expected Transport, got {other:?}"),
        }
    }

    /// Serve one canned HTTP response on a local socket and hand back the
    /// request line the provider sent, so the test sees the real wire.
    fn one_shot(status: &str, body: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let status = status.to_string();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            // Drain the headers so the client is not writing into a closed
            // socket when the response goes out.
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            request_line
        });
        (base, handle)
    }

    #[test]
    fn a_search_is_a_get_with_the_query_encoded_and_json_asked_for() {
        let (base, server) = one_shot("200 OK", RECORDED);
        // A trailing slash on the base must not produce `//search`.
        let provider = SearxngProvider::with_base_url(format!("{base}/"));
        let results = provider
            .search(&Query::new("rust & zorp").with_max_results(1))
            .unwrap();
        let request = server.join().unwrap();

        assert!(request.starts_with("GET /search?"), "{request}");
        assert!(
            request.contains("q=rust+%26+zorp") || request.contains("q=rust%20%26%20zorp"),
            "{request}"
        );
        assert!(request.contains("format=json"), "{request}");
        // SearXNG has no count parameter, so the cap is applied here.
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://docs.searxng.org/");
    }

    #[test]
    fn a_403_says_how_to_turn_json_on() {
        let (base, server) = one_shot("403 Forbidden", "");
        let provider = SearxngProvider::with_base_url(base);
        let err = provider.search(&Query::new("q")).unwrap_err();
        server.join().unwrap();
        match &err {
            SearchError::Status { status, body, .. } => {
                assert_eq!(*status, 403);
                assert!(body.contains("search.formats"), "{body}");
            }
            other => panic!("expected Status, got {other:?}"),
        }
    }

    #[test]
    fn from_env_defaults_to_localhost_and_honors_an_override() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var(SEARXNG_BASE_URL_VAR);
        assert_eq!(SearxngProvider::from_env().base_url(), SEARXNG_BASE_URL);
        std::env::set_var(SEARXNG_BASE_URL_VAR, "   ");
        assert_eq!(SearxngProvider::from_env().base_url(), SEARXNG_BASE_URL);
        std::env::set_var(SEARXNG_BASE_URL_VAR, "http://10.0.0.5:8080");
        assert_eq!(
            SearxngProvider::from_env().base_url(),
            "http://10.0.0.5:8080"
        );
        std::env::remove_var(SEARXNG_BASE_URL_VAR);
    }

    #[test]
    fn name_is_searxng() {
        assert_eq!(
            SearxngProvider::with_base_url(SEARXNG_BASE_URL).name(),
            "searxng"
        );
    }
}
