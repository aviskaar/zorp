use std::fmt;

#[derive(Debug)]
pub enum DeliverError {
    TrackKilled,
    NoDraft,
    NoVenueTool,
    NoEvidence,
    Paper(zorp_paper::PaperError),
    AgentOutcome(String),
    Io(String),
    Track(zorp_track::TrackError),
}

impl fmt::Display for DeliverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeliverError::TrackKilled => write!(f, "this track has already been killed"),
            DeliverError::NoDraft => write!(
                f,
                "this track has no draft.md yet; run co-write at least once before deliver"
            ),
            DeliverError::NoVenueTool => write!(
                f,
                "no huiban-prefixed tool is available; configure the huiban MCP server (--mcp or .zorp/mcp.toml)"
            ),
            DeliverError::NoEvidence => write!(
                f,
                "this track's evidence record is empty, so there is nothing a paper could cite; run investigate first"
            ),
            DeliverError::Paper(e) => write!(f, "{e}"),
            DeliverError::AgentOutcome(outcome) => write!(f, "agent did not complete: {outcome}"),
            DeliverError::Io(msg) => write!(f, "could not read or write track files: {msg}"),
            DeliverError::Track(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DeliverError {}

impl From<zorp_track::TrackError> for DeliverError {
    fn from(e: zorp_track::TrackError) -> Self {
        DeliverError::Track(e)
    }
}

impl From<zorp_paper::PaperError> for DeliverError {
    fn from(e: zorp_paper::PaperError) -> Self {
        DeliverError::Paper(e)
    }
}

impl From<std::io::Error> for DeliverError {
    fn from(e: std::io::Error) -> Self {
        DeliverError::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_track_killed() {
        assert!(DeliverError::TrackKilled.to_string().contains("killed"));
    }

    #[test]
    fn display_no_draft_mentions_co_write() {
        assert!(DeliverError::NoDraft.to_string().contains("co-write"));
    }

    #[test]
    fn display_no_venue_tool_mentions_huiban() {
        assert!(DeliverError::NoVenueTool.to_string().contains("huiban"));
    }

    #[test]
    fn display_agent_outcome_includes_the_outcome() {
        let e = DeliverError::AgentOutcome("StepLimit".to_string());
        assert!(e.to_string().contains("StepLimit"));
    }

    #[test]
    fn from_io_error_wraps_correctly() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let e: DeliverError = io_err.into();
        assert!(matches!(e, DeliverError::Io(_)));
    }
}
