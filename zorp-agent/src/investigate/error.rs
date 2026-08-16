use super::result::ParseError;
use std::fmt;

#[derive(Debug)]
pub enum InvestigateError {
    TrackKilled,
    PreregRequired {
        missing: &'static str,
    },
    PreregMismatch {
        field: &'static str,
        recorded: String,
        provided: String,
    },
    AgentOutcome(String),
    Scoring(ParseError),
    Track(zorp_track::TrackError),
}

impl fmt::Display for InvestigateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InvestigateError::TrackKilled => write!(f, "this track has already been killed"),
            InvestigateError::PreregRequired { missing } => write!(
                f,
                "no pre-registration exists for this track yet; pass --{missing} on the first investigate call"
            ),
            InvestigateError::PreregMismatch { field, recorded, provided } => write!(
                f,
                "--{field} ({provided}) does not match the track's recorded pre-registration ({recorded})"
            ),
            InvestigateError::AgentOutcome(outcome) => write!(f, "agent did not complete: {outcome}"),
            InvestigateError::Scoring(e) => write!(f, "could not score the attempt: {e}"),
            InvestigateError::Track(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for InvestigateError {}

impl From<ParseError> for InvestigateError {
    fn from(e: ParseError) -> Self {
        InvestigateError::Scoring(e)
    }
}

impl From<zorp_track::TrackError> for InvestigateError {
    fn from(e: zorp_track::TrackError) -> Self {
        InvestigateError::Track(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_track_killed() {
        assert!(InvestigateError::TrackKilled.to_string().contains("killed"));
    }

    #[test]
    fn display_prereg_required_names_the_missing_flag() {
        let e = InvestigateError::PreregRequired {
            missing: "metric-name",
        };
        assert!(e.to_string().contains("--metric-name"));
    }

    /// `missing` carries one flag in some call paths and a list in others, so
    /// the sentence cannot agree in number with either. It should not try.
    #[test]
    fn display_prereg_required_reads_correctly_for_one_flag_and_for_several() {
        let one = InvestigateError::PreregRequired {
            missing: "metric-name",
        }
        .to_string();
        let many = InvestigateError::PreregRequired {
            missing: "metric-name, --kill-threshold, and --threshold-direction",
        }
        .to_string();
        for message in [&one, &many] {
            assert!(
                !message.contains(" is required") && !message.contains(" are required"),
                "number agreement cannot hold for both cases: {message}"
            );
        }
    }

    #[test]
    fn display_prereg_mismatch_names_both_values() {
        let e = InvestigateError::PreregMismatch {
            field: "kill-threshold",
            recorded: "100".to_string(),
            provided: "50".to_string(),
        };
        let s = e.to_string();
        assert!(s.contains("100") && s.contains("50"));
    }

    #[test]
    fn from_parse_error_wraps_correctly() {
        let e: InvestigateError = ParseError::NoFencedBlock.into();
        assert!(matches!(
            e,
            InvestigateError::Scoring(ParseError::NoFencedBlock)
        ));
    }
}
