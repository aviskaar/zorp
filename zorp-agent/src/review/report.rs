//! The review report.
//!
//! Two things it must never do. It must never imply a review was
//! exhaustive when a bound cut it short, and it must never present a
//! finding about reach or cost as a finding about correctness. Both are
//! the same failure: a report read as saying more than it knows.

use super::convergence::Stop;
use super::dimension::{by_key, Category};
use super::finding::{Finding, Severity};
use super::verify::{Verdict, Verification};
use serde::Serialize;
use std::fmt::Write as _;

/// A finding that reached verification, with what verification concluded.
#[derive(Clone, Debug, Serialize)]
pub struct ReviewedFinding {
    #[serde(flatten)]
    pub finding: Finding,
    pub verification: Verification,
    /// Which round proposed it.
    pub round: usize,
}

/// Per-dimension accounting, so a dimension that contributes nothing is
/// visible as such rather than merely absent. A dimension that proposes
/// steadily and never survives verification is a dimension producing
/// generic advice, and this table is how that gets noticed.
#[derive(Clone, Debug, Default, Serialize)]
pub struct DimensionTally {
    pub dimension: String,
    pub proposed: usize,
    pub dropped_unanchored: usize,
    pub duplicates: usize,
    pub checker_rejected: usize,
    pub refuted: usize,
    pub surviving: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReviewReport {
    pub paper: String,
    pub paper_words: usize,
    pub dimensions_run: Vec<String>,
    pub stop: Stop,
    pub coverage_is_complete: bool,
    pub rounds_run: usize,
    pub max_rounds: usize,
    pub agents_spent: usize,
    pub agent_budget: usize,
    pub max_depth: usize,
    /// Upheld and unverified findings, ranked. Refuted ones are counted
    /// in `tallies` and not listed: they did not survive an attempt to
    /// break them and printing them would bury the ones that did.
    pub findings: Vec<ReviewedFinding>,
    pub tallies: Vec<DimensionTally>,
    /// Dimensions asked for and not run, with why.
    pub skipped: Vec<(String, String)>,
    /// Work the budget refused, so the report can say what was not
    /// covered instead of implying it was covered and came back clean.
    pub not_covered: Vec<String>,
    /// Reviewer answers that could not be parsed. Counted, because a
    /// dimension whose agents all failed to answer is not a dimension
    /// that found nothing.
    pub unparseable_replies: usize,
}

fn category_of(dimension: &str) -> Category {
    by_key(dimension)
        .map(|d| d.category)
        .unwrap_or(Category::Technical)
}

fn title_of(dimension: &str) -> &str {
    by_key(dimension).map(|d| d.title).unwrap_or(dimension)
}

/// Rank findings for the report: worst first, and within a severity in
/// the order they were found, so a reader working top-down works through
/// them in the order they matter.
pub fn rank(findings: &mut [ReviewedFinding]) {
    findings.sort_by(|a, b| {
        a.finding
            .severity
            .cmp(&b.finding.severity)
            .then(a.round.cmp(&b.round))
            .then(a.finding.dimension.cmp(&b.finding.dimension))
    });
}

const ORDERED_CATEGORIES: [Category; 4] = [
    Category::Technical,
    Category::Communication,
    Category::Distribution,
    Category::Executive,
];

pub fn render_markdown(report: &ReviewReport) -> String {
    let mut out = String::new();
    out.push_str("# Paper review\n\n");
    let _ = writeln!(
        out,
        "Paper: `{}` ({} words)\n",
        report.paper, report.paper_words
    );

    out.push_str("## How this review stopped\n\n");
    let _ = writeln!(out, "{}\n", report.stop.describe());
    if !report.coverage_is_complete {
        out.push_str(
            "**This review is incomplete.** A bound was reached before the review ran out \
             of things to say. Treat the absence of a finding as absence of coverage, not \
             as evidence the paper is clean.\n\n",
        );
    }
    let _ = writeln!(
        out,
        "- Rounds: {} of {} allowed\n- Agents: {} of {} allowed\n- Recursion depth limit: {}\n",
        report.rounds_run,
        report.max_rounds,
        report.agents_spent,
        report.agent_budget,
        report.max_depth
    );

    let upheld = report
        .findings
        .iter()
        .filter(|f| f.verification.verdict == Verdict::Upheld)
        .count();
    let unverified = report.findings.len() - upheld;

    out.push_str("## Findings\n\n");
    if report.findings.is_empty() {
        let _ = writeln!(
            out,
            "No findings. {} proposals were made across {} dimensions and none survived \
             both the anchor check and adversarial verification.\n",
            report.tallies.iter().map(|t| t.proposed).sum::<usize>(),
            report.dimensions_run.len()
        );
    } else {
        let _ = writeln!(
            out,
            "{upheld} findings survived an attempt to refute them. {unverified} could not \
             be verified before a bound was reached.\n"
        );
        for category in ORDERED_CATEGORIES {
            let in_category: Vec<&ReviewedFinding> = report
                .findings
                .iter()
                .filter(|f| category_of(&f.finding.dimension) == category)
                .collect();
            if in_category.is_empty() {
                continue;
            }
            let _ = writeln!(out, "### {}\n", category.label());
            if let Some(caveat) = category.caveat() {
                let _ = writeln!(out, "> {caveat}\n");
            }
            for f in in_category {
                let _ = writeln!(
                    out,
                    "#### [{}] {}\n",
                    f.finding.severity.label(),
                    f.finding.claim
                );
                let _ = writeln!(out, "- Dimension: {}", title_of(&f.finding.dimension));
                let _ = writeln!(out, "- From the paper: \"{}\"", f.finding.anchor);
                let _ = writeln!(out, "- Why it matters: {}", f.finding.evidence);
                let votes: Vec<String> = f
                    .verification
                    .votes
                    .iter()
                    .map(|(lens, vote)| format!("{lens}={vote:?}").to_lowercase())
                    .collect();
                let _ = writeln!(
                    out,
                    "- Verification: {} ({})\n",
                    f.verification.verdict.label(),
                    if votes.is_empty() {
                        "no verifier reached it".to_string()
                    } else {
                        votes.join(", ")
                    }
                );
            }
        }
    }

    out.push_str("## What each dimension contributed\n\n");
    out.push_str(
        "| Dimension | Proposed | Unanchored | Repeat | Checker dropped | Refuted | Surviving |\n",
    );
    out.push_str("|---|---|---|---|---|---|---|\n");
    for t in &report.tallies {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} |",
            title_of(&t.dimension),
            t.proposed,
            t.dropped_unanchored,
            t.duplicates,
            t.checker_rejected,
            t.refuted,
            t.surviving
        );
    }
    out.push('\n');

    if !report.skipped.is_empty()
        || !report.not_covered.is_empty()
        || report.unparseable_replies > 0
    {
        out.push_str("## What this review did not cover\n\n");
        for (key, reason) in &report.skipped {
            let _ = writeln!(out, "- {}: not run, {reason}", title_of(key));
        }
        for item in &report.not_covered {
            let _ = writeln!(out, "- {item}");
        }
        if report.unparseable_replies > 0 {
            let _ = writeln!(
                out,
                "- {} reviewer answers could not be read and were discarded.",
                report.unparseable_replies
            );
        }
        out.push('\n');
    }

    out
}

