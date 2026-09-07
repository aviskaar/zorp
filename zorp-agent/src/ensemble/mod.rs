//! Ensemble: one model does the work, other models test it, and the
//! findings go back to the first model for a revision. A DAG with a return
//! edge, driven by code.
//!
//! Reuses `panel` for lenses, verdict parsing and agreement counting. What
//! it adds: a reviewer that may run commands, a hash check in code that the
//! reviewer changed nothing, the return edge to the main model, memoized
//! verdicts, a findings ledger, pruning on code-visible failure, and a
//! record per run. See
//! `docs/superpowers/specs/2026-09-05-ensemble-dag-design.md` and
//! `docs/DECISIONS.md` (2026-09-05).
//!
//! Two rules are not negotiable. Code launches every run and review here;
//! no tool starts one, and `agent.rs` has a test saying so. And no roster
//! changes on a model's opinion: a reviewer is dropped for altering an
//! output, or for two unusable replies, and for nothing else.

pub mod hashes;
pub mod ledger;
pub mod record;

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::panel::Lens;

/// Rounds of review and revision, when the roster does not say.
pub const DEFAULT_ROUNDS: usize = 2;
/// Steps a reviewer may take. Lower than a working agent's on purpose: a
/// review that needs sixty steps is doing the task over.
pub const DEFAULT_REVIEW_STEPS: usize = 20;
/// Names the roster file. Roles come from here and never from the
/// instruction text.
pub const ROSTER_VAR: &str = "ZORP_ENSEMBLE";
pub const REVIEW_STEPS_VAR: &str = "ZORP_ENSEMBLE_REVIEW_STEPS";
/// Where reviewer transcripts and the record go. Defaults to
/// `<cwd>/scratch/ensemble/<run-id>/`.
pub const LOG_DIR_VAR: &str = "ZORP_ENSEMBLE_LOG_DIR";

/// Who plays which role. One lens per reviewer, in this order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Roster {
    pub main: String,
    pub reviewers: Vec<String>,
    pub rounds: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RosterFile {
    main: Role,
    #[serde(default)]
    reviewer: Vec<Role>,
    #[serde(default = "default_rounds")]
    rounds: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Role {
    model: String,
}

fn default_rounds() -> usize {
    DEFAULT_ROUNDS
}

impl Roster {
    pub fn parse(text: &str) -> Result<Roster, String> {
        let file: RosterFile = toml::from_str(text).map_err(|e| format!("roster: {e}"))?;
        if file.main.model.trim().is_empty() {
            return Err("roster: [main] model is empty".to_string());
        }
        let reviewers: Vec<String> = file.reviewer.into_iter().map(|r| r.model).collect();
        if reviewers.is_empty() {
            return Err("roster: at least one [[reviewer]] is required".to_string());
        }
        let lens_count = lenses().len();
        if reviewers.len() > lens_count {
            return Err(format!(
                "roster: {} reviewers but only {} lenses; one lens per reviewer",
                reviewers.len(),
                lens_count
            ));
        }
        if file.rounds == 0 {
            return Err("roster: rounds must be at least 1".to_string());
        }
        Ok(Roster {
            main: file.main.model,
            reviewers,
            rounds: file.rounds,
        })
    }

    pub fn load(path: &Path) -> Result<Roster, String> {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("roster {}: {e}", path.display()))?;
        Roster::parse(&text)
    }
}

/// The three angles, code-defined, assigned one per reviewer in roster
/// order. A corroborated finding was then reached from two angles by two
/// models, which is the only agreement worth counting.
pub fn lenses() -> Vec<Lens> {
    vec![
        Lens::new(
            "contract",
            "Every output the instruction requires must exist at its path, in the \
named format, with the named columns, keys and units. Read the instruction, list \
what it requires, then check each requirement against the workspace. Report each \
output that is missing, misnamed, in the wrong format, or that lacks a column, key \
or unit the instruction asked for. Name the output path in `locus`.",
        ),
        Lens::new(
            "reproduction",
            "Pick the key number, table or figure the instruction asks for and recompute \
it from the data by a route the main run did not take: a different tool, a \
different library, or a hand calculation over a subset. Compare it with what was \
submitted. Report a disagreement with both values and how you got yours. Name the \
output path in `locus`. If you cannot reproduce it, say what stopped you and \
report nothing else.",
        ),
        Lens::new(
            "adversary",
            "Assume the submitted work is wrong and try to show it. Check assumptions \
the instruction did not license, off-by-one and index errors, unit and scale \
mistakes, a wrong column, a default a library applied silently, and edge cases in \
the data such as empty groups, missing values, duplicates and ties. Run the \
checks; do not reason about them from the code alone. Report what broke and how. \
Name the file in `locus`.",
        ),
    ]
}

