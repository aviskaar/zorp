//! When the review stops.
//!
//! "Until the reviewer is satisfied" is not a stopping condition. A
//! reviewer asked "anything else?" will always produce something, so
//! satisfaction never arrives and the loop never ends. This module
//! replaces it with two bounds the reviewer cannot argue with: stop after
//! `quiet_rounds` consecutive rounds that produce no finding not already
//! seen, and stop unconditionally at `max_rounds`.

use super::finding::{normalize, Finding};

/// Why the loop stopped. The report prints this verbatim, because a
/// review that ran out of rounds is not the same as a review that found
/// nothing more to say, and presenting them the same way would turn a
/// truncated pass into a clean bill of health.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Stop {
    /// `quiet_rounds` consecutive rounds produced nothing new.
    Converged { quiet_rounds: usize },
    /// The round cap was reached with findings still appearing.
    RoundCap { max_rounds: usize },
    /// The agent budget ran out before the loop could converge.
    BudgetExhausted { rounds_run: usize },
}

impl Stop {
    /// Whether the review got to the end of what it had to say. False
    /// means coverage is incomplete and the report must say so.
    pub fn is_complete(&self) -> bool {
        matches!(self, Stop::Converged { .. })
    }

    pub fn describe(&self) -> String {
        match self {
            Stop::Converged { quiet_rounds } => format!(
                "converged: {quiet_rounds} consecutive rounds produced no finding that had not already been seen"
            ),
            Stop::RoundCap { max_rounds } => format!(
                "stopped at the round cap of {max_rounds} with new findings still appearing, so this review is not exhaustive"
            ),
            Stop::BudgetExhausted { rounds_run } => format!(
                "stopped after {rounds_run} rounds because the agent budget ran out, so this review is not exhaustive"
            ),
        }
    }
}

/// What one round produced, after deduplication.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoundOutcome {
    /// Findings not seen in any previous round.
    pub fresh: Vec<Finding>,
    /// The dimension of each proposal that repeated something already
    /// seen, so per-dimension accounting can attribute the repeat rather
    /// than carry an unattributed total.
    pub duplicates: Vec<String>,
}

/// How similar two findings' words must be, as a Jaccard score over their
/// token sets, before the second counts as a repeat of the first.
///
/// Exact matching would never converge: agents rephrase, so round two
/// says "no confidence intervals are given" where round one said "the
/// results report no confidence interval". A threshold this low would
/// merge genuinely different findings if they were compared across
/// dimensions, so they never are.
pub const DEFAULT_SIMILARITY: f64 = 0.6;

pub struct Convergence {
    quiet_rounds_required: usize,
    max_rounds: usize,
    similarity: f64,
    /// Every finding ever proposed, including ones the checker dropped
    /// and ones verification refuted. Deduplicating against survivors
    /// only would let a refuted finding come back every round forever,
    /// and the loop would never go quiet.
    seen: Vec<(String, Vec<String>)>,
    consecutive_quiet: usize,
    rounds_run: usize,
    budget_exhausted: bool,
}

/// Token set of the words that carry meaning in a finding: its quoted
/// span plus its claim. Deliberately not the evidence prose, which is the
/// part an agent rewrites most freely.
fn tokens(finding: &Finding) -> Vec<String> {
    let joined = format!("{} {}", finding.anchor, finding.claim);
    let mut t: Vec<String> = normalize(&joined)
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| !w.is_empty())
        .collect();
    t.sort();
    t.dedup();
    t
}

fn jaccard(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let mut shared = 0usize;
    for word in a {
        if b.binary_search(word).is_ok() {
            shared += 1;
        }
    }
    let union = a.len() + b.len() - shared;
    if union == 0 {
        return 0.0;
    }
    shared as f64 / union as f64
}

impl Convergence {
    pub fn new(quiet_rounds_required: usize, max_rounds: usize) -> Self {
        Convergence {
            quiet_rounds_required,
            max_rounds,
            similarity: DEFAULT_SIMILARITY,
            seen: Vec::new(),
            consecutive_quiet: 0,
            rounds_run: 0,
            budget_exhausted: false,
        }
    }

    pub fn with_similarity(mut self, similarity: f64) -> Self {
        self.similarity = similarity;
        self
    }

