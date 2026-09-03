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
                write!(f, "no claims object in the critic's answer, fenced or bare")
            }
            ParseError::InvalidJson(msg) => write!(f, "claims object was not valid JSON: {msg}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Whether `body` has the one field a claims answer requires.
fn is_claims_shaped(body: &str) -> bool {
    serde_json::from_str::<RawClaims>(body).is_ok()
}

/// Parse a critic's answer into the claims it extracted.
///
/// Checks every candidate, last first, because the critic may quote the
/// draft before answering. Fences are preferred, but a bare object is
/// still an answer when it has the required `claims` array.
pub fn parse_claims(agent_output: &str) -> Result<Vec<Claim>, ParseError> {
    let blocks = crate::blocks::fenced_blocks(agent_output);
    let bare = crate::blocks::bare_objects(agent_output);
    if blocks.is_empty() && bare.is_empty() {
        return Err(ParseError::NoFencedBlock);
    }

    let found = blocks
        .iter()
        .rev()
        .find(|block| is_claims_shaped(block))
        .or_else(|| bare.iter().rev().find(|block| is_claims_shaped(block)));
    let Some(block) = found else {
        let last_err = blocks
            .iter()
            .chain(bare.iter())
            .filter_map(|block| serde_json::from_str::<RawClaims>(block).err())
            .next_back();
        return Err(ParseError::InvalidJson(
            last_err.map(|e| e.to_string()).unwrap_or_default(),
        ));
    };

    // Shaped, so this cannot fail: the shape check parsed it and
    // confirmed the required field.
    let raw = serde_json::from_str::<RawClaims>(block).expect("shaped claims object");
    Ok(raw
        .claims
        .into_iter()
        .map(|c| Claim {
            text: c.claim,
            evidence: c.evidence.filter(|e| !e.trim().is_empty()),
        })
        .collect())
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

    #[test]
    fn a_bare_claims_object_is_still_read() {
        let text = r#"The extracted claims are {"claims": [{"claim": "Latency was 42ms."}]}"#;
        let claims = parse_claims(text).unwrap();
        assert_eq!(claims[0].text, "Latency was 42ms.");
    }

    #[test]
    fn an_unclosed_final_fence_is_still_read() {
        let text =
            "Here is my answer.\n```json\n{\"claims\": [{\"claim\": \"Latency was 42ms.\"}]}";
        let claims = parse_claims(text).unwrap();
        assert_eq!(claims[0].text, "Latency was 42ms.");
    }
}
