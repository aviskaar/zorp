use serde::Deserialize;
use std::fmt;

/// One factual claim a critic found in the draft, and the evidence key
/// it says the claim rests on. The critic extracts, it does not judge:
/// whether the key is real is decided in `audit`, against the ledger.
#[derive(Debug, Clone, PartialEq)]
pub struct Claim {
    pub text: String,
    pub evidence: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawClaims {
    claims: Vec<RawClaim>,
}

#[derive(Debug, Deserialize)]
struct RawClaim {
    claim: String,
    #[serde(default)]
    evidence: Option<String>,
}

#[derive(Debug)]
pub enum ParseError {
    NoFencedBlock,
    InvalidJson(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::NoFencedBlock => {
                write!(f, "no fenced JSON block found in the critic's answer")
            }
            ParseError::InvalidJson(msg) => write!(f, "fenced block was not valid JSON: {msg}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Pull the contents of every fenced code block out of `text`, in order
/// of appearance. A third copy of the scanner `validate::result` and
/// `investigate::result` already carry: those two are private to their
/// modules and are covered by their own tests, and folding all three
/// into one helper means editing two tested modules for no behaviour
/// change. Worth doing, but not in the change that adds the third.
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

/// Parse a critic's answer into the claims it extracted. Scans every
/// fenced block, not just the first, for the same reason the other two
/// parsers do: the model may quote the draft in a fence before its
/// answer.
pub fn parse_claims(agent_output: &str) -> Result<Vec<Claim>, ParseError> {
    let blocks = all_fenced_blocks(agent_output);
    if blocks.is_empty() {
        return Err(ParseError::NoFencedBlock);
    }
    let mut last_err = None;
    for block in &blocks {
        match serde_json::from_str::<RawClaims>(block) {
            Ok(raw) => {
                return Ok(raw
                    .claims
                    .into_iter()
                    .map(|c| Claim {
                        text: c.claim,
                        evidence: c.evidence.filter(|e| !e.trim().is_empty()),
                    })
                    .collect())
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(ParseError::InvalidJson(
        last_err.map(|e| e.to_string()).unwrap_or_default(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap(json: &str) -> String {
        format!("Here is what I found.\n```json\n{json}\n```\n")
    }

    #[test]
    fn parses_a_well_formed_block() {
        let text = wrap(
            r#"{"claims": [{"claim": "Latency was 42ms.", "evidence": "metric:latency_ms"}, {"claim": "Users will notice.", "evidence": null}]}"#,
        );
        let claims = parse_claims(&text).unwrap();
        assert_eq!(claims.len(), 2);
        assert_eq!(claims[0].evidence.as_deref(), Some("metric:latency_ms"));
        assert_eq!(claims[1].evidence, None);
    }

    #[test]
    fn an_empty_claim_list_parses_rather_than_erroring() {
        // "I found no factual claims" is a real answer about a draft
        // that is all framing, and it is not a parse failure.
        let claims = parse_claims(&wrap(r#"{"claims": []}"#)).unwrap();
        assert!(claims.is_empty());
    }

    #[test]
    fn a_missing_evidence_field_reads_as_uncited() {
        let claims = parse_claims(&wrap(r#"{"claims": [{"claim": "Something."}]}"#)).unwrap();
        assert_eq!(claims[0].evidence, None);
    }

    #[test]
    fn a_blank_evidence_string_reads_as_uncited_not_as_a_key() {
        let claims = parse_claims(&wrap(
            r#"{"claims": [{"claim": "Something.", "evidence": "  "}]}"#,
        ))
        .unwrap();
        assert_eq!(claims[0].evidence, None);
    }

    #[test]
    fn missing_block_errors() {
        let err = parse_claims("no block here at all").unwrap_err();
        assert!(matches!(err, ParseError::NoFencedBlock));
    }

    #[test]
    fn invalid_json_in_block_errors() {
        let err = parse_claims(&wrap("{ not json")).unwrap_err();
        assert!(matches!(err, ParseError::InvalidJson(_)));
    }

    #[test]
    fn skips_a_decoy_leading_fenced_block() {
        let text = format!(
            "The draft says:\n```\nLatency was 42ms.\n```\nAnd here is my answer.\n```json\n{}\n```\n",
            r#"{"claims": [{"claim": "Latency was 42ms.", "evidence": "metric:latency_ms"}]}"#
        );
        let claims = parse_claims(&text).unwrap();
        assert_eq!(claims.len(), 1);
    }
}
