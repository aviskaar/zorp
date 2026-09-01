use serde::Deserialize;
use std::fmt;
use zorp_track::Citation;

#[derive(Debug, Deserialize)]
struct RawValidationResult {
    redundancy_score: f64,
    redundancy_citations: Vec<Citation>,
    feasibility_score: f64,
    feasibility_citations: Vec<Citation>,
    verdict: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidationResult {
    pub redundancy_score: f64,
    pub redundancy_citations: Vec<Citation>,
    pub feasibility_score: f64,
    pub feasibility_citations: Vec<Citation>,
    pub verdict: String,
}

#[derive(Debug)]
pub enum ParseError {
    NoFencedBlock,
    InvalidJson(String),
    MissingCitation { dimension: &'static str },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::NoFencedBlock => {
                write!(f, "no result object in the agent's answer, fenced or bare")
            }
            ParseError::InvalidJson(msg) => write!(f, "result block was not valid JSON: {msg}"),
            ParseError::MissingCitation { dimension } => {
                write!(f, "{dimension} has a nonzero score with no citation")
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse the agent's final answer into a `ValidationResult`.
///
/// Reads backwards to the last block that deserializes, checking fences
/// first and bare objects only when no fence carries one. This is the
/// same extraction `investigate` uses, from the same module, for the
/// same reason: the model may quote another fence (a config snippet, a
/// log line, an API response) before its answer, may lose its closing
/// fence to truncation, and may skip the backticks altogether. Run 7's
/// corpus lost half its attempts to that last case in the sibling
/// parser, and nothing about this one made it immune
/// (`docs/DECISIONS.md`, 2026-09-01).
///
/// Requires a citation for any dimension scored above zero, the same
/// "no citation, no claim" discipline enforced here at parse time, not
/// only prompted for. Widening where the answer may be found does not
/// touch that: every candidate must still deserialize into all five
/// fields and then face the citation check below.
pub fn parse_validation_result(agent_output: &str) -> Result<ValidationResult, ParseError> {
    let blocks = crate::blocks::fenced_blocks(agent_output);
    let bare = crate::blocks::bare_objects(agent_output);
    if blocks.is_empty() && bare.is_empty() {
        return Err(ParseError::NoFencedBlock);
    }
    let parse = |b: &String| serde_json::from_str::<RawValidationResult>(b).ok();
    let Some(raw) = blocks
        .iter()
        .rev()
        .find_map(&parse)
        .or_else(|| bare.iter().rev().find_map(&parse))
    else {
        let last_err = blocks
            .iter()
            .chain(bare.iter())
            .filter_map(|b| serde_json::from_str::<RawValidationResult>(b).err())
            .next_back();
        return Err(ParseError::InvalidJson(
            last_err.map(|e| e.to_string()).unwrap_or_default(),
        ));
    };

    if raw.redundancy_score > 0.0 && raw.redundancy_citations.is_empty() {
        return Err(ParseError::MissingCitation {
            dimension: "redundancy",
        });
    }
    if raw.feasibility_score > 0.0 && raw.feasibility_citations.is_empty() {
        return Err(ParseError::MissingCitation {
            dimension: "feasibility",
        });
    }

    Ok(ValidationResult {
        redundancy_score: raw.redundancy_score,
        redundancy_citations: raw.redundancy_citations,
        feasibility_score: raw.feasibility_score,
        feasibility_citations: raw.feasibility_citations,
        verdict: raw.verdict,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap(json: &str) -> String {
        format!("Here is my finding.\n```json\n{json}\n```\n")
    }

    #[test]
    fn parses_a_well_formed_block() {
        let text = wrap(
            r#"{"redundancy_score": 20.0, "redundancy_citations": [{"text": "no prior work found", "source": "search 1"}], "feasibility_score": 85.0, "feasibility_citations": [{"text": "tooling exists", "source": "repo readme"}], "verdict": "worth investigating"}"#,
        );
        let result = parse_validation_result(&text).unwrap();
        assert_eq!(result.redundancy_score, 20.0);
        assert_eq!(result.feasibility_citations.len(), 1);
        assert_eq!(result.verdict, "worth investigating");
    }

    #[test]
    fn missing_block_errors() {
        let err = parse_validation_result("no block here at all").unwrap_err();
        assert!(matches!(err, ParseError::NoFencedBlock));
    }

    #[test]
    fn nonzero_score_with_no_citations_errors() {
        let text = wrap(
            r#"{"redundancy_score": 40.0, "redundancy_citations": [], "feasibility_score": 0.0, "feasibility_citations": [], "verdict": "unclear"}"#,
        );
        let err = parse_validation_result(&text).unwrap_err();
        assert!(matches!(
            err,
            ParseError::MissingCitation {
                dimension: "redundancy"
            }
        ));
    }

    #[test]
    fn zero_score_with_no_citations_is_fine() {
        let text = wrap(
            r#"{"redundancy_score": 0.0, "redundancy_citations": [], "feasibility_score": 0.0, "feasibility_citations": [], "verdict": "no evidence found"}"#,
        );
        assert!(parse_validation_result(&text).is_ok());
    }

    #[test]
    fn skips_a_decoy_leading_fenced_block_and_parses_the_json_one() {
        let json = r#"{"redundancy_score": 20.0, "redundancy_citations": [{"text": "no prior work found", "source": "search 1"}], "feasibility_score": 85.0, "feasibility_citations": [{"text": "tooling exists", "source": "repo readme"}], "verdict": "worth investigating"}"#;
        let text = format!(
            "Here's the config I found:\n```yaml\nkey: value\nother: 1\n```\nAnd here is my finding.\n```json\n{json}\n```\n"
        );
        let result = parse_validation_result(&text).unwrap();
        assert_eq!(result.redundancy_score, 20.0);
        assert_eq!(result.feasibility_citations.len(), 1);
        assert_eq!(result.verdict, "worth investigating");
    }

    #[test]
    fn invalid_json_in_block_errors() {
        let text = wrap("{ not json");
        let err = parse_validation_result(&text).unwrap_err();
        assert!(matches!(err, ParseError::InvalidJson(_)));
    }

    fn full_result(verdict: &str) -> String {
        format!(
            r#"{{"redundancy_score": 20.0, "redundancy_citations": [{{"text": "t", "source": "s"}}], "feasibility_score": 85.0, "feasibility_citations": [{{"text": "t", "source": "s"}}], "verdict": "{verdict}"}}"#
        )
    }

    /// The failure that cost `investigate` half a corpus run. This
    /// parser had it too.
    #[test]
    fn a_result_with_no_backticks_around_it_is_still_read() {
        let text = format!("Here is my assessment.\n{}", full_result("bare"));
        assert_eq!(parse_validation_result(&text).unwrap().verdict, "bare");
    }

    /// The answer comes last, so truncation takes the closing fence and
    /// nothing else with it.
    #[test]
    fn an_unclosed_final_fence_is_still_read() {
        let text = format!("Here it is.\n```json\n{}", full_result("truncated"));
        assert_eq!(parse_validation_result(&text).unwrap().verdict, "truncated");
    }

    /// Backwards, so a model that shows the shape before filling it in
    /// does not have its illustration recorded as its assessment.
    #[test]
    fn a_later_block_wins_over_an_earlier_complete_one() {
        let text = format!(
            "I'll answer like this:\n```json\n{}\n```\nOn reflection:\n```json\n{}\n```\n",
            full_result("first pass"),
            full_result("final")
        );
        assert_eq!(parse_validation_result(&text).unwrap().verdict, "final");
    }

    /// Widening where the answer may be found must not invent one, and
    /// must not reach past the citation rule.
    #[test]
    fn a_bare_object_still_faces_the_citation_check() {
        let text = r#"Assessment: {"redundancy_score": 40.0, "redundancy_citations": [], "feasibility_score": 0.0, "feasibility_citations": [], "verdict": "unclear"}"#;
        assert!(matches!(
            parse_validation_result(text).unwrap_err(),
            ParseError::MissingCitation {
                dimension: "redundancy"
            }
        ));
    }
}
