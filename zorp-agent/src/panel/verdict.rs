//! What a reviewer returns, and what the panel makes of the set.
//!
//! The parsing here is strict on purpose. A reviewer that answered in
//! prose has not reviewed anything the panel can count, and accepting a
//! best guess at what it meant would put a made-up finding next to real
//! ones with nothing to tell them apart.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;

/// How bad a reviewer thinks something is.
///
/// Three levels, not five. A reviewer asked to grade finely spends its
/// attention on the grade, and the only distinction the panel acts on
/// is whether something has to be fixed before the work can be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Worth knowing, does not block.
    Note,
    /// Probably wrong, or right but unsupported.
    Concern,
    /// Wrong in a way that makes the work unusable as it stands.
    Blocking,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Note => "note",
            Severity::Concern => "concern",
            Severity::Blocking => "blocking",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One thing one reviewer objected to.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PanelFinding {
    pub severity: Severity,
    /// What is wrong, in the reviewer's words.
    pub claim: String,
    /// Where in the target it is. Free text, because the target may be a
    /// document, a diff, or a record, and a line number does not fit all
    /// three. Used for corroboration, so a reviewer that leaves it vague
    /// makes its own finding harder to corroborate.
    pub locus: String,
}

/// What one reviewer came back with.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewerVerdict {
    pub lens: String,
    pub findings: Vec<PanelFinding>,
    /// The reviewer's whole answer, kept so a reader can see the
    /// reasoning behind a finding and not only the finding.
    pub answer: String,
}

/// A reviewer that did not come back with anything countable.
///
/// Kept as a first class part of the report rather than logged and
/// dropped. A panel of five where two fell over is not a panel of
/// three, and a report that cannot tell those apart lets "every
/// reviewer agreed" mean "the one reviewer that ran agreed".
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewerFailure {
    pub lens: String,
    pub why: String,
}

#[derive(Debug, Deserialize)]
struct RawVerdict {
    findings: Vec<PanelFinding>,
}

/// Why a reviewer's answer could not be read.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    NoFencedBlock,
    InvalidJson(String),
    /// A finding with an empty claim. Not tolerated, because it would
    /// count towards corroboration while saying nothing.
    EmptyClaim,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::NoFencedBlock => {
                write!(
                    f,
                    "no verdict object in the reviewer's answer, fenced or bare"
                )
            }
            ParseError::InvalidJson(msg) => write!(f, "verdict object was not valid JSON: {msg}"),
            ParseError::EmptyClaim => {
                write!(f, "a finding has an empty claim, so it says nothing")
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// Whether `body` has the one field a verdict requires.
fn is_verdict_shaped(body: &str) -> bool {
    serde_json::from_str::<RawVerdict>(body).is_ok()
}

/// Read a reviewer's answer into findings.
///
/// An answer with no findings is a valid verdict, not an error: a
/// reviewer that looked and found nothing is a result, and treating it
/// as a failure would quietly bias the panel towards objection.
pub fn parse_verdict(answer: &str) -> Result<Vec<PanelFinding>, ParseError> {
    let blocks = crate::blocks::fenced_blocks(answer);
    let bare = crate::blocks::bare_objects(answer);
    if blocks.is_empty() && bare.is_empty() {
        return Err(ParseError::NoFencedBlock);
    }
    let found = blocks
        .iter()
        .rev()
        .find(|block| is_verdict_shaped(block))
        .or_else(|| bare.iter().rev().find(|block| is_verdict_shaped(block)));
    let Some(block) = found else {
        let last_err = blocks
            .iter()
            .chain(bare.iter())
            .filter_map(|block| serde_json::from_str::<RawVerdict>(block).err())
            .next_back();
        return Err(ParseError::InvalidJson(
            last_err.map(|e| e.to_string()).unwrap_or_default(),
        ));
    };

    // Shaped, so this cannot fail: the shape check parsed it and
    // confirmed the required field.
    let raw = serde_json::from_str::<RawVerdict>(block).expect("shaped verdict object");
    for finding in &raw.findings {
        if finding.claim.trim().is_empty() {
            return Err(ParseError::EmptyClaim);
        }
    }
    Ok(raw.findings)
}

/// One locus, and every lens that raised something about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agreement {
    pub locus: String,
    /// The lenses that raised it, sorted and deduplicated. Its length is
    /// the corroboration count, and it is a count of *lenses* rather
    /// than of findings so one reviewer listing the same objection three
    /// times cannot corroborate itself.
    pub lenses: Vec<String>,
    /// The worst severity any lens assigned it.
    pub highest: Severity,
}

