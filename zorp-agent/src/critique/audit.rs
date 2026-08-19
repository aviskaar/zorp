use super::claims::Claim;
use super::ledger::EvidenceLedger;
use std::fmt;
use zorp_track::CritiqueFinding;

/// Why a claim failed the audit. Every kind is decided by comparing the
/// draft to the evidence ledger, so a finding can always be traced to a
/// thing the record does or does not contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingKind {
    /// A figure appears in the draft that appears nowhere in the record,
    /// at any plausible rounding or scaling.
    NumberNotInRecord,
    /// A factual claim that rests on nothing in the record.
    UncitedClaim,
    /// A claim that cites evidence the record does not contain.
    EvidenceNotInRecord,
}

impl FindingKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            FindingKind::NumberNotInRecord => "number-not-in-record",
            FindingKind::UncitedClaim => "uncited-claim",
            FindingKind::EvidenceNotInRecord => "evidence-not-in-record",
        }
    }
}

impl fmt::Display for FindingKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub kind: FindingKind,
    /// The text in the draft the finding is about.
    pub claim: String,
    /// What is wrong with it, phrased so a human can check the call.
    pub detail: String,
}

impl Finding {
    pub fn to_record(&self) -> CritiqueFinding {
        CritiqueFinding {
            kind: self.kind.as_str().to_string(),
            claim: self.claim.clone(),
            detail: self.detail.clone(),
        }
    }
}

/// A number as prose wrote it: the value plus how many decimal places
/// were written. A draft rounds and the record does not, so `42` has to
/// be able to match a recorded `42.0` without `41.6` matching it too.
#[derive(Debug, Clone, PartialEq)]
pub struct WrittenNumber {
    pub value: f64,
    pub decimals: usize,
    /// The line it was written on, for the finding's claim text.
    pub context: String,
}

/// Words that turn the number after them into a label. "Section 3" is
/// not a claim about anything the record could hold.
const LABEL_WORDS: [&str; 12] = [
    "section", "table", "figure", "step", "part", "chapter", "appendix", "round", "version",
    "phase", "item", "footnote",
];

/// Every maximal digit run in `chars`, as
/// `(start, end, value, decimals)` over char indices. No filtering: the
/// callers decide what counts.
fn numeric_runs(chars: &[char]) -> Vec<(usize, usize, f64, usize)> {
    let mut runs = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        let mut digits = String::new();
        while i < chars.len() {
            if chars[i].is_ascii_digit() {
                digits.push(chars[i]);
                i += 1;
            } else if chars[i] == ','
                && i + 3 < chars.len()
                && chars[i + 1..i + 4].iter().all(|c| c.is_ascii_digit())
            {
                // A thousands separator, so 1,024 is one number.
                i += 1;
            } else {
                break;
            }
        }
        let mut decimals = 0;
        if i + 1 < chars.len() && chars[i] == '.' && chars[i + 1].is_ascii_digit() {
            digits.push('.');
            i += 1;
            while i < chars.len() && chars[i].is_ascii_digit() {
                digits.push(chars[i]);
                decimals += 1;
                i += 1;
            }
        }
        if let Ok(value) = digits.parse::<f64>() {
            runs.push((start, i, value, decimals));
        }
    }
    runs
}

/// Every number in `text`, with no exclusions at all. Used to scrape the
/// evidence side of the comparison, where being generous only ever
/// widens what counts as recorded.
pub fn all_numbers(text: &str) -> Vec<f64> {
    let chars: Vec<char> = text.chars().collect();
    numeric_runs(&chars).into_iter().map(|r| r.2).collect()
}

/// How far into the line an ordered-list marker runs, or 0 if there is
/// none. The `1.` starting a list item is punctuation.
fn ordered_list_marker_end(chars: &[char]) -> usize {
    let mut i = 0;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    let digit_start = i;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i > digit_start
        && i < chars.len()
        && (chars[i] == '.' || chars[i] == ')')
        && (i + 1 == chars.len() || chars[i + 1].is_whitespace())
    {
        return i + 1;
    }
    0
}

