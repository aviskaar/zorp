use std::fmt;

#[derive(Debug)]
pub enum ReviewError {
    TrackKilled,
    NoPaper(String),
    /// The caller named a dimension that does not exist. Refused rather
    /// than ignored: silently running a smaller review than was asked for
    /// is the failure this whole capability is trying not to commit.
    UnknownDimension(String),
    Io(String),
    Track(zorp_track::TrackError),
}

impl fmt::Display for ReviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReviewError::TrackKilled => write!(f, "this track has already been killed"),
            ReviewError::NoPaper(path) => write!(
                f,
                "no paper to review at {path}; pass --paper, or run co-write to produce a draft.md"
            ),
            ReviewError::UnknownDimension(msg) => write!(f, "{msg}"),
            ReviewError::Io(msg) => write!(f, "could not read or write review files: {msg}"),
            ReviewError::Track(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ReviewError {}

impl From<zorp_track::TrackError> for ReviewError {
    fn from(e: zorp_track::TrackError) -> Self {
        ReviewError::Track(e)
    }
}

impl From<std::io::Error> for ReviewError {
    fn from(e: std::io::Error) -> Self {
        ReviewError::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_track_killed() {
        assert!(ReviewError::TrackKilled.to_string().contains("killed"));
    }

    #[test]
    fn display_no_paper_says_how_to_get_one() {
        let e = ReviewError::NoPaper(".zorp/tracks/t1/draft.md".to_string());
        assert!(e.to_string().contains("--paper"));
        assert!(e.to_string().contains("co-write"));
    }

    #[test]
    fn display_unknown_dimension_passes_the_message_through() {
        let e = ReviewError::UnknownDimension("unknown review dimension 'nope'".to_string());
        assert!(e.to_string().contains("nope"));
    }

    #[test]
    fn from_io_error_wraps_correctly() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let e: ReviewError = io_err.into();
        assert!(matches!(e, ReviewError::Io(_)));
    }
}
