use std::fmt;

#[non_exhaustive]
#[derive(Debug)]
pub enum TrackError {
    Io(String),
    Db(String),
    Library(String),
    NotFound {
        kind: &'static str,
        id: String,
    },
    IntegrityMismatch {
        track_id: String,
        detail: String,
    },
    CheckpointBlocked {
        kind: String,
    },
    AlreadyRegistered {
        track_id: String,
    },
    ExpectationAfterOutcome {
        experiment_id: String,
        metric_key: String,
    },
    Malformed {
        what: &'static str,
        detail: String,
    },
}

impl fmt::Display for TrackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrackError::Io(msg) => write!(f, "zorp-track io error: {msg}"),
            TrackError::Db(msg) => write!(f, "zorp-track db error: {msg}"),
            TrackError::Library(msg) => write!(f, "zorp-track library error: {msg}"),
            TrackError::NotFound { kind, id } => write!(f, "{kind} not found: {id}"),
            TrackError::IntegrityMismatch { track_id, detail } => {
                write!(f, "prereg integrity mismatch for track '{track_id}': {detail}")
            }
            TrackError::CheckpointBlocked { kind } => write!(
                f,
                "checkpoint '{kind}' has no interactive terminal and AutoApprove is not set"
            ),
            TrackError::AlreadyRegistered { track_id } => write!(
                f,
                "track '{track_id}' is already pre-registered; write_prereg cannot be called twice for the same track"
            ),
            TrackError::ExpectationAfterOutcome { experiment_id, metric_key } => write!(
                f,
                "experiment '{experiment_id}' already recorded metric '{metric_key}'; an expectation written now would be a postdiction"
            ),
            TrackError::Malformed { what, detail } => {
                write!(f, "malformed {what}: {detail}")
            }
        }
    }
}

impl std::error::Error for TrackError {}

impl From<duckdb::Error> for TrackError {
    fn from(e: duckdb::Error) -> Self {
        TrackError::Db(e.to_string())
    }
}

impl From<std::io::Error> for TrackError {
    fn from(e: std::io::Error) -> Self {
        TrackError::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_not_found() {
        let e = TrackError::NotFound {
            kind: "track",
            id: "t1".into(),
        };
        assert!(e.to_string().contains("track"));
        assert!(e.to_string().contains("t1"));
    }

    #[test]
    fn display_integrity_mismatch() {
        let e = TrackError::IntegrityMismatch {
            track_id: "t1".into(),
            detail: "hash mismatch".into(),
        };
        assert!(e.to_string().contains("t1"));
        assert!(e.to_string().contains("hash mismatch"));
    }

    #[test]
    fn display_checkpoint_blocked() {
        let e = TrackError::CheckpointBlocked {
            kind: "validate".into(),
        };
        assert!(e.to_string().contains("validate"));
        assert!(e.to_string().contains("AutoApprove"));
    }
}
