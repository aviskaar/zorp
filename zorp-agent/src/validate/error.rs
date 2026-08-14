use super::ParseError;
use std::fmt;

#[derive(Debug)]
pub enum ValidateError {
    NoSearchTool,
    AgentOutcome(String),
    Scoring(ParseError),
    Track(zorp_track::TrackError),
    Embedding(String),
}

impl fmt::Display for ValidateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidateError::NoSearchTool => write!(
                f,
                "no search-capable tool is available; configure an MCP search server (--mcp or .zorp/mcp.toml)"
            ),
            ValidateError::AgentOutcome(outcome) => write!(f, "agent did not complete: {outcome}"),
            ValidateError::Scoring(e) => write!(f, "could not score the search results: {e}"),
            ValidateError::Track(e) => write!(f, "{e}"),
            ValidateError::Embedding(msg) => write!(f, "could not embed a cited source: {msg}"),
        }
    }
}

impl std::error::Error for ValidateError {}

impl From<ParseError> for ValidateError {
    fn from(e: ParseError) -> Self {
        ValidateError::Scoring(e)
    }
}

impl From<zorp_track::TrackError> for ValidateError {
    fn from(e: zorp_track::TrackError) -> Self {
        ValidateError::Track(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_no_search_tool_mentions_mcp() {
        let e = ValidateError::NoSearchTool;
        assert!(e.to_string().contains("MCP"));
    }

    #[test]
    fn display_agent_outcome_includes_the_outcome() {
        let e = ValidateError::AgentOutcome("StepLimit".to_string());
        assert!(e.to_string().contains("StepLimit"));
    }

    #[test]
    fn from_parse_error_wraps_correctly() {
        let e: ValidateError = ParseError::NoFencedBlock.into();
        assert!(matches!(
            e,
            ValidateError::Scoring(ParseError::NoFencedBlock)
        ));
    }
}