/// Whether the run at `start..end` is a number in its own right rather
/// than one field of a date, a version, a path, or an identifier.
fn is_standalone(chars: &[char], start: usize, end: usize) -> bool {
    if start > 0 {
        let prev = chars[start - 1];
        if prev.is_alphanumeric() || matches!(prev, '_' | '.' | '/' | ':' | '#') {
            return false;
        }
        // A dash between two digits joins fields (a date, a range), it
        // does not start a new number.
        if prev == '-' && start >= 2 && chars[start - 2].is_ascii_digit() {
            return false;
        }
    }
    if end < chars.len() {
        let next = chars[end];
        if next == '_' {
            return false;
        }
        if matches!(next, '.' | '-' | '/' | ':')
            && end + 1 < chars.len()
            && chars[end + 1].is_ascii_digit()
        {
            return false;
        }
        if next.is_alphabetic() {
            let suffix: String = chars[end..]
                .iter()
                .take_while(|c| c.is_alphabetic())
                .collect::<String>()
                .to_ascii_lowercase();
            // An ordinal is a position, not a measurement.
            if matches!(suffix.as_str(), "st" | "nd" | "rd" | "th") {
                return false;
            }
        }
    }
    true
}

fn in_url_token(chars: &[char], start: usize) -> bool {
    let mut left = start;
    while left > 0 && !chars[left - 1].is_whitespace() {
        left -= 1;
    }
    let mut right = start;
    while right < chars.len() && !chars[right].is_whitespace() {
        right += 1;
    }
    let token: String = chars[left..right].iter().collect();
    token.contains("://") || token.starts_with("www.")
}

fn follows_label_word(chars: &[char], start: usize) -> bool {
    let mut i = start;
    while i > 0 && chars[i - 1] == ' ' {
        i -= 1;
    }
    let word_end = i;
    while i > 0 && chars[i - 1].is_alphabetic() {
        i -= 1;
    }
    if i == word_end {
        return false;
    }
    let word: String = chars[i..word_end]
        .iter()
        .collect::<String>()
        .to_ascii_lowercase();
    LABEL_WORDS.contains(&word.as_str())
}

/// Numbers in `text` that read as claims rather than as structure.
///
/// Deliberately conservative. Every false positive here turns into a
/// revision request against a draft that was fine, so section numbers,
/// dates, versions, list markers, and URLs are all skipped rather than
/// reported and argued about.
pub fn numbers_in_prose(text: &str) -> Vec<WrittenNumber> {
    let mut out = Vec::new();
    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        // A heading numbers a section; a fence delimits one. Neither
        // asserts anything.
        if trimmed.starts_with('#') || trimmed.starts_with("```") {
            continue;
        }
        let chars: Vec<char> = raw_line.chars().collect();
        let marker_end = ordered_list_marker_end(&chars);
        for (start, end, value, decimals) in numeric_runs(&chars) {
            if start < marker_end
                || !is_standalone(&chars, start, end)
                || in_url_token(&chars, start)
                || follows_label_word(&chars, start)
            {
                continue;
            }
            out.push(WrittenNumber {
                value,
                decimals,
                context: trimmed.to_string(),
            });
        }
    }
    out
}

/// Whether `recorded` and `written` agree once both are rounded to the
/// precision the draft wrote.
fn rounds_together(written: f64, recorded: f64, decimals: usize) -> bool {
    if !written.is_finite() || !recorded.is_finite() {
        return false;
    }
    let factor = 10f64.powi(decimals.min(10) as i32);
    (written * factor).round() == (recorded * factor).round()
}

/// Whether `recorded` rounds to `written` at the precision `written` was
/// written to, allowing for a draft expressing a recorded proportion as
/// a percentage or the other way round.
///
/// The two scalings are asymmetric on purpose. Only a value that could
/// be a proportion is tried as a percentage, and only a value that could
/// be a percentage is tried as a proportion, so a recorded 3 does not
/// quietly license a drafted 300.
pub fn number_is_supported(written: &WrittenNumber, recorded: &[f64]) -> bool {
    recorded.iter().any(|r| {
        rounds_together(written.value, *r, written.decimals)
            || (r.abs() <= 1.0 && rounds_together(written.value, r * 100.0, written.decimals))
            || (r.abs() >= 1.0 && rounds_together(written.value, r / 100.0, written.decimals))
    })
}

fn format_written(n: &WrittenNumber) -> String {
    format!("{:.*}", n.decimals, n.value)
}

/// Flag every figure in the draft that the record cannot account for.
/// Runs on the draft text alone: no model is involved, so a critic that
/// says nothing is wrong cannot make this check pass.
pub fn audit_numbers(draft: &str, ledger: &EvidenceLedger) -> Vec<Finding> {
    let measured = ledger.recorded_numbers();
    let counts = ledger.recorded_counts();
    let mut findings = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for n in numbers_in_prose(draft) {
        if number_is_supported(&n, &measured) {
            continue;
        }
        // Counts of things in the record are matched exactly. Scaling
        // them the way a proportion is scaled would let a track with one
        // experiment license any drafted 100.
        if counts
            .iter()
            .any(|c| rounds_together(n.value, *c, n.decimals))
        {
            continue;
        }
        // One finding per distinct figure. A draft that repeats an
        // invented number has one problem, not five, and the round-over-
        // round comparison counts findings.
        if !seen.insert(format_written(&n)) {
            continue;
        }
        findings.push(Finding {
            kind: FindingKind::NumberNotInRecord,
            claim: n.context.clone(),
            detail: format!(
                "the draft states {}, which the evidence record does not contain at any rounding. \
                 Cite the recorded figure it comes from, show how it is derived from one, or remove it.",
                format_written(&n)
            ),
        });
    }
    findings
}