/// The panel's read-only allow-list plus a shell. The verifier's tests are
/// hidden and the only way to test is to run checks in the workspace. A
/// shell can write, so the check that it did not is in code: see
/// `hashes` and the loop in `run`.
pub fn reviewer_tools() -> Vec<String> {
    let mut tools = crate::panel::reviewer_tools();
    tools.push("run_command".to_string());
    tools
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC_ROSTER: &str = r#"
rounds = 2

[main]
model = "nvidia/nemotron-3-super-120b-a12b:free"

[[reviewer]]
model = "minimax/minimax-m3:free"
[[reviewer]]
model = "dots-studio/dots-3-note-preview:free"
[[reviewer]]
model = "minimax/minimax-m2.7:free"
"#;

    #[test]
    fn the_spec_roster_parses_in_order() {
        let r = Roster::parse(SPEC_ROSTER).unwrap();
        assert_eq!(r.main, "nvidia/nemotron-3-super-120b-a12b:free");
        assert_eq!(
            r.reviewers,
            vec![
                "minimax/minimax-m3:free",
                "dots-studio/dots-3-note-preview:free",
                "minimax/minimax-m2.7:free"
            ]
        );
        assert_eq!(r.rounds, 2);
    }

    #[test]
    fn rounds_default_to_two() {
        let r = Roster::parse("[main]\nmodel = \"a\"\n[[reviewer]]\nmodel = \"b\"\n").unwrap();
        assert_eq!(r.rounds, DEFAULT_ROUNDS);
    }

    #[test]
    fn more_reviewers_than_lenses_is_refused_by_name() {
        let text = "[main]\nmodel = \"a\"\n[[reviewer]]\nmodel = \"b\"\n[[reviewer]]\nmodel = \"c\"\n[[reviewer]]\nmodel = \"d\"\n[[reviewer]]\nmodel = \"e\"\n";
        let err = Roster::parse(text).unwrap_err();
        assert!(err.contains("4 reviewers"), "{err}");
        assert!(err.contains("3 lenses"), "{err}");
    }

    #[test]
    fn a_roster_needs_a_reviewer_and_a_round() {
        assert!(Roster::parse("[main]\nmodel = \"a\"\n").is_err());
        assert!(
            // `rounds` must come before any table header: TOML scopes a
            // bare key to the most recently opened table, so placed after
            // `[[reviewer]]` it would land on that reviewer, not the root.
            Roster::parse("rounds = 0\n[main]\nmodel = \"a\"\n[[reviewer]]\nmodel = \"b\"\n")
                .is_err()
        );
        assert!(Roster::parse("[main]\nmodel = \"\"\n[[reviewer]]\nmodel = \"b\"\n").is_err());
    }

    #[test]
    fn the_lenses_are_contract_reproduction_adversary() {
        let names: Vec<String> = lenses().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["contract", "reproduction", "adversary"]);
    }

    #[test]
    fn reviewer_tools_are_the_panels_plus_a_shell() {
        let mut expected = crate::panel::reviewer_tools();
        expected.push("run_command".to_string());
        assert_eq!(reviewer_tools(), expected);
    }

    #[test]
    fn a_rounds_key_after_a_reviewer_table_is_refused() {
        let text = "[main]\nmodel = \"a\"\n[[reviewer]]\nmodel = \"b\"\nrounds = 4\n";
        let err = Roster::parse(text).unwrap_err();
        assert!(err.contains("rounds"), "{err}");
    }
}
