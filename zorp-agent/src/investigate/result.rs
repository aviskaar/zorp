use serde::Deserialize;
use std::fmt;

#[derive(Debug, Deserialize)]
struct RawAttemptResult {
    metric_value: Option<f64>,
    summary: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttemptResult {
    pub metric_value: f64,
    pub summary: String,
}

#[derive(Debug)]
pub enum ParseError {
    NoFencedBlock,
    InvalidJson(String),
    MissingMetricValue,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::NoFencedBlock => {
                write!(f, "no fenced JSON block found in the agent's answer")
            }
            ParseError::InvalidJson(msg) => write!(f, "fenced block was not valid JSON: {msg}"),
            ParseError::MissingMetricValue => write!(f, "fenced block has no metric_value"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Pull the contents of every fenced code block out of `text`, in order
/// of appearance. Mirrors `validate::result::all_fenced_blocks`
/// (duplicated rather than shared: that copy is private to the
/// `validate` module, and the result shapes the two modules parse
/// differ), for the same reason: the model may quote another fenced
/// block (a log line, a config snippet) before its final JSON answer.
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

/// Parse the agent's final answer into an `AttemptResult`. Scans every
/// fenced block (not just the first) and tries to deserialize each into
/// the expected shape, same discipline as `validate::result::
/// parse_validation_result`. `metric_value` is required: a block that
/// parses as JSON but omits it (or the model's answer had no valid
/// block at all) is a scoring failure, not a silent zero.
pub fn parse_attempt_result(agent_output: &str) -> Result<AttemptResult, ParseError> {
    let blocks = all_fenced_blocks(agent_output);
    if blocks.is_empty() {
        return Err(ParseError::NoFencedBlock);
    }
    let mut last_err = None;
    let mut saw_shaped_block = false;
    for block in &blocks {
        match serde_json::from_str::<RawAttemptResult>(block) {
            // A block that deserializes but has no metric_value is a
            // decoy, not the answer: keep scanning, in case a later block
            // carries both fields. MissingMetricValue is only reported if
            // no block in the whole answer had one.
            Ok(raw) => match raw.metric_value {
                Some(metric_value) => {
                    return Ok(AttemptResult {
                        metric_value,
                        summary: raw.summary,
                    })
                }
                None => saw_shaped_block = true,
            },
            Err(e) => last_err = Some(e),
        }
    }
    if saw_shaped_block {
        return Err(ParseError::MissingMetricValue);
    }
    Err(ParseError::InvalidJson(
        last_err.map(|e| e.to_string()).unwrap_or_default(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap(json: &str) -> String {
        format!("Here is my finding.\n```json\n{json}\n```\n")
    }

    #[test]
    fn parses_a_well_formed_block() {
        let text = wrap(r#"{"metric_value": 42.5, "summary": "latency improved"}"#);
        let result = parse_attempt_result(&text).unwrap();
        assert_eq!(result.metric_value, 42.5);
        assert_eq!(result.summary, "latency improved");
    }

    #[test]
    fn missing_block_errors() {
        let err = parse_attempt_result("no block here at all").unwrap_err();
        assert!(matches!(err, ParseError::NoFencedBlock));
    }

    #[test]
    fn missing_metric_value_errors() {
        let text = wrap(r#"{"summary": "no number given"}"#);
        let err = parse_attempt_result(&text).unwrap_err();
        assert!(matches!(err, ParseError::MissingMetricValue));
    }

    #[test]
    fn skips_a_decoy_leading_fenced_block_and_parses_the_json_one() {
        let text = format!(
            "Here's the config I found:\n```yaml\nkey: value\n```\nAnd here is my finding.\n```json\n{}\n```\n",
            r#"{"metric_value": 7.0, "summary": "done"}"#
        );
        let result = parse_attempt_result(&text).unwrap();
        assert_eq!(result.metric_value, 7.0);
    }

    #[test]
    fn skips_a_decoy_block_that_has_a_summary_but_no_metric_value() {
        let text = format!(
            "First, a note:\n```json\n{}\n```\nAnd here is my finding.\n```json\n{}\n```\n",
            r#"{"summary": "still working on it"}"#, r#"{"metric_value": 3.5, "summary": "done"}"#
        );
        let result = parse_attempt_result(&text).unwrap();
        assert_eq!(result.metric_value, 3.5);
        assert_eq!(result.summary, "done");
    }

    #[test]
    fn invalid_json_in_block_errors() {
        let text = wrap("{ not json");
        let err = parse_attempt_result(&text).unwrap_err();
        assert!(matches!(err, ParseError::InvalidJson(_)));
    }
}