    pub fn rounds_run(&self) -> usize {
        self.rounds_run
    }

    pub fn seen_count(&self) -> usize {
        self.seen.len()
    }

    /// A repeat of something already proposed, in the same dimension.
    /// Findings are never compared across dimensions: the same sentence
    /// of a paper can be wrong statistically and wrong about its
    /// citation, and those are two findings, not one.
    fn is_duplicate(&self, finding: &Finding) -> bool {
        let t = tokens(finding);
        self.seen
            .iter()
            .filter(|(dim, _)| dim == &finding.dimension)
            .any(|(_, seen)| jaccard(&t, seen) >= self.similarity)
    }

    /// Record what a round proposed and return only what was new.
    ///
    /// Every proposal is added to `seen`, whatever happens to it
    /// afterwards. That is what makes the quiet counter monotone: a
    /// finding can be new exactly once.
    pub fn register_round(&mut self, proposed: Vec<Finding>) -> RoundOutcome {
        self.rounds_run += 1;
        let mut outcome = RoundOutcome::default();
        for finding in proposed {
            if self.is_duplicate(&finding) {
                outcome.duplicates.push(finding.dimension.clone());
                continue;
            }
            self.seen
                .push((finding.dimension.clone(), tokens(&finding)));
            outcome.fresh.push(finding);
        }
        if outcome.fresh.is_empty() {
            self.consecutive_quiet += 1;
        } else {
            self.consecutive_quiet = 0;
        }
        outcome
    }

    /// Record a proposal that never reached `register_round`, so it
    /// cannot come back as new later. Used for findings the anchor check
    /// dropped: they were still proposed.
    pub fn note_seen(&mut self, finding: &Finding) {
        if !self.is_duplicate(finding) {
            self.seen.push((finding.dimension.clone(), tokens(finding)));
        }
    }

    pub fn note_budget_exhausted(&mut self) {
        self.budget_exhausted = true;
    }

