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
                write!(f, "no result object in the agent's answer, fenced or bare")
            }
            ParseError::InvalidJson(msg) => write!(f, "result block was not valid JSON: {msg}"),
            ParseError::MissingMetricValue => {
                write!(f, "result block has no finite metric_value")
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// Whether `body` is the answer rather than something that resembles it.
///
/// Both fields, and a `metric_value` that is a finite number. The
/// finiteness is not decoration: this number is compared against the
/// pre-registered kill threshold and fed to every calibration statistic
/// downstream, and one infinity there turns the sigma, the surprise and
/// the coverage counts to nonsense for the whole track.
fn is_result_shaped(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    value
        .get("metric_value")
        .and_then(|v| v.as_f64())
        .is_some_and(f64::is_finite)
        && value.get("summary").is_some_and(|v| v.is_string())
}

/// Parse the agent's final answer into an `AttemptResult`.
///
/// Reads backwards to the last block that is actually a result, checks
/// fences first and bare objects only when no fence carries one. Every
/// part of that sentence was paid for by a discarded run.
///
/// **Backwards, not forwards.** A model that fills in the requested
/// shape while explaining itself ("I'll report {"metric_value": 0, ...}
/// once I've measured it") used to have that placeholder parsed as its
/// answer, because the old scan took the first block carrying a
/// `metric_value` and stopped. That is the worse of the two failures
/// here: it does not fail the attempt, it records a wrong number in the
/// evidence record and moves on. Same reason `panel` reads verdicts
/// backwards and `parse_forecast` was changed to.
///
/// **Bare objects too.** Run 7's corpus lost 10 of 20 attempts to
/// `no fenced JSON block found in the agent's answer`, every one of them
/// the same small local model, and their raw text held a complete result
/// object with no backticks around it. The fence is the model's
/// punctuation, not its answer (`docs/DECISIONS.md`, 2026-09-01).
///
/// This loosens where the answer may be found and nothing else.
/// `is_result_shaped` demands both fields and a finite number, so a
/// block that only resembles a result is stepped over rather than
/// repaired. `metric_value` is still required: an answer without one is
/// a scoring failure, never a silent zero.
pub fn parse_attempt_result(agent_output: &str) -> Result<AttemptResult, ParseError> {
    let blocks = crate::blocks::fenced_blocks(agent_output);
    let bare = crate::blocks::bare_objects(agent_output);
    if blocks.is_empty() && bare.is_empty() {
        return Err(ParseError::NoFencedBlock);
    }

    if let Some(block) = blocks
        .iter()
        .rev()
        .find(|b| is_result_shaped(b))
        .or_else(|| bare.iter().rev().find(|b| is_result_shaped(b)))
    {
        // Shaped, so this cannot fail; the shape check already parsed it
        // and confirmed both fields.
        if let Ok(raw) = serde_json::from_str::<RawAttemptResult>(block) {
            if let Some(metric_value) = raw.metric_value {
                return Ok(AttemptResult {
                    metric_value,
                    summary: raw.summary,
                });
            }
        }
    }

    // Nothing was the answer. Work out which way it fell, so the failure
    // names what the model actually did: an answer shaped like a result
    // but missing its number is a different problem from one that was
    // never JSON, and reporting them the same way is what made the
    // corpus failures unreadable until somebody counted them by hand.
    let mut last_err = None;
    let mut saw_shaped_block = false;
    for block in blocks.iter().chain(bare.iter()) {
        match serde_json::from_str::<RawAttemptResult>(block) {
            // Anything that parses and reaches here either had no
            // `metric_value` or had one that was not finite, since a
            // usable one would have been returned above. Same bucket
            // either way: the answer had the shape and no number in it.
            Ok(_) => saw_shaped_block = true,
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

    // The corpus failures. Run 7 lost 10 of 20 attempts here, all of
    // them the same small local model and all of them reported as "no
    // fenced JSON block found in the agent's answer".

    /// The whole of the reported bug. The model answered with exactly
    /// the object it was asked for and no backticks around it, and half
    /// a corpus run was discarded over the punctuation.
    #[test]
    fn a_result_with_no_backticks_around_it_is_still_read() {
        let text = "I measured it and got the number below.\n\
                    {\"metric_value\": 12.5, \"summary\": \"measured\"}";
        let result = parse_attempt_result(text).unwrap();
        assert_eq!(result.metric_value, 12.5);
        assert_eq!(result.summary, "measured");
    }

    /// The model is asked for its answer last, so a truncated response
    /// loses its closing fence and nothing else.
    #[test]
    fn an_unclosed_final_fence_is_still_read() {
        let text = "Here it is.\n```json\n{\"metric_value\": 5.0, \"summary\": \"done\"}";
        assert_eq!(parse_attempt_result(text).unwrap().metric_value, 5.0);
    }

    /// The dangerous one. A model that fills in the requested shape
    /// while narrating used to have its placeholder recorded as the
    /// answer, which does not fail the attempt: it writes a wrong number
    /// into the evidence record and carries on.
    #[test]
    fn a_filled_in_placeholder_does_not_beat_the_real_answer() {
        let text = format!(
            "I'll report it like this once I've measured:\n```json\n{}\n```\n\
             Now measuring... done.\n```json\n{}\n```\n",
            r#"{"metric_value": 0, "summary": "placeholder"}"#,
            r#"{"metric_value": 88.0, "summary": "the real one"}"#
        );
        let result = parse_attempt_result(&text).unwrap();
        assert_eq!(result.metric_value, 88.0);
        assert_eq!(result.summary, "the real one");
    }

    /// A fence wins over a bare object even when the bare one comes
    /// later, because the fence is what was asked for and a model that
    /// mentions an object afterwards is still commenting.
    #[test]
    fn a_fenced_result_beats_a_bare_one_that_follows_it() {
        let text = "```json\n{\"metric_value\": 1.0, \"summary\": \"fenced\"}\n```\n\
                    For reference the shape is {\"metric_value\": 9.9, \"summary\": \"aside\"}";
        assert_eq!(parse_attempt_result(text).unwrap().metric_value, 1.0);
    }

    /// A metric that is not a number is refused rather than coerced. A
    /// model quoting its result ("12.5") must not have that recorded as
    /// a measurement.
    #[test]
    fn a_metric_that_is_not_a_number_is_refused_rather_than_recorded() {
        for body in [
            r#"{"metric_value": "12.5", "summary": "quoted"}"#,
            // Out of f64 range. serde_json refuses this before it can
            // become an infinity, which is why the `is_finite` check in
            // `is_result_shaped` is a belt on top of braces rather than
            // the thing doing the work.
            r#"{"metric_value": 1e400, "summary": "overflowed"}"#,
        ] {
            assert!(
                parse_attempt_result(&wrap(body)).is_err(),
                "{body} should not have been recorded"
            );
        }
    }

    /// Prose with no object anywhere is still a failure. Widening where
    /// an answer may be found must not invent one.
    #[test]
    fn prose_with_no_object_is_still_a_failure() {
        let err = parse_attempt_result("The latency was about twelve milliseconds.").unwrap_err();
        assert!(matches!(err, ParseError::NoFencedBlock));
    }
}
