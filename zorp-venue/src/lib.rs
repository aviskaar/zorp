//! Venue formatting and submission conformance for zorp.
//!
//! zorp's `deliver` capability already matches a finished draft against
//! real venues and writes a ranked shortlist. This crate is the step after
//! that: given a draft and one target venue, say what would get the paper
//! desk-rejected, and produce a manuscript in the venue's required form.
//!
//! Three things shape the design.
//!
//! **Venue requirements are data.** Adding a venue is a TOML file in
//! `profiles/`, or in a user's own `~/.config/zorp/venues/` or the
//! project's `.zorp/venues/`, layered the way flavors layer. It is never a
//! code change.
//!
//! **Nothing here may invent a requirement.** Every rule carries the URL
//! it was read off and the date it was read. A rule with no source is
//! reported as unverified rather than counted as compliance, and a profile
//! past its staleness limit says so at the top of every report. A
//! confident, wrong "this venue allows nine pages" is exactly the failure
//! zorp exists to prevent, and here it hands the author a desk rejection
//! while telling them they were fine.
//!
//! **A check is a pure function.** No model, no network, no clock except
//! the date a caller passes in. That is what makes the anonymisation
//! checks testable adversarially, which is the only way to trust them.

pub mod check;
pub mod date;
pub mod latex;
pub mod manuscript;
pub mod profile;
pub mod report;

pub use check::Inputs;
pub use date::Date;
pub use manuscript::Manuscript;
pub use profile::{
    CheckKind, CycleState, Freshness, ProfileError, Rule, Severity, VenueProfile,
};
pub use report::{Counts, Finding, Report, Verdict};

/// Load a profile and check a draft against it.
pub fn conform(
    home: &std::path::Path,
    cwd: &std::path::Path,
    venue_id: &str,
    manuscript: &Manuscript,
    inputs: &Inputs,
    today: Date,
) -> Result<Report, ProfileError> {
    let profile = profile::load(home, cwd, venue_id)?;
    Ok(check::run(&profile, manuscript, inputs, today))
}