impl Agreement {
    pub fn corroboration(&self) -> usize {
        self.lenses.len()
    }
}

/// Everything the panel produced.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelReport {
    /// The label of what was reviewed, carried so a report read on its
    /// own says what it is about.
    pub target: String,
    pub verdicts: Vec<ReviewerVerdict>,
    pub failures: Vec<ReviewerFailure>,
    /// How many reviewers were asked for. Compared against
    /// `verdicts.len()` to tell a complete panel from a partial one.
    pub lenses_requested: usize,
    /// True when a human stopped the panel. A partial panel that was
    /// stopped is not the same as one that fell over, and the report
    /// says which.
    pub stopped: bool,
}

impl PanelReport {
    /// Whether every requested reviewer returned a readable verdict.
    ///
    /// The one thing to check before quoting a corroboration count. Two
    /// of two agreeing is a weaker claim than two of five, and the
    /// number alone cannot tell them apart.
    pub fn is_complete(&self) -> bool {
        !self.stopped && self.verdicts.len() == self.lenses_requested
    }

    /// Loci raised by more than one lens, most corroborated first.
    ///
    /// Computed here, in code, and never asked of a model. A panel whose
    /// members read each other's findings is one reviewer with extra
    /// steps, so agreement has to be measured from outside rather than
    /// negotiated from inside.
    ///
    /// Matching is on the locus, normalized for case and surrounding
    /// whitespace and nothing else. Deliberately literal: a fuzzier
    /// match would merge two different objections that happen to be
    /// worded alike, and inflating a corroboration count is the one
    /// error this function must not make.
    pub fn agreements(&self) -> Vec<Agreement> {
        let mut by_locus: BTreeMap<String, (Vec<String>, Severity, String)> = BTreeMap::new();
        for verdict in &self.verdicts {
            for finding in &verdict.findings {
                let key = finding.locus.trim().to_lowercase();
                if key.is_empty() {
                    continue;
                }
                let entry = by_locus.entry(key).or_insert_with(|| {
                    (Vec::new(), Severity::Note, finding.locus.trim().to_string())
                });
                if !entry.0.contains(&verdict.lens) {
                    entry.0.push(verdict.lens.clone());
                }
                entry.1 = entry.1.max(finding.severity);
            }
        }
        let mut out: Vec<Agreement> = by_locus
            .into_values()
            .filter(|(lenses, _, _)| lenses.len() > 1)
            .map(|(mut lenses, highest, locus)| {
                lenses.sort();
                Agreement {
                    locus,
                    lenses,
                    highest,
                }
            })
            .collect();
        out.sort_by(|a, b| {
            b.corroboration()
                .cmp(&a.corroboration())
                .then_with(|| b.highest.cmp(&a.highest))
                .then_with(|| a.locus.cmp(&b.locus))
        });
        out
    }

