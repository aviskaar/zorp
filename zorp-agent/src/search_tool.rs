//! The `web_search` tool: a thin adapter over a `zorp_search::SearchProvider`.
//!
//! This is the only built-in that sends anything over the network, which is
//! why it lives behind the non-default `search` feature and why
//! `Policy::decide` gates it at `Ask` rather than allowing it like a local
//! read. The provider itself lives in the `zorp-search` crate and knows
//! nothing about tools, agents, or approval.

use crate::tools::{cap_output, Context, Tool, ToolError, ToolOutput, ToolResult};
use serde_json::{json, Value};
use zorp_search::{Query, SearchProvider, SearchResult};

/// Cap on results requested when the model does not say. Enough to judge
/// novelty, small enough to keep the transcript readable.
const DEFAULT_MAX_RESULTS: u32 = 5;

/// Ceiling on the rendered result text handed back to the model.
const MAX_CONTENT_BYTES: usize = 16_000;

pub struct WebSearch {
    provider: Box<dyn SearchProvider + Send + Sync>,
}

impl WebSearch {
    pub fn new(provider: Box<dyn SearchProvider + Send + Sync>) -> Self {
        WebSearch { provider }
    }
}

impl Tool for WebSearch {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web for current information and prior work. Returns \
         titles, URLs, and extracted snippets. Cite the URL when you use a \
         result."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type":"string","description":"what to search for"},
                "max_results": {
                    "type":"integer",
                    "description":"how many results to return (default 5)"
                }
            },
            "required": ["query"]
        })
    }

    fn run(&self, args: &Value, _cx: &mut Context) -> ToolResult {
        let text = args
            .get("query")
            .and_then(Value::as_str)
            .filter(|q| !q.trim().is_empty())
            .ok_or_else(|| ToolError::new("web_search: 'query' is required"))?;

        let max = args
            .get("max_results")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(DEFAULT_MAX_RESULTS);

        let results = self
            .provider
            .search(&Query::new(text).with_max_results(max))
            // A failed search is never reported as "no results". An empty
            // result set and a broken request mean different things to a
            // novelty score, and conflating them records a wrong number.
            .map_err(|e| ToolError::new(e.to_string()))?;

        let n = results.len();
        let content = if results.is_empty() {
            "no results".to_string()
        } else {
            results.iter().map(render).collect::<Vec<_>>().join("\n\n")
        };

        Ok(ToolOutput::new(
            cap_output(&content, MAX_CONTENT_BYTES),
            format!("'{text}' ({n} results)"),
        ))
    }
}

fn render(r: &SearchResult) -> String {
    let mut out = format!("{}\n{}", r.title, r.url);
    if !r.snippet.is_empty() {
        out.push('\n');
        out.push_str(&r.snippet);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::cancel_token;
    use zorp_search::SearchError;

    /// `SearchError` is deliberately not `Clone` (it carries response text),
    /// so the stub rebuilds the failure on each call instead of holding one.
    enum Stub {
        Hits(Vec<SearchResult>),
        Fails,
    }

    impl SearchProvider for Stub {
        fn name(&self) -> &str {
            "stub"
        }
        fn search(&self, _query: &Query) -> Result<Vec<SearchResult>, SearchError> {
            match self {
                Stub::Hits(v) => Ok(v.clone()),
                Stub::Fails => Err(SearchError::Status {
                    provider: "tavily".to_string(),
                    status: 429,
                    body: String::new(),
                }),
            }
        }
    }

    fn cx() -> Context {
        Context::new(std::path::PathBuf::from("."), cancel_token())
    }

    fn hit(title: &str, url: &str, snippet: &str) -> SearchResult {
        SearchResult {
            title: title.to_string(),
            url: url.to_string(),
            snippet: snippet.to_string(),
            score: None,
        }
    }

    fn tool(results: Vec<SearchResult>) -> WebSearch {
        WebSearch::new(Box::new(Stub::Hits(results)))
    }

    #[test]
    fn results_are_rendered_with_titles_and_urls() {
        let t = tool(vec![hit("Kafka", "https://example.com/a", "a broker")]);
        let out = t.run(&json!({"query": "kafka"}), &mut cx()).unwrap();
        assert!(out.content.contains("https://example.com/a"));
        assert!(out.content.contains("a broker"));
        assert_eq!(out.summary, "'kafka' (1 results)");
    }

    #[test]
    fn a_missing_query_is_a_tool_error() {
        let t = tool(vec![]);
        assert!(t.run(&json!({}), &mut cx()).is_err());
    }

    #[test]
    fn a_blank_query_is_a_tool_error() {
        let t = tool(vec![]);
        assert!(t.run(&json!({"query": "   "}), &mut cx()).is_err());
    }

    /// The distinction the whole feature rests on: a provider failure must
    /// reach the model as an error, not as an empty result set.
    #[test]
    fn a_provider_failure_is_an_error_not_an_empty_result_set() {
        let t = WebSearch::new(Box::new(Stub::Fails));
        // ToolOutput has no Debug, so match rather than unwrap_err.
        match t.run(&json!({"query": "kafka"}), &mut cx()) {
            Ok(out) => panic!("a failed search must not succeed: {}", out.summary),
            Err(e) => assert!(e.message.contains("429"), "{}", e.message),
        }
    }

    #[test]
    fn an_empty_result_set_says_so_and_is_not_an_error() {
        let t = tool(vec![]);
        let out = t.run(&json!({"query": "kafka"}), &mut cx()).unwrap();
        assert_eq!(out.content, "no results");
        assert_eq!(out.summary, "'kafka' (0 results)");
    }

    #[test]
    fn a_result_with_no_snippet_still_renders_its_url() {
        let t = tool(vec![hit("Kafka", "https://example.com/a", "")]);
        let out = t.run(&json!({"query": "kafka"}), &mut cx()).unwrap();
        assert!(out.content.contains("https://example.com/a"));
    }
}
