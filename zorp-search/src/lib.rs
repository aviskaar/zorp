//! Web search for zorp: a provider trait and the types it exchanges.
//!
//! This crate knows about HTTP and JSON and nothing else. It has no
//! dependency on `zorp-agent`, so adding a provider never touches the
//! harness, and the harness's tool adapter is the only place that knows a
//! search is a tool call.

mod tavily;

pub use tavily::{TavilyProvider, TAVILY_API_KEY_VAR, TAVILY_BASE_URL, TAVILY_BASE_URL_VAR};

use std::fmt;

/// A search backend. Blocking, because the whole harness is.
pub trait SearchProvider {
    /// Stable, lowercase identifier, used in errors and activity lines.
    fn name(&self) -> &str;

    /// Run one search. An empty `Vec` means the provider found nothing. A
    /// failed request is an `Err`, never an empty `Vec`.
    fn search(&self, query: &Query) -> Result<Vec<SearchResult>, SearchError>;
}

/// What to search for. Provider specific knobs (Tavily's `search_depth`,
/// topic filters, and so on) stay on the provider, not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// The search string.
    pub text: String,
    /// Cap on how many results to return. `None` leaves it to the provider.
    pub max_results: Option<u32>,
}

impl Query {
    /// A query with no result cap.
    pub fn new(text: impl Into<String>) -> Self {
        Query {
            text: text.into(),
            max_results: None,
        }
    }

    /// Ask the provider for at most `max` results.
    pub fn with_max_results(mut self, max: u32) -> Self {
        self.max_results = Some(max);
        self
    }
}

/// One hit. This is the intersection of what Tavily, Brave, and Exa return,
/// so a second provider needs no change above the trait.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    /// The provider's extract of the page. Tavily calls this `content`.
    pub snippet: String,
    /// Provider relevance score, if it reports one. Scales differ between
    /// providers, so it orders results and means nothing on its own.
    pub score: Option<f64>,
}

/// Why a search did not produce results. Every variant names the provider so
/// the message still makes sense once it is a tool result. No variant carries
/// the API key.
#[non_exhaustive]
#[derive(Debug)]
pub enum SearchError {
    /// No key was configured, so no request was attempted.
    MissingApiKey { provider: String, var: String },
    /// The provider answered with a non-2xx status.
    Status {
        provider: String,
        status: u16,
        body: String,
    },
    /// The request never completed: connection refused, TLS failure, timeout.
    Transport { provider: String, message: String },
    /// The provider answered, but not with something readable as results.
    MalformedResponse { provider: String, message: String },
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchError::MissingApiKey { provider, var } => {
                write!(f, "{provider} search: no API key; set {var}")
            }
            SearchError::Status {
                provider,
                status,
                body,
            } => {
                if body.is_empty() {
                    write!(f, "{provider} search: HTTP status {status}")
                } else {
                    write!(f, "{provider} search: HTTP status {status}: {body}")
                }
            }
            SearchError::Transport { provider, message } => {
                write!(f, "{provider} search: request failed: {message}")
            }
            SearchError::MalformedResponse { provider, message } => {
                write!(f, "{provider} search: malformed response: {message}")
            }
        }
    }
}

impl std::error::Error for SearchError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_missing_api_key_names_the_provider_and_the_variable() {
        let err = SearchError::MissingApiKey {
            provider: "tavily".into(),
            var: "ZORP_TAVILY_API_KEY".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("tavily"), "{msg}");
        assert!(msg.contains("ZORP_TAVILY_API_KEY"), "{msg}");
    }

    #[test]
    fn display_status_names_the_provider_and_the_status() {
        let err = SearchError::Status {
            provider: "tavily".into(),
            status: 401,
            body: "unauthorized".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("tavily"), "{msg}");
        assert!(msg.contains("401"), "{msg}");
        assert!(msg.contains("unauthorized"), "{msg}");
    }

    #[test]
    fn display_status_omits_an_empty_body() {
        let err = SearchError::Status {
            provider: "tavily".into(),
            status: 502,
            body: String::new(),
        };
        assert_eq!(err.to_string(), "tavily search: HTTP status 502");
    }

    #[test]
    fn display_transport_names_the_provider_and_the_condition() {
        let err = SearchError::Transport {
            provider: "tavily".into(),
            message: "connection refused".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("tavily"), "{msg}");
        assert!(msg.contains("connection refused"), "{msg}");
    }

    #[test]
    fn display_malformed_names_the_provider_and_the_condition() {
        let err = SearchError::MalformedResponse {
            provider: "tavily".into(),
            message: "no `results` array".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("tavily"), "{msg}");
        assert!(msg.contains("results"), "{msg}");
    }

    #[test]
    fn query_carries_an_optional_result_cap() {
        assert_eq!(Query::new("q").max_results, None);
        assert_eq!(Query::new("q").with_max_results(5).max_results, Some(5));
    }
}
