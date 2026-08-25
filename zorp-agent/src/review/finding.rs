//! What a reviewer agent hands back, and the rules for taking it seriously.
//!
//! A finding is a claim about the paper, not a fact about it. Two things
//! happen to it before it costs anything to verify: it must name a
//! verbatim span of the paper it is about, and that span must actually be
//! in the paper. The anchor rule is what stops a reviewer padding the
//! report with advice it could have written without reading anything.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// The paper should not be submitted or shipped with this in it.
    Blocking,
    /// A reviewer would raise it and expect an answer.
    Major,
    /// Worth fixing, would not sink the paper.
    Minor,
}

impl Severity {
    pub fn parse(s: &str) -> Option<Severity> {
        match s.trim().to_ascii_lowercase().as_str() {
            "blocking" | "critical" => Some(Severity::Blocking),
            "major" | "high" => Some(Severity::Major),
            "minor" | "low" | "nit" => Some(Severity::Minor),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Severity::Blocking => "blocking",
            Severity::Major => "major",
            Severity::Minor => "minor",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A single claim about the paper, from one dimension, anchored to a span
/// of the paper it is about.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub dimension: String,
    pub severity: Severity,
    /// What is wrong, in one sentence.
    pub claim: String,
    /// A verbatim span copied out of the paper. Not a paraphrase: it is
    /// checked against the paper text before the finding goes any further.
    pub anchor: String,
    /// Why the anchored span is a problem.
    pub evidence: String,
}

/// Why a proposed finding did not reach verification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dropped {
    /// The quoted span is not in the paper, so the reviewer either
    /// invented it or is not talking about this paper.
    AnchorNotInPaper,
    /// No quoted span at all: generic advice with nothing behind it.
    NoAnchor,
    /// The checker judged the doer's claim unsupported.
    CheckerRejected,
}

impl Dropped {
    pub fn reason(&self) -> &'static str {
        match self {
            Dropped::AnchorNotInPaper => "the quoted span is not in the paper",
            Dropped::NoAnchor => "no span of the paper was quoted",
            Dropped::CheckerRejected => "the checker judged it unsupported",
        }
    }
}

/// Collapse runs of whitespace and lowercase, so an anchor that differs
/// from the paper only in line wrapping still matches. Models re-flow
/// quoted text constantly and a byte-exact check would reject almost
/// every honest finding.
pub fn normalize(text: &str) -> String {
    text.split_whitespace()
        .map(|w| w.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The shortest anchor worth trusting. A three-word quote matches
/// somewhere in any paper, so it is no evidence that the reviewer read
/// anything.
pub const MIN_ANCHOR_WORDS: usize = 5;

/// Whether `finding` quotes a span that is really in `paper`.
pub fn anchor_is_in_paper(finding: &Finding, normalized_paper: &str) -> Result<(), Dropped> {
    let anchor = normalize(&finding.anchor);
    if anchor.split(' ').filter(|w| !w.is_empty()).count() < MIN_ANCHOR_WORDS {
        return Err(Dropped::NoAnchor);
    }
    if normalized_paper.contains(&anchor) {
        Ok(())
    } else {
        Err(Dropped::AnchorNotInPaper)
    }
}

#[derive(Debug, Deserialize)]
struct RawFinding {
    severity: String,
    claim: String,
    #[serde(default)]
    anchor: String,
    #[serde(default)]
    evidence: String,
}

#[derive(Debug, Deserialize)]
struct RawReply {
    #[serde(default)]
    findings: Vec<RawFinding>,
    /// One-based indices into the doer's numbered list that the checker
    /// judges unsupported. Only a checker fills this in.
    #[serde(default)]
    unsupported: Vec<usize>,
}

/// What one reviewer agent said.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    pub findings: Vec<Finding>,
    pub unsupported: Vec<usize>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    NoFencedBlock,
    InvalidJson(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::NoFencedBlock => write!(f, "no fenced JSON block in the agent's answer"),
            ParseError::InvalidJson(msg) => write!(f, "fenced block was not valid JSON: {msg}"),
        }
    }
}

/// Every fenced block in `text`, in order. Shared shape with
/// `validate::result`, and for the same reason: an agent may quote a
/// snippet of the paper in a fence before its own JSON block.
fn all_fenced_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        let after_start = &rest[start + 3..];
        let content_start = after_start.find('\n').map(|i| i + 1).unwrap_or(0);
        let after_open = &after_start[content_start..];
        let Some(end) = after_open.find("```") else {
            break;
        };
        blocks.push(after_open[..end].trim_end().to_string());
        rest = &after_open[end + 3..];
    }
    blocks
}