    /// Every blocking finding, whoever raised it.
    ///
    /// Not filtered by corroboration. One reviewer finding a real defect
    /// that the others missed is the case a panel exists for, and
    /// requiring a second vote before a blocking finding is shown would
    /// throw exactly that away.
    pub fn blocking(&self) -> Vec<(&str, &PanelFinding)> {
        let mut out: Vec<(&str, &PanelFinding)> = self
            .verdicts
            .iter()
            .flat_map(|v| {
                v.findings
                    .iter()
                    .filter(|f| f.severity == Severity::Blocking)
                    .map(move |f| (v.lens.as_str(), f))
            })
            .collect();
        out.sort_by(|a, b| a.1.locus.cmp(&b.1.locus).then_with(|| a.0.cmp(b.0)));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(severity: Severity, locus: &str) -> PanelFinding {
        PanelFinding {
            severity,
            claim: format!("something about {locus}"),
            locus: locus.to_string(),
        }
    }

    fn verdict(lens: &str, findings: Vec<PanelFinding>) -> ReviewerVerdict {
        ReviewerVerdict {
            lens: lens.to_string(),
            findings,
            answer: String::new(),
        }
    }

    fn report(verdicts: Vec<ReviewerVerdict>) -> PanelReport {
        let lenses_requested = verdicts.len();
        PanelReport {
            target: "draft.md".into(),
            verdicts,
            failures: Vec::new(),
            lenses_requested,
            stopped: false,
        }
    }

    #[test]
    fn a_well_formed_verdict_parses() {
        let answer = "I looked at it.\n\n```json\n{\"findings\": [\
            {\"severity\": \"blocking\", \"claim\": \"the number is not in the record\", \
             \"locus\": \"section 3\"}]}\n```";
        let findings = parse_verdict(answer).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Blocking);
        assert_eq!(findings[0].locus, "section 3");
    }

    /// A reviewer that looked and found nothing is a result. Treating it
    /// as a failure would bias the panel towards objection, which is the
    /// opposite of what adversarial review is for.
    #[test]
    fn an_empty_finding_list_is_a_verdict_not_a_failure() {
        let answer = "```json\n{\"findings\": []}\n```";
        assert_eq!(parse_verdict(answer).unwrap(), vec![]);
    }