/// Collapse every run of whitespace to a single space, so a claim the
/// model reflowed still matches the draft it came from.
fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Check the claims a critic extracted against the ledger.
///
/// Returns the findings and the number of extracted claims that were
/// discarded because their text is not in the draft. A critic that
/// invents a sentence must not be able to make the pass rewrite a
/// sentence that was never there.
pub fn audit_claims(
    claims: &[Claim],
    draft: &str,
    ledger: &EvidenceLedger,
) -> (Vec<Finding>, usize) {
    let keys = ledger.evidence_keys();
    let normalized_draft = normalize_whitespace(draft);
    let mut findings: Vec<Finding> = Vec::new();
    let mut discarded = 0;
    for claim in claims {
        let text = claim.text.trim();
        if text.is_empty() || !normalized_draft.contains(&normalize_whitespace(text)) {
            discarded += 1;
            continue;
        }
        let finding = match &claim.evidence {
            None => Finding {
                kind: FindingKind::UncitedClaim,
                claim: text.to_string(),
                detail: "this claim rests on nothing in the evidence record. Attach the recorded \
                         figure or source it comes from, weaken it to what the record supports, \
                         or remove it."
                    .to_string(),
            },
            Some(key) if !keys.contains(key.as_str()) => Finding {
                kind: FindingKind::EvidenceNotInRecord,
                claim: text.to_string(),
                detail: format!(
                    "this claim cites '{key}', which the evidence record does not contain."
                ),
            },
            Some(_) => continue,
        };
        // The same sentence reported twice is one problem.
        if !findings
            .iter()
            .any(|f| f.kind == finding.kind && f.claim == finding.claim)
        {
            findings.push(finding);
        }
    }
    (findings, discarded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zorp_track::experiment::MetricValue;
    use zorp_track::validation::{Citation, Validation};

    fn ledger(metrics: &[(&str, f64)]) -> EvidenceLedger {
        EvidenceLedger {
            metrics: metrics
                .iter()
                .map(|(k, v)| ("exp-1".to_string(), k.to_string(), MetricValue::Number(*v)))
                .collect(),
            experiment_count: 1,
            ..Default::default()
        }
    }

    fn written(value: f64, decimals: usize) -> WrittenNumber {
        WrittenNumber {
            value,
            decimals,
            context: String::new(),
        }
    }

    #[test]
    fn a_figure_that_is_nowhere_in_the_record_is_a_finding() {
        let l = ledger(&[("latency_ms", 42.0)]);
        let findings = audit_numbers("Latency fell to 17ms after the change.", &l);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].kind, FindingKind::NumberNotInRecord);
        assert!(findings[0].detail.contains("17"), "{findings:?}");
    }

    #[test]
    fn a_figure_that_matches_a_recorded_metric_is_not_a_finding() {
        let l = ledger(&[("latency_ms", 42.0)]);
        assert!(audit_numbers("Latency was 42ms.", &l).is_empty());
    }

    #[test]
    fn a_draft_may_round_a_recorded_value() {
        let l = ledger(&[("latency_ms", 41.66)]);
        // Written to one decimal place, so 41.66 rounds onto it.
        assert!(audit_numbers("Latency was 41.7ms.", &l).is_empty());
        // Written to two, so it has to match to two.
        assert_eq!(audit_numbers("Latency was 41.61ms.", &l).len(), 1);
    }

    #[test]
    fn a_recorded_proportion_may_be_written_as_a_percentage() {
        let l = ledger(&[("hit_rate", 0.83)]);
        assert!(audit_numbers("The cache hit 83% of the time.", &l).is_empty());
    }

    #[test]
    fn dates_versions_list_markers_and_urls_are_not_audited() {
        let l = ledger(&[("latency_ms", 42.0)]);
        let draft = "\
# Findings 7

Run on 2026-08-18 with toolchain 1.82.0.

1. Latency was 42ms.
2. See https://example.com/reports/9912 for the harness.

See section 3 and table 2 for the raw numbers.
";
        let findings = audit_numbers(draft, &l);
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn counting_the_attempts_the_record_actually_holds_is_not_a_finding() {
        let mut l = ledger(&[("latency_ms", 42.0)]);
        l.experiment_count = 3;
        assert!(audit_numbers("Across 3 attempts, latency was 42ms.", &l).is_empty());
    }

    #[test]
    fn a_number_only_present_in_a_citation_is_supported() {
        let mut l = ledger(&[("latency_ms", 42.0)]);
        l.validation = Some(Validation {
            id: "v1".to_string(),
            track_id: "t1".to_string(),
            redundancy_score: 20.0,
            redundancy_citations: vec![Citation {
                text: "the prior benchmark reported 913 rps".to_string(),
                source: "search result 1".to_string(),
            }],
            feasibility_score: 85.0,
            feasibility_citations: vec![],
            verdict: "worth investigating".to_string(),
            created_at: 0,
        });
        assert!(audit_numbers("The prior benchmark reported 913 rps.", &l).is_empty());
    }

    #[test]
    fn precision_matching_does_not_accept_an_unrelated_value() {
        assert!(number_is_supported(&written(42.0, 0), &[42.0]));
        assert!(number_is_supported(&written(42.0, 0), &[41.6]));
        assert!(!number_is_supported(&written(42.0, 0), &[41.4]));
        assert!(!number_is_supported(&written(42.5, 1), &[42.0]));
    }

    #[test]
    fn all_numbers_takes_everything_including_structure() {
        let found = all_numbers("v1.2 on 2026-08-18, section 3");
        assert!(found.contains(&1.2), "{found:?}");
        assert!(found.contains(&3.0), "{found:?}");
    }

    #[test]
    fn a_claim_resting_on_nothing_is_uncited() {
        let l = ledger(&[("latency_ms", 42.0)]);
        let draft = "Users will notice the difference immediately.";
        let claims = vec![Claim {
            text: "Users will notice the difference immediately.".to_string(),
            evidence: None,
        }];
        let (findings, discarded) = audit_claims(&claims, draft, &l);
        assert_eq!(discarded, 0);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].kind, FindingKind::UncitedClaim);
    }

    #[test]
    fn a_claim_citing_a_key_the_record_does_not_have_is_flagged() {
        let l = ledger(&[("latency_ms", 42.0)]);
        let draft = "Throughput doubled.";
        let claims = vec![Claim {
            text: "Throughput doubled.".to_string(),
            evidence: Some("metric:throughput".to_string()),
        }];
        let (findings, _) = audit_claims(&claims, draft, &l);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].kind, FindingKind::EvidenceNotInRecord);
        assert!(findings[0].detail.contains("metric:throughput"));
    }

    #[test]
    fn a_claim_citing_a_recorded_key_is_clean() {
        let l = ledger(&[("latency_ms", 42.0)]);
        let draft = "Latency was 42ms.";
        let claims = vec![Claim {
            text: "Latency was 42ms.".to_string(),
            evidence: Some("metric:latency_ms".to_string()),
        }];
        let (findings, discarded) = audit_claims(&claims, draft, &l);
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(discarded, 0);
    }

    /// The critic is a model, and a model will occasionally hand back a
    /// sentence the draft does not contain. Revising a draft to fix a
    /// sentence that was never in it is strictly worse than doing
    /// nothing, so those claims are dropped and counted.
    #[test]
    fn a_claim_whose_text_is_not_in_the_draft_is_discarded() {
        let l = ledger(&[("latency_ms", 42.0)]);
        let draft = "Latency was 42ms.";
        let claims = vec![Claim {
            text: "Latency was 9000ms and the servers caught fire.".to_string(),
            evidence: None,
        }];
        let (findings, discarded) = audit_claims(&claims, draft, &l);
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(discarded, 1);
    }

    #[test]
    fn claim_matching_tolerates_whitespace_the_model_reflowed() {
        let l = ledger(&[("latency_ms", 42.0)]);
        let draft = "Latency was 42ms\nafter the change.";
        let claims = vec![Claim {
            text: "Latency was 42ms after the change.".to_string(),
            evidence: None,
        }];
        let (findings, discarded) = audit_claims(&claims, draft, &l);
        assert_eq!(discarded, 0, "reflowed whitespace is still the same claim");
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn an_empty_claim_list_still_leaves_the_number_audit_in_force() {
        // A lazy or evasive critic returning no claims must not be able
        // to declare a draft clean: the deterministic half runs anyway.
        let l = ledger(&[("latency_ms", 42.0)]);
        let draft = "Latency fell to 17ms.";
        let (findings, _) = audit_claims(&[], draft, &l);
        assert!(findings.is_empty());
        assert_eq!(audit_numbers(draft, &l).len(), 1);
    }
}
