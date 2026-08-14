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
                write!(f, "no fenced JSON block found in the agent's answer")
            }
            ParseError::InvalidJson(msg) => write!(f, "fenced block was not valid JSON: {msg}"),
            ParseError::MissingCitation { dimension } => {
                write!(f, "{dimension} has a nonzero score with no citation")
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// Pull the contents of every fenced code block (```` ``` ````...```` ``` ````,
/// optional language tag on the opening fence ignored) out of `text`, in
/// order of appearance. The task prompt asks the model to end its answer
/// with a JSON block, but nothing stops it from quoting an earlier fence
/// (a config snippet, a log line) first, so callers that need "the JSON
/// one" specifically should scan all of these rather than assuming the
/// first is the right one.
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

/// Parse the agent's final answer into a `ValidationResult`. Scans every
/// fenced block in the answer (not just the first) and tries to
/// deserialize each into the expected JSON shape, since the model may
/// quote other code fences (a config snippet, a log line, an API
/// response) before its final JSON block. Requires a citation for any
/// dimension scored above zero, the same "no citation, no claim"
/// discipline enforced here at parse time, not only prompted for.
pub fn parse_validation_result(agent_output: &str) -> Result<ValidationResult, ParseError> {
    let blocks = all_fenced_blocks(agent_output);
    if blocks.is_empty() {
        return Err(ParseError::NoFencedBlock);
    }
    let mut last_err = None;
    let raw: RawValidationResult = 'found: {
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
}