    /// The verdict is asked for last. A reviewer quoting the passage it
    /// objects to in a fence above must not have the quote parsed as its
    /// answer.
    #[test]
    fn the_last_fenced_block_is_the_verdict() {
        let answer = "The passage says:\n\n```\n{\"findings\": [{\"severity\": \"note\", \
            \"claim\": \"quoted\", \"locus\": \"quoted\"}]}\n```\n\nMy verdict:\n\n\
            ```json\n{\"findings\": [{\"severity\": \"blocking\", \"claim\": \"real\", \
            \"locus\": \"real\"}]}\n```";
        let findings = parse_verdict(answer).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].claim, "real");
    }

    #[test]
    fn prose_with_no_object_is_not_a_verdict() {
        assert_eq!(
            parse_verdict("Looks fine to me.").unwrap_err(),
            ParseError::NoFencedBlock
        );
    }

    #[test]
    fn a_fenced_block_that_is_not_json_is_refused() {
        let err = parse_verdict("```\nnot json at all\n```").unwrap_err();
        assert!(matches!(err, ParseError::InvalidJson(_)), "{err:?}");
    }

    #[test]
    fn a_bare_verdict_object_is_still_read() {
        let answer = r#"My verdict is {"findings": [{"severity": "note", "claim": "small issue", "locus": "section 3"}]}"#;
        let findings = parse_verdict(answer).unwrap();
        assert_eq!(findings[0].claim, "small issue");
    }

    #[test]
    fn an_unclosed_final_fence_is_still_read() {
        let answer = "My verdict:\n```json\n{\"findings\": [{\"severity\": \"note\", \"claim\": \"small issue\", \"locus\": \"section 3\"}]}";
        let findings = parse_verdict(answer).unwrap();
        assert_eq!(findings[0].claim, "small issue");
    }

    /// A finding with no claim would count towards corroboration while
    /// saying nothing.
    #[test]
    fn an_empty_claim_is_refused() {
        let answer = "```json\n{\"findings\": [{\"severity\": \"note\", \"claim\": \"  \", \
            \"locus\": \"somewhere\"}]}\n```";
        assert_eq!(parse_verdict(answer).unwrap_err(), ParseError::EmptyClaim);
    }

    #[test]
    fn two_lenses_on_one_locus_corroborate() {
        let r = report(vec![
            verdict("correctness", vec![finding(Severity::Concern, "section 3")]),
            verdict("evidence", vec![finding(Severity::Blocking, "section 3")]),
        ]);
        let agreements = r.agreements();
        assert_eq!(agreements.len(), 1);
        assert_eq!(agreements[0].corroboration(), 2);
        assert_eq!(agreements[0].highest, Severity::Blocking);
        assert_eq!(agreements[0].lenses, vec!["correctness", "evidence"]);
    }

    /// A single lens listing the same objection three times must not
    /// corroborate itself. Counting findings rather than lenses is the
    /// obvious implementation and it is wrong.
    #[test]
    fn one_lens_repeating_itself_does_not_corroborate() {
        let r = report(vec![verdict(
            "correctness",
            vec![
                finding(Severity::Note, "section 3"),
                finding(Severity::Concern, "section 3"),
                finding(Severity::Blocking, "section 3"),
            ],
        )]);
        assert!(r.agreements().is_empty(), "{:?}", r.agreements());
    }

    #[test]
    fn a_locus_only_one_lens_raised_is_not_an_agreement() {
        let r = report(vec![
            verdict("correctness", vec![finding(Severity::Blocking, "alone")]),
            verdict("evidence", vec![finding(Severity::Note, "elsewhere")]),
        ]);
        assert!(r.agreements().is_empty());
    }

    #[test]
    fn agreements_are_ordered_by_corroboration() {
        let r = report(vec![
            verdict(
                "a",
                vec![finding(Severity::Note, "x"), finding(Severity::Note, "y")],
            ),
            verdict(
                "b",
                vec![finding(Severity::Note, "x"), finding(Severity::Note, "y")],
            ),
            verdict("c", vec![finding(Severity::Note, "y")]),
        ]);
        let agreements = r.agreements();
        assert_eq!(agreements.len(), 2);
        assert_eq!(agreements[0].locus, "y");
        assert_eq!(agreements[0].corroboration(), 3);
        assert_eq!(agreements[1].locus, "x");
    }

    /// Case and surrounding whitespace only. Anything fuzzier would
    /// merge two different objections that happen to be worded alike,
    /// and inflating a corroboration count is the one error this must
    /// not make.
    #[test]
    fn locus_matching_normalizes_case_and_whitespace_and_nothing_else() {
        let r = report(vec![
            verdict("a", vec![finding(Severity::Note, "  Section 3 ")]),
            verdict("b", vec![finding(Severity::Note, "section 3")]),
            verdict("c", vec![finding(Severity::Note, "section three")]),
        ]);
        let agreements = r.agreements();
        assert_eq!(agreements.len(), 1, "{agreements:?}");
        assert_eq!(agreements[0].corroboration(), 2);
    }

    /// A blocking finding from one reviewer is the case a panel exists
    /// for. Requiring a second vote before showing it would throw away
    /// exactly the finding the others missed.
    #[test]
    fn an_uncorroborated_blocking_finding_is_still_reported() {
        let r = report(vec![
            verdict("a", vec![finding(Severity::Blocking, "only a saw this")]),
            verdict("b", vec![]),
        ]);
        assert!(r.agreements().is_empty());
        assert_eq!(r.blocking().len(), 1);
        assert_eq!(r.blocking()[0].0, "a");
    }

    /// Two of two agreeing is a weaker claim than two of five, and the
    /// corroboration count alone cannot tell them apart.
    #[test]
    fn a_panel_that_lost_a_reviewer_is_not_complete() {
        let mut r = report(vec![verdict("a", vec![]), verdict("b", vec![])]);
        r.lenses_requested = 5;
        r.failures = vec![ReviewerFailure {
            lens: "c".into(),
            why: "no fenced JSON block".into(),
        }];
        assert!(!r.is_complete());

        r.lenses_requested = 2;
        assert!(r.is_complete());
    }

    #[test]
    fn a_stopped_panel_is_never_complete() {
        let mut r = report(vec![verdict("a", vec![])]);
        r.stopped = true;
        assert!(!r.is_complete());
    }

    #[test]
    fn severity_orders_note_below_concern_below_blocking() {
        assert!(Severity::Note < Severity::Concern);
        assert!(Severity::Concern < Severity::Blocking);
    }
}