    /// Whether to stop, and why. Checked after each round.
    pub fn stop(&self) -> Option<Stop> {
        if self.consecutive_quiet >= self.quiet_rounds_required {
            return Some(Stop::Converged {
                quiet_rounds: self.consecutive_quiet,
            });
        }
        if self.budget_exhausted {
            return Some(Stop::BudgetExhausted {
                rounds_run: self.rounds_run,
            });
        }
        if self.rounds_run >= self.max_rounds {
            return Some(Stop::RoundCap {
                max_rounds: self.max_rounds,
            });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::finding::Severity;

    fn f(dimension: &str, claim: &str, anchor: &str) -> Finding {
        Finding {
            dimension: dimension.to_string(),
            severity: Severity::Major,
            claim: claim.to_string(),
            anchor: anchor.to_string(),
            evidence: "because".to_string(),
        }
    }

    #[test]
    fn a_round_with_new_findings_does_not_stop_the_loop() {
        let mut c = Convergence::new(2, 10);
        let out = c.register_round(vec![f("stat", "no error bars", "we report 14%")]);
        assert_eq!(out.fresh.len(), 1);
        assert_eq!(c.stop(), None);
    }

    #[test]
    fn stops_after_the_required_number_of_quiet_rounds() {
        let mut c = Convergence::new(2, 10);
        c.register_round(vec![f("stat", "no error bars", "we report 14%")]);
        assert_eq!(c.stop(), None);
        c.register_round(vec![]);
        assert_eq!(c.stop(), None, "one quiet round is not two");
        c.register_round(vec![]);
        assert_eq!(c.stop(), Some(Stop::Converged { quiet_rounds: 2 }));
    }

    /// A quiet round followed by a productive one must not carry the
    /// earlier silence forward, or the loop stops on two quiet rounds
    /// that were never consecutive.
    #[test]
    fn a_new_finding_resets_the_quiet_counter() {
        let mut c = Convergence::new(2, 10);
        c.register_round(vec![]);
        c.register_round(vec![f("stat", "no error bars", "we report 14 percent")]);
        assert_eq!(c.stop(), None);
        c.register_round(vec![]);
        assert_eq!(c.stop(), None, "only one round has been quiet since");
    }

    #[test]
    fn the_round_cap_stops_a_loop_that_keeps_finding_things() {
        let mut c = Convergence::new(2, 3);
        for i in 0..3 {
            let out = c.register_round(vec![f(
                "stat",
                &format!("problem {i}"),
                &format!("span {i}"),
            )]);
            assert_eq!(out.fresh.len(), 1);
        }
        assert_eq!(c.stop(), Some(Stop::RoundCap { max_rounds: 3 }));
    }

    /// The rule that keeps the loop finite in practice: a finding that
    /// was proposed and then thrown away is still seen. Deduplicating
    /// against survivors would let a refuted finding return every round
    /// and the quiet counter would never advance.
    #[test]
    fn a_refuted_finding_does_not_count_as_new_when_it_comes_back() {
        let mut c = Convergence::new(2, 10);
        let claim = f(
            "stat",
            "no error bars are reported",
            "we observe a 14% reduction",
        );
        let first = c.register_round(vec![claim.clone()]);
        assert_eq!(first.fresh.len(), 1);
        // The caller refutes it. Nothing is told to the convergence
        // tracker, because survival is not what it tracks.
        let second = c.register_round(vec![claim]);
        assert!(second.fresh.is_empty(), "a repeat is never new");
        assert_eq!(second.duplicates, vec!["stat".to_string()]);
    }

    #[test]
    fn a_rephrased_finding_counts_as_a_repeat() {
        let mut c = Convergence::new(2, 10);
        c.register_round(vec![f(
            "stat",
            "no error bars are reported for the latency result",
            "we observe a 14% reduction in latency",
        )]);
        let second = c.register_round(vec![f(
            "stat",
            "no error bars reported for the latency result",
            "we observe a 14% reduction in latency",
        )]);
        assert!(second.fresh.is_empty());
    }

    /// The same sentence can be wrong in two different ways. Merging
    /// those would silently drop one of them.
    #[test]
    fn the_same_span_in_a_different_dimension_is_a_different_finding() {
        let mut c = Convergence::new(2, 10);
        c.register_round(vec![f(
            "statistical-validity",
            "no error bars",
            "we observe a 14% reduction in latency",
        )]);
        let second = c.register_round(vec![f(
            "citation-integrity",
            "no error bars",
            "we observe a 14% reduction in latency",
        )]);
        assert_eq!(second.fresh.len(), 1);
    }

    #[test]
    fn a_dropped_proposal_is_still_seen_and_cannot_come_back_as_new() {
        let mut c = Convergence::new(2, 10);
        let dropped = f("stat", "no error bars are reported", "invented span");
        c.note_seen(&dropped);
        let out = c.register_round(vec![dropped]);
        assert!(out.fresh.is_empty());
    }

    #[test]
    fn budget_exhaustion_stops_the_loop_and_says_so() {
        let mut c = Convergence::new(2, 10);
        c.register_round(vec![f("stat", "a problem", "a span")]);
        c.note_budget_exhausted();
        assert_eq!(c.stop(), Some(Stop::BudgetExhausted { rounds_run: 1 }));
    }

    /// Convergence outranks the budget: a review that finished saying
    /// what it had to say is complete even if it spent its last agent
    /// doing it.
    #[test]
    fn convergence_outranks_budget_exhaustion() {
        let mut c = Convergence::new(1, 10);
        c.register_round(vec![]);
        c.note_budget_exhausted();
        assert_eq!(c.stop(), Some(Stop::Converged { quiet_rounds: 1 }));
    }

    #[test]
    fn only_a_converged_stop_reports_complete_coverage() {
        assert!(Stop::Converged { quiet_rounds: 2 }.is_complete());
        assert!(!Stop::RoundCap { max_rounds: 4 }.is_complete());
        assert!(!Stop::BudgetExhausted { rounds_run: 2 }.is_complete());
    }

    #[test]
    fn a_truncated_review_says_it_is_not_exhaustive() {
        assert!(Stop::RoundCap { max_rounds: 4 }
            .describe()
            .contains("not exhaustive"));
        assert!(Stop::BudgetExhausted { rounds_run: 2 }
            .describe()
            .contains("not exhaustive"));
    }
}
