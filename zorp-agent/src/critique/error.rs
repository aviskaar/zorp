use std::fmt;

#[derive(Debug)]
pub enum CritiqueError {
    TrackKilled,
    NoDraft,
    NoEvidence,
    AgentOutcome(String),
    Parse(super::claims::ParseError),
    /// The run record changed while the pass was running. The pass reads
    /// the record and writes only the draft, so this means something the
    /// agent did reached the record, and the revision is thrown away
    /// rather than trusted.
    RecordMutated {
        what: &'static str,
    },
    Io(String),
    Track(zorp_track::TrackError),
}

impl fmt::Display for CritiqueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CritiqueError::TrackKilled => write!(f, "this track has already been killed"),
            CritiqueError::NoDraft => write!(
                f,
                "this track has no draft.md yet; run co-write before critique"
            ),
            CritiqueError::NoEvidence => write!(
                f,
                "this track has recorded no evidence yet, so there is nothing to check the draft against; run investigate at least once"
            ),
            CritiqueError::AgentOutcome(outcome) => write!(f, "agent did not complete: {outcome}"),
            CritiqueError::Parse(e) => write!(f, "{e}"),
            CritiqueError::RecordMutated { what } => write!(
                f,
                "the run record changed while critique was running ({what}); the draft was left alone. Only a human moves pre-registered intent"
            ),
            CritiqueError::Io(msg) => write!(f, "could not write the critique: {msg}"),
            CritiqueError::Track(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CritiqueError {}

impl From<zorp_track::TrackError> for CritiqueError {
    fn from(e: zorp_track::TrackError) -> Self {
        CritiqueError::Track(e)
    }
}

impl From<std::io::Error> for CritiqueError {
    fn from(e: std::io::Error) -> Self {
        CritiqueError::Io(e.to_string())
    }
}

impl From<super::claims::ParseError> for CritiqueError {
    fn from(e: super::claims::ParseError) -> Self {
        CritiqueError::Parse(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_no_draft_mentions_co_write() {
        assert!(CritiqueError::NoDraft.to_string().contains("co-write"));
    }

    #[test]
    fn display_no_evidence_mentions_investigate() {
        assert!(CritiqueError::NoEvidence
            .to_string()
            .contains("investigate"));
    }

    #[test]
    fn display_record_mutated_says_the_draft_was_not_touched() {
        let e = CritiqueError::RecordMutated {
            what: "kill threshold",
        };
        let msg = e.to_string();
        assert!(msg.contains("kill threshold"), "{msg}");
        assert!(msg.contains("left alone"), "{msg}");
    }

    #[test]
    fn from_io_error_wraps_correctly() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let e: CritiqueError = io_err.into();
        assert!(matches!(e, CritiqueError::Io(_)));
    }
}