/// Parse one reviewer agent's answer. An unparseable severity is read as
/// [`Severity::Minor`]: the safe direction is to under-rank a finding,
/// never to promote one nobody graded into a blocker.
pub fn parse_reply(dimension: &str, agent_output: &str) -> Result<Reply, ParseError> {
    let blocks = all_fenced_blocks(agent_output);
    if blocks.is_empty() {
        return Err(ParseError::NoFencedBlock);
    }
    let mut last_err = None;
    let raw: RawReply = 'found: {
        for block in &blocks {
            match serde_json::from_str(block) {
                Ok(raw) => break 'found raw,
                Err(e) => last_err = Some(e),
            }
        }
        return Err(ParseError::InvalidJson(
            last_err.map(|e| e.to_string()).unwrap_or_default(),
        ));
    };

    Ok(Reply {
        findings: raw
            .findings
            .into_iter()
            .map(|f| Finding {
                dimension: dimension.to_string(),
                severity: Severity::parse(&f.severity).unwrap_or(Severity::Minor),
                claim: f.claim,
                anchor: f.anchor,
                evidence: f.evidence,
            })
            .collect(),
        unsupported: raw.unsupported,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(anchor: &str) -> Finding {
        Finding {
            dimension: "statistical-validity".to_string(),
            severity: Severity::Major,
            claim: "no error bars".to_string(),
            anchor: anchor.to_string(),
            evidence: "a single run is reported as a result".to_string(),
        }
    }

    #[test]
    fn severity_orders_blocking_first() {
        let mut v = vec![Severity::Minor, Severity::Blocking, Severity::Major];
        v.sort();
        assert_eq!(
            v,
            vec![Severity::Blocking, Severity::Major, Severity::Minor]
        );
    }

    #[test]
    fn an_anchor_really_in_the_paper_is_accepted() {
        let paper = normalize("We observe a 14% reduction in latency across the suite.");
        let f = finding("a 14% reduction in latency across");
        assert_eq!(anchor_is_in_paper(&f, &paper), Ok(()));
    }

    /// Line wrapping is the normal case, not the exception: an agent
    /// quoting a paragraph re-flows it.
    #[test]
    fn an_anchor_matches_across_different_whitespace() {
        let paper = normalize("We observe a 14% reduction\nin latency across the suite.");
        let f = finding("We  observe   a 14% reduction in latency");
        assert_eq!(anchor_is_in_paper(&f, &paper), Ok(()));
    }

    /// The check that stops a reviewer padding the report: advice with
    /// no span of the paper behind it never reaches verification.
    #[test]
    fn generic_advice_with_no_anchor_is_dropped() {
        let paper = normalize("We observe a 14% reduction in latency across the suite.");
        let f = finding("");
        assert_eq!(anchor_is_in_paper(&f, &paper), Err(Dropped::NoAnchor));
    }

    #[test]
    fn a_too_short_anchor_is_treated_as_no_anchor() {
        let paper = normalize("We observe a 14% reduction in latency across the suite.");
        let f = finding("in latency");
        assert_eq!(anchor_is_in_paper(&f, &paper), Err(Dropped::NoAnchor));
    }

    /// A reviewer quoting text the paper does not contain has either
    /// invented it or reviewed something else. Either way the finding is
    /// worthless and must not cost three refuters to disprove.
    #[test]
    fn an_invented_anchor_is_dropped() {
        let paper = normalize("We observe a 14% reduction in latency across the suite.");
        let f = finding("we ran the benchmark thirty times with fixed seeds");
        assert_eq!(
            anchor_is_in_paper(&f, &paper),
            Err(Dropped::AnchorNotInPaper)
        );
    }

    #[test]
    fn parses_a_well_formed_reply() {
        let out = "Looked at section 4.\n```json\n{\"findings\": [{\"severity\": \"blocking\", \"claim\": \"no error bars\", \"anchor\": \"we observe a 14% reduction\", \"evidence\": \"single run\"}]}\n```";
        let reply = parse_reply("statistical-validity", out).unwrap();
        assert_eq!(reply.findings.len(), 1);
        assert_eq!(reply.findings[0].severity, Severity::Blocking);
        assert_eq!(reply.findings[0].dimension, "statistical-validity");
        assert!(reply.unsupported.is_empty());
    }

    #[test]
    fn parses_an_empty_findings_list() {
        let out = "Nothing wrong here.\n```json\n{\"findings\": []}\n```";
        let reply = parse_reply("readability", out).unwrap();
        assert!(reply.findings.is_empty());
    }

    #[test]
    fn parses_a_checkers_unsupported_list() {
        let out = "```json\n{\"findings\": [], \"unsupported\": [1, 3]}\n```";
        let reply = parse_reply("readability", out).unwrap();
        assert_eq!(reply.unsupported, vec![1, 3]);
    }

    #[test]
    fn skips_a_decoy_fenced_block() {
        let out = "Here is the passage:\n```\nWe observe a 14% reduction.\n```\n```json\n{\"findings\": []}\n```";
        assert!(parse_reply("readability", out).is_ok());
    }

    #[test]
    fn an_unknown_severity_reads_as_minor_never_as_blocking() {
        let out = "```json\n{\"findings\": [{\"severity\": \"catastrophic\", \"claim\": \"c\", \"anchor\": \"a\", \"evidence\": \"e\"}]}\n```";
        let reply = parse_reply("readability", out).unwrap();
        assert_eq!(reply.findings[0].severity, Severity::Minor);
    }

    #[test]
    fn no_fenced_block_is_a_parse_error() {
        assert_eq!(
            parse_reply("readability", "I could not find anything").unwrap_err(),
            ParseError::NoFencedBlock
        );
    }

    #[test]
    fn invalid_json_is_a_parse_error() {
        let err = parse_reply("readability", "```json\n{ not json\n```").unwrap_err();
        assert!(matches!(err, ParseError::InvalidJson(_)));
    }
}
