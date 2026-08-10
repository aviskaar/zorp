use std::fmt;

#[derive(Debug)]
pub enum CoWriteError {
    TrackKilled,
    NoMetrics,
    AgentOutcome(String),
    Io(String),
    Track(zorp_track::TrackError),
}

impl fmt::Display for CoWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoWriteError::TrackKilled => write!(f, "this track has already been killed"),
            CoWriteError::NoMetrics => write!(
                f,
                "this track has no recorded metrics yet; run investigate at least once before co-write"
            ),
            CoWriteError::AgentOutcome(outcome) => write!(f, "agent did not complete: {outcome}"),
            CoWriteError::Io(msg) => write!(f, "could not write draft.md: {msg}"),
            CoWriteError::Track(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CoWriteError {}

impl From<zorp_track::TrackError> for CoWriteError {
    fn from(e: zorp_track::TrackError) -> Self {
        CoWriteError::Track(e)
    }
}

impl From<std::io::Error> for CoWriteError {
    fn from(e: std::io::Error) -> Self {
        CoWriteError::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_track_killed() {
        assert!(CoWriteError::TrackKilled.to_string().contains("killed"));
    }

    #[test]
    fn display_no_metrics_mentions_investigate() {
        assert!(CoWriteError::NoMetrics.to_string().contains("investigate"));
    }

    #[test]
    fn display_agent_outcome_includes_the_outcome() {
        let e = CoWriteError::AgentOutcome("StepLimit".to_string());
        assert!(e.to_string().contains("StepLimit"));
    }

    #[test]
    fn from_io_error_wraps_correctly() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let e: CoWriteError = io_err.into();
        assert!(matches!(e, CoWriteError::Io(_)));
    }
}
