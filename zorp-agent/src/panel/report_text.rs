//! A panel report, as lines for a terminal.
//!
//! `panel::run` has been in this crate the whole time and only the browser
//! could call it, which is a strange place to end up given the decision
//! that created it says in its own words that the panel is "a button in the
//! browser and a function in `zorp-agent`". The function was here; nothing
//! in the CLI reached it.
//!
//! This is the rendering half, split out so it can be tested without a
//! model, a socket or a terminal. It decides nothing: the agreement count
//! is computed by `PanelReport::agreements`, in code, and this prints what
//! that came back with.
//!
//! Two things it must keep saying. A partial panel is not a complete one,
//! because two of two agreeing is a weaker claim than two of five and the
//! count alone cannot tell them apart. And a reviewer that fell over is
//! part of the report rather than a line in a log, or "every reviewer
//! agreed" quietly starts meaning "the one reviewer that ran agreed".

use super::verdict::{PanelReport, Severity};

/// How much of a reviewer's own answer to show under its findings.
///
/// The answer is kept so a reader can see the reasoning behind a finding
/// and not only the finding, but a terminal is not the place for five full
/// essays by default. `--full` prints them whole.
const ANSWER_PREVIEW_CHARS: usize = 400;

/// The mark in front of a finding, so severity reads down the left edge.
fn mark(severity: Severity) -> &'static str {
    match severity {
        Severity::Blocking => "!!",
        Severity::Concern => " !",
        Severity::Note => "  ",
    }
}

/// The whole report, as lines.
///
/// `full` prints each reviewer's answer in its entirety rather than a
/// preview.
pub fn lines(report: &PanelReport, full: bool) -> Vec<String> {
    let mut out = Vec::new();

    out.push(format!("panel on {}", report.target));

    // Said before anything else, because it is what qualifies every number
    // below it. A reader who sees the agreement count first and the
    // completeness second has already drawn a conclusion.
    let ran = report.verdicts.len();
    let asked = report.lenses_requested;
    if report.stopped {
        out.push(format!(
            "stopped: {ran} of {asked} reviewers came back before it was stopped"
        ));
    } else if ran != asked {
        out.push(format!("partial: {ran} of {asked} reviewers came back"));
    } else {
        out.push(format!("complete: {ran} of {asked} reviewers came back"));
    }

    for failure in &report.failures {
        out.push(format!("  failed  {}: {}", failure.lens, failure.why));
    }

    for verdict in &report.verdicts {
        out.push(String::new());
        out.push(format!(
            "{} ({} finding{})",
            verdict.lens,
            verdict.findings.len(),
            if verdict.findings.len() == 1 { "" } else { "s" }
        ));
        for finding in &verdict.findings {
            out.push(format!(
                "  {} {}  {}",
                mark(finding.severity),
                finding.locus,
                finding.claim
            ));
        }
        let answer = verdict.answer.trim();
        if !answer.is_empty() {
            out.push(String::new());
            out.push(indent(&clip(answer, full), "  "));
        }
    }

    let agreements = report.agreements();
    out.push(String::new());
    if agreements.is_empty() {
        out.push("no finding was raised from more than one angle".to_string());
    } else {
        out.push("agreement, counted in code:".to_string());
        for agreement in &agreements {
            // Lenses rather than findings, so one reviewer listing the same
            // objection three times cannot corroborate itself.
            out.push(format!(
                "  {} {} of {}  {}  ({})",
                mark(agreement.highest),
                agreement.corroboration(),
                asked,
                agreement.locus,
                agreement.lenses.join(", ")
            ));
        }
    }

    // The last line, because it is the one a reader should leave with.
    if !report.is_complete() {
        out.push(String::new());
        out.push(
            "This panel is not complete, so a corroboration count here is out of a \
             smaller number than it looks."
                .to_string(),
        );
    }
    out
}

fn clip(text: &str, full: bool) -> String {
    if full || text.chars().count() <= ANSWER_PREVIEW_CHARS {
        return text.to_string();
    }
    let kept: String = text.chars().take(ANSWER_PREVIEW_CHARS).collect();
    format!(
        "{}...\n(run with --full for the whole answer)",
        kept.trim_end()
    )
}