/// The severity counts a checkpoint prompt needs.
pub fn severity_counts(report: &ReviewReport) -> (usize, usize, usize) {
    let count = |s: Severity| {
        report
            .findings
            .iter()
            .filter(|f| f.finding.severity == s)
            .count()
    };
    (
        count(Severity::Blocking),
        count(Severity::Major),
        count(Severity::Minor),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::verify::Vote;

    fn reviewed(dimension: &str, severity: Severity, verdict: Verdict) -> ReviewedFinding {
        ReviewedFinding {
            finding: Finding {
                dimension: dimension.to_string(),
                severity,
                claim: format!("a problem in {dimension}"),
                anchor: "we observe a 14% reduction in latency".to_string(),
                evidence: "a single run is reported as a result".to_string(),
            },
            verification: Verification {
                verdict,
                votes: vec![("text".to_string(), Vote::Upheld)],
            },
            round: 1,
        }
    }

    fn base_report() -> ReviewReport {
        ReviewReport {
            paper: "docs/paper/zorp-paper.md".to_string(),
            paper_words: 8000,
            dimensions_run: vec!["statistical-validity".to_string()],
            stop: Stop::Converged { quiet_rounds: 2 },
            coverage_is_complete: true,
            rounds_run: 3,
            max_rounds: 4,
            agents_spent: 42,
            agent_budget: 150,
            max_depth: 3,
            findings: vec![],
            tallies: vec![DimensionTally {
                dimension: "statistical-validity".to_string(),
                proposed: 4,
                dropped_unanchored: 2,
                duplicates: 1,
                checker_rejected: 0,
                refuted: 1,
                surviving: 0,
            }],
            skipped: vec![],
            not_covered: vec![],
            unparseable_replies: 0,
        }
    }

    /// A reviewer that always finds something tells you nothing, so a
    /// clean paper has to be expressible.
    #[test]
    fn a_review_with_nothing_to_say_reports_zero_findings() {
        let md = render_markdown(&base_report());
        assert!(md.contains("No findings."));
        assert!(md.contains("4 proposals were made"));
        assert!(!md.contains("This review is incomplete"));
    }

    /// The failure this whole report format exists to prevent: a
    /// truncated review read as a clean bill of health.
    #[test]
    fn a_review_stopped_by_a_bound_says_it_is_incomplete() {
        let mut r = base_report();
        r.stop = Stop::RoundCap { max_rounds: 4 };
        r.coverage_is_complete = false;
        let md = render_markdown(&r);
        assert!(md.contains("This review is incomplete"));
        assert!(md.contains("not exhaustive"));
        assert!(md.contains("absence of coverage"));
    }

    #[test]
    fn a_review_stopped_by_the_budget_says_so_and_lists_what_was_refused() {
        let mut r = base_report();
        r.stop = Stop::BudgetExhausted { rounds_run: 2 };
        r.coverage_is_complete = false;
        r.not_covered = vec!["reproducibility doer: the agent budget is spent (150 of 150)".into()];
        let md = render_markdown(&r);
        assert!(md.contains("What this review did not cover"));
        assert!(md.contains("reproducibility doer"));
    }

    #[test]
    fn a_skipped_dimension_is_named_with_its_reason() {
        let mut r = base_report();
        r.skipped = vec![(
            "venue-fit".to_string(),
            "no venue shortlist was available".to_string(),
        )];
        let md = render_markdown(&r);
        assert!(md.contains("Venue fit: not run, no venue shortlist"));
    }

    /// Shareable is not correct. A distribution finding must never sit
    /// under a heading a reader takes for a correctness finding.
    #[test]
    fn distribution_findings_are_separated_from_technical_ones() {
        let mut r = base_report();
        r.findings = vec![
            reviewed("statistical-validity", Severity::Major, Verdict::Upheld),
            reviewed("virality-reach", Severity::Major, Verdict::Upheld),
        ];
        let md = render_markdown(&r);
        let technical = md.find("### Technical").expect("technical heading");
        let distribution = md.find("### Distribution").expect("distribution heading");
        assert!(technical < distribution);
        assert!(md.contains("Shareable is not the same as correct"));
    }

    #[test]
    fn executive_findings_carry_their_own_caveat() {
        let mut r = base_report();
        r.findings = vec![reviewed("exec-cfo", Severity::Major, Verdict::Upheld)];
        let md = render_markdown(&r);
        assert!(md.contains("### Executive"));
        assert!(md.contains("not about whether the paper is correct"));
    }

    /// A report with only technical findings must not print caveats
    /// about reach that do not apply to anything in it.
    #[test]
    fn a_purely_technical_report_prints_no_distribution_caveat() {
        let mut r = base_report();
        r.findings = vec![reviewed(
            "statistical-validity",
            Severity::Major,
            Verdict::Upheld,
        )];
        let md = render_markdown(&r);
        assert!(!md.contains("Shareable is not the same as correct"));
    }

    #[test]
    fn findings_are_ranked_worst_first() {
        let mut findings = vec![
            reviewed("readability", Severity::Minor, Verdict::Upheld),
            reviewed("statistical-validity", Severity::Blocking, Verdict::Upheld),
            reviewed("data-correctness", Severity::Major, Verdict::Upheld),
        ];
        rank(&mut findings);
        let severities: Vec<Severity> = findings.iter().map(|f| f.finding.severity).collect();
        assert_eq!(
            severities,
            vec![Severity::Blocking, Severity::Major, Severity::Minor]
        );
    }

    #[test]
    fn an_unverified_finding_is_listed_and_labelled_as_such() {
        let mut r = base_report();
        r.findings = vec![reviewed(
            "statistical-validity",
            Severity::Major,
            Verdict::Unverified("budget ran out".to_string()),
        )];
        let md = render_markdown(&r);
        assert!(md.contains("Verification: unverified"));
        assert!(md.contains("could not\nbe verified") || md.contains("could not be verified"));
    }

    #[test]
    fn the_per_dimension_table_shows_what_each_dimension_contributed() {
        let md = render_markdown(&base_report());
        assert!(md.contains("What each dimension contributed"));
        assert!(md.contains("| Statistical validity | 4 | 2 | 1 | 0 | 1 | 0 |"));
    }

    #[test]
    fn severity_counts_split_by_severity() {
        let mut r = base_report();
        r.findings = vec![
            reviewed("statistical-validity", Severity::Blocking, Verdict::Upheld),
            reviewed("data-correctness", Severity::Major, Verdict::Upheld),
            reviewed("readability", Severity::Major, Verdict::Upheld),
        ];
        assert_eq!(severity_counts(&r), (1, 2, 0));
    }

    #[test]
    fn the_report_serializes_to_json() {
        let mut r = base_report();
        r.findings = vec![reviewed(
            "statistical-validity",
            Severity::Major,
            Verdict::Upheld,
        )];
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"statistical-validity\""));
        assert!(json.contains("\"upheld\""));
    }
}