fn indent(text: &str, by: &str) -> String {
    text.lines()
        .map(|l| format!("{by}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::verdict::{PanelFinding, ReviewerFailure, ReviewerVerdict};

    fn finding(severity: Severity, locus: &str, claim: &str) -> PanelFinding {
        PanelFinding {
            severity,
            claim: claim.to_string(),
            locus: locus.to_string(),
        }
    }

    fn verdict(lens: &str, findings: Vec<PanelFinding>) -> ReviewerVerdict {
        ReviewerVerdict {
            lens: lens.to_string(),
            findings,
            answer: "because the record does not support it".to_string(),
        }
    }

    fn report(verdicts: Vec<ReviewerVerdict>, failures: Vec<ReviewerFailure>) -> PanelReport {
        let asked = verdicts.len() + failures.len();
        PanelReport {
            target: "the draft".to_string(),
            verdicts,
            failures,
            lenses_requested: asked,
            stopped: false,
        }
    }

    fn text(report: &PanelReport) -> String {
        lines(report, false).join("\n")
    }

    #[test]
    fn a_complete_panel_says_so_and_lists_every_finding() {
        let r = report(
            vec![
                verdict(
                    "evidence",
                    vec![finding(
                        Severity::Blocking,
                        "section 3",
                        "the number is unsupported",
                    )],
                ),
                verdict(
                    "clarity",
                    vec![finding(Severity::Note, "section 1", "long sentence")],
                ),
            ],
            vec![],
        );

        let out = text(&r);
        assert!(out.contains("panel on the draft"), "{out}");
        assert!(out.contains("complete: 2 of 2"), "{out}");
        assert!(out.contains("the number is unsupported"), "{out}");
        assert!(out.contains("long sentence"), "{out}");
        assert!(!out.contains("not complete"), "{out}");
    }

    /// A panel of five where two fell over is not a panel of three, and a
    /// report that cannot tell those apart lets "every reviewer agreed"
    /// mean "the one reviewer that ran agreed".
    #[test]
    fn a_reviewer_that_fell_over_is_in_the_report_and_the_count_says_so() {
        let r = report(
            vec![verdict(
                "evidence",
                vec![finding(Severity::Concern, "section 3", "unsupported")],
            )],
            vec![ReviewerFailure {
                lens: "clarity".to_string(),
                why: "the model returned no fenced block".to_string(),
            }],
        );

        let out = text(&r);
        assert!(out.contains("partial: 1 of 2"), "{out}");
        assert!(out.contains("failed  clarity"), "{out}");
        assert!(out.contains("no fenced block"), "{out}");
        assert!(out.contains("not complete"), "{out}");
    }

    /// A stop is not a failure and the report says which.
    #[test]
    fn a_stopped_panel_reads_as_stopped_rather_than_broken() {
        let mut r = report(vec![verdict("evidence", vec![])], vec![]);
        r.lenses_requested = 3;
        r.stopped = true;

        let out = text(&r);
        assert!(out.contains("stopped: 1 of 3"), "{out}");
        assert!(!out.contains("partial:"), "{out}");
    }

    /// The count is of lenses, so one reviewer listing the same objection
    /// three times cannot corroborate itself.
    #[test]
    fn agreement_is_counted_across_lenses_and_the_denominator_is_shown() {
        let r = report(
            vec![
                verdict(
                    "evidence",
                    vec![
                        finding(Severity::Concern, "section 3", "unsupported"),
                        finding(Severity::Note, "section 3", "still unsupported"),
                    ],
                ),
                verdict(
                    "clarity",
                    vec![finding(Severity::Blocking, "section 3", "also unsupported")],
                ),
            ],
            vec![],
        );

        let out = text(&r);
        assert!(out.contains("agreement, counted in code"), "{out}");
        // Two lenses, not three findings, and out of the two that were asked.
        assert!(out.contains("2 of 2"), "{out}");
        assert!(
            out.contains("evidence, clarity") || out.contains("clarity, evidence"),
            "{out}"
        );
    }

    #[test]
    fn no_agreement_says_so_rather_than_printing_nothing() {
        let r = report(
            vec![
                verdict("evidence", vec![finding(Severity::Note, "one", "a")]),
                verdict("clarity", vec![finding(Severity::Note, "two", "b")]),
            ],
            vec![],
        );

        assert!(
            text(&r).contains("no finding was raised from more than one angle"),
            "{}",
            text(&r)
        );
    }

    /// Severity reads down the left edge, so a blocking finding is
    /// findable without reading every line.
    #[test]
    fn severity_is_marked_in_the_margin() {
        let r = report(
            vec![verdict(
                "evidence",
                vec![
                    finding(Severity::Blocking, "a", "stops the work"),
                    finding(Severity::Note, "b", "worth knowing"),
                ],
            )],
            vec![],
        );

        let out = text(&r);
        assert!(out.contains("!! a  stops the work"), "{out}");
        assert!(out.contains("    b  worth knowing"), "{out}");
    }

    /// The reasoning is kept, but a terminal is not the place for five
    /// full essays unless somebody asks.
    #[test]
    fn a_long_answer_is_previewed_and_full_prints_it_whole() {
        let long = "x".repeat(ANSWER_PREVIEW_CHARS * 2);
        let mut v = verdict("evidence", vec![]);
        v.answer = long.clone();
        let r = report(vec![v], vec![]);

        let preview = lines(&r, false).join("\n");
        assert!(preview.contains("--full"), "{preview}");
        assert!(preview.len() < long.len() + 500, "the preview did not clip");

        let whole = lines(&r, true).join("\n");
        assert!(
            whole.contains(&long),
            "--full did not print the whole answer"
        );
        assert!(!whole.contains("--full for the whole answer"));
    }

    /// An empty panel is a readable report rather than a crash or a blank
    /// screen.
    #[test]
    fn a_panel_where_everything_failed_still_reads() {
        let r = report(
            vec![],
            vec![
                ReviewerFailure {
                    lens: "evidence".to_string(),
                    why: "connection refused".to_string(),
                },
                ReviewerFailure {
                    lens: "clarity".to_string(),
                    why: "connection refused".to_string(),
                },
            ],
        );

        let out = text(&r);
        assert!(out.contains("partial: 0 of 2"), "{out}");
        assert!(out.contains("not complete"), "{out}");
        assert!(out.contains("no finding was raised"), "{out}");
    }
}
