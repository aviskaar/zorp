//! The forecast that makes an anomaly possible.
//!
//! aryabhatta measures surprise as the distance between what was
//! expected and what happened. Before this module nothing ever wrote an
//! expectation, so `expectations` was always empty, the calibration
//! report always answered `NotEnoughEvidence`, and the re-run gate had
//! nothing to gate. Every reader was built and none of them could ever
//! see anything.
//!
//! The one rule that makes a forecast worth recording is that it is
//! made before the outcome exists. `record_expectation` refuses one
//! after the fact, which stops the database being lied to, but it
//! cannot tell a real forecast from a number the model produced in the
//! same breath as the result. So the forecast runs as its own agent,
//! with no tools and one step, against a prompt that does not contain
//! the task's findings, and it runs before the working agent starts. A
//! forecast asked for alongside the answer would satisfy the guard and
//! mean nothing.

use crate::agent::{Agent, Outcome};
use crate::model::Model;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// A forecast about one metric, before the experiment runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Forecast {
    pub expected_value: f64,
    pub interval_low: f64,
    pub interval_high: f64,
    pub confidence: f64,
}

/// Why a forecast could not be read back.
#[derive(Debug, Clone, PartialEq)]
pub enum ForecastError {
    /// The answer carried no fenced JSON block.
    NoFencedBlock,
    /// The block was not JSON, or not the shape asked for.
    InvalidJson(String),
    /// A number was missing, not a number, or not finite.
    BadNumber(&'static str),
    /// The interval does not contain its own expected value, or the
    /// stated confidence is not a probability.
    Incoherent(String),
}

impl std::fmt::Display for ForecastError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ForecastError::NoFencedBlock => {
                write!(f, "no forecast object in the answer, fenced or bare")
            }
            ForecastError::InvalidJson(e) => {
                write!(f, "forecast block is not the shape asked for: {e}")
            }
            ForecastError::BadNumber(field) => write!(
                f,
                "forecast field {field} is missing or not a finite number"
            ),
            ForecastError::Incoherent(why) => write!(f, "forecast is not coherent: {why}"),
        }
    }
}

impl std::error::Error for ForecastError {}

const FORECAST_SYSTEM_PROMPT: &str = "\
You state a quantitative forecast and nothing else. You have no tools \
and you are not being asked to do the work. You are being asked what \
you expect the result to be, before anyone does it.\n\n\
An honest interval is one you would be willing to be scored on. Stating \
an enormous interval to be safe is not caution, it is refusing to \
answer: interval width is recorded and reported next to coverage, so a \
forecast that cannot be wrong is visibly worthless.";

/// The prompt a forecaster sees.
///
/// It carries the hypothesis and the metric name and nothing about how
/// the work will be done, because a forecast conditioned on the working
/// agent's findings is a postdiction wearing the right timestamp.
pub fn forecast_prompt(hypothesis: &str, metric_name: &str) -> String {
    format!(
        "An experiment is about to be run to test this hypothesis:\n\n\
         {hypothesis}\n\n\
         It will report one number, the metric named '{metric_name}'.\n\n\
         State what you expect that number to be, before it is measured.\n\n\
         Answer with a single fenced JSON block, exactly this shape:\n\
         ```json\n\
         {{\"expected_value\": <number>, \"interval_low\": <number>, \
         \"interval_high\": <number>, \"confidence\": <number between 0 and 1>}}\n\
         ```\n\n\
         `confidence` is the probability you assign to the true value \
         landing inside your interval, for example 0.80. Do not write \
         anything after the block."
    )
}

/// The last fenced block in `text`, parsed as a forecast.
///
/// Last, not first, for the same reason `panel` reads verdicts
/// backwards: a model that quotes the requested shape while explaining
/// itself would otherwise have its example parsed as its answer.
pub fn parse_forecast(text: &str) -> Result<Forecast, ForecastError> {
    let blocks = fenced_blocks(text);
    let bare = bare_objects(text);
    let last = blocks
        .last()
        .or_else(|| bare.last())
        .ok_or(ForecastError::NoFencedBlock)?;

    // Backwards to the last block that is actually a forecast, rather than
    // straight to the last block of any kind. A model that closes with an
    // empty fence, or with its caveats in one, used to throw its own answer
    // away: the empty body reached serde as "" and came back as "EOF while
    // parsing a value at line 1 column 0". That was 11 of the 25 discarded
    // attempts in the 60-directory registry run.
    //
    // This loosens which block is read and nothing else. `is_forecast_shaped`
    // demands all four numbers be present and finite, so a block that only
    // resembles a forecast is stepped over rather than repaired, and the one
    // that is selected still faces every coherence check below. A forecast
    // that cannot be read must never become one that was invented.
    //
    // A forecast that arrived without backticks is found the same way, and
    // only when no fence carries one. Run 9 discarded 11 of its 22 failed
    // attempts as "no fenced json block" while their raw text held a
    // complete forecast object, and scored 2 overall. The fence is the
    // model's punctuation, not its answer.
    let block = blocks
        .iter()
        .rev()
        .find(|b| is_forecast_shaped(b))
        .or_else(|| bare.iter().rev().find(|b| is_forecast_shaped(b)))
        .unwrap_or(last);

    let value: serde_json::Value =
        serde_json::from_str(block).map_err(|e| ForecastError::InvalidJson(e.to_string()))?;

    let number = |field: &'static str| -> Result<f64, ForecastError> {
        value
            .get(field)
            .and_then(|v| v.as_f64())
            .filter(|n| n.is_finite())
            .ok_or(ForecastError::BadNumber(field))
    };

    let expected_value = number("expected_value")?;
    let interval_low = number("interval_low")?;
    let interval_high = number("interval_high")?;
    let confidence = number("confidence")?;

    // Checked here rather than left to `record_expectation`, so a
    // malformed forecast is reported as a forecast problem instead of
    // surfacing later as a store error with no context. The store
    // checks them again; this is not a substitute for that.
    if interval_low > interval_high {
        return Err(ForecastError::Incoherent(format!(
            "interval_low {interval_low} is above interval_high {interval_high}"
        )));
    }
    if expected_value < interval_low || expected_value > interval_high {
        return Err(ForecastError::Incoherent(format!(
            "expected_value {expected_value} is outside its own interval [{interval_low}, {interval_high}]"
        )));
    }
    if confidence <= 0.0 || confidence >= 1.0 {
        return Err(ForecastError::Incoherent(format!(
            "confidence {confidence} is not a probability strictly between 0 and 1"
        )));
    }

    Ok(Forecast {
        expected_value,
        interval_low,
        interval_high,
        confidence,
    })
}

/// Whether a block body is a forecast rather than prose, an empty fence,
/// or some other JSON. All four numbers present and finite, which is the
/// same bar `parse_forecast` applies; anything less is not a candidate.
fn is_forecast_shaped(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    [
        "expected_value",
        "interval_low",
        "interval_high",
        "confidence",
    ]
    .iter()
    .all(|field| {
        value
            .get(field)
            .and_then(|v| v.as_f64())
            .is_some_and(f64::is_finite)
    })
}

/// Every fenced block body in `text`, in order.
///
/// A fence left open at end of input still yields its body. The model is
/// asked for the forecast last, so a truncated answer loses its closing
/// fence and nothing else, and dropping a body that parses because three
/// backticks never arrived is throwing away the answer over punctuation.
/// That was 8 of the 25 discarded attempts in the registry run, all of
/// them reported as "no fenced json block in the forecast".
fn fenced_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut inside: Option<Vec<&str>> = None;
    for line in text.lines() {
        let fence = line.trim_start().starts_with("```");
        match (&mut inside, fence) {
            (None, true) => inside = Some(Vec::new()),
            (Some(body), true) => {
                blocks.push(body.join("\n"));
                inside = None;
            }
            (Some(body), false) => body.push(line),
            (None, false) => {}
        }
    }
    if let Some(body) = inside {
        blocks.push(body.join("\n"));
    }
    blocks
}

/// Every balanced `{...}` span in `text`, in order.
///
/// The fallback for a model that answers with the right object and no
/// backticks. Balanced rather than regular: a forecast is flat today, but a
/// scan that stops at the first `}` would silently truncate the moment one
/// nests, and a truncated object parses as nothing rather than as something
/// wrong. Quotes and escapes are tracked so a brace inside a string cannot
/// open or close a span.
///
/// This finds candidates and judges none of them. Every span still faces
/// `is_forecast_shaped` and then every coherence check, so widening where a
/// forecast may be found does not widen what counts as one. A forecast that
/// cannot be read must never become one that was invented.
fn bare_objects(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = balanced_end(bytes, i) {
                spans.push(text[i..end].to_string());
                i = end;
                continue;
            }
        }
        i += 1;
    }
    spans
}

/// The index just past the `}` closing the object that opens at `start`,
/// or `None` when it never closes.
fn balanced_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, byte) in bytes.iter().enumerate().skip(start) {
        if in_string {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// Ask for a forecast on a fresh agent with no tools and one step.
///
/// Returns `Ok(None)` when the model produced nothing usable. A
/// forecast that cannot be read is not worth failing an investigation
/// over: the run still happened and its outcome is still worth
/// recording, and an experiment with no expectation is simply one the
/// calibration report does not score. It must never be worth *faking*,
/// which is why nothing here substitutes a default interval.
pub fn ask(
    model: &dyn Model,
    hypothesis: &str,
    metric_name: &str,
    cwd: PathBuf,
) -> Result<Option<Forecast>, ForecastError> {
    let mut agent = Agent::new(
        model.clone_box(),
        FORECAST_SYSTEM_PROMPT,
        1,
        cwd,
        Arc::new(AtomicBool::new(false)),
        crate::approval::ApprovalMode::AutoApprove,
    )
    // No tools at all. A forecaster that can read the repository can
    // find last run's number and report it, which is measurement
    // dressed as prediction.
    .register_builtins_filtered(Some(&[]));

    match agent.run(&forecast_prompt(hypothesis, metric_name)) {
        Outcome::Complete(text) => parse_forecast(&text).map(Some),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(body: &str) -> String {
        format!("Here is my forecast.\n\n```json\n{body}\n```\n")
    }

    /// A fence and its body on one line.
    ///
    /// `fenced_blocks` is line based, so a line that opens and closes a fence
    /// around its own body toggles twice and captures nothing between. The
    /// empty body then reached serde as "" and came back as "EOF while
    /// parsing a value at line 1 column 0", reported as "not the shape asked
    /// for". Four of run 8's fourteen discards looked like this, and the
    /// forecast was sitting in the text the whole time:
    ///
    /// ```text
    /// ...7919 lines** across the 19 top-level `.rs` files. ```json
    /// {"expected_value": 7919, ...} ``` ```
    /// ```
    ///
    /// The bare scan rescues it, because it reads the text and not the
    /// fences. Recorded as its own test because it is a different failure
    /// from an unfenced answer and would come back on its own.
    #[test]
    fn a_fence_and_its_body_on_one_line_is_still_read() {
        let text = "I counted 7919 lines across the 19 top-level files. \
                    ```json {\"expected_value\": 7919, \"interval_low\": 7899, \
                    \"interval_high\": 7939, \"confidence\": 0.95} ``` ```";
        let forecast = parse_forecast(text).expect("a one-line fenced forecast should parse");
        assert_eq!(forecast.expected_value, 7919.0);
        assert_eq!(forecast.interval_low, 7899.0);
        assert_eq!(forecast.confidence, 0.95);
    }

    /// A forecast the model did not fence is still a forecast.
    ///
    /// Measured, not imagined: run 9 on nemotron-3-super-120b discarded 11 of
    /// its 22 failed attempts as "no fenced json block", and the raw text of
    /// several of them was a complete, correct forecast object that simply
    /// arrived without backticks. That run scored 2. Refusing those is
    /// refusing evidence the model did produce, over punctuation it was never
    /// going to be reliable about.
    #[test]
    fn an_unfenced_forecast_is_read() {
        let text = "After counting the files I estimate the total.\n\n\
                    {\"expected_value\": 1750, \"interval_low\": 1600, \
                    \"interval_high\": 1900, \"confidence\": 0.85}\n";
        let forecast = parse_forecast(text).expect("an unfenced forecast should parse");
        assert_eq!(forecast.expected_value, 1750.0);
        assert_eq!(forecast.confidence, 0.85);
    }

    /// The last one, for the same reason a fenced forecast is read backwards:
    /// a model that restates the requested shape while explaining itself must
    /// not have its example parsed as its answer.
    #[test]
    fn the_last_unfenced_forecast_wins() {
        let text = "The shape I will use is \
                    {\"expected_value\": 1, \"interval_low\": 0, \
                    \"interval_high\": 2, \"confidence\": 0.5}. \
                    My actual answer is \
                    {\"expected_value\": 900, \"interval_low\": 850, \
                    \"interval_high\": 950, \"confidence\": 0.9}";
        let forecast = parse_forecast(text).expect("should parse");
        assert_eq!(forecast.expected_value, 900.0);
    }

    /// A fenced forecast still wins. This loosens where a forecast may be
    /// found and changes nothing about which one is preferred.
    #[test]
    fn a_fenced_forecast_beats_a_bare_one() {
        let text = format!(
            "{{\"expected_value\": 1, \"interval_low\": 0, \"interval_high\": 2, \"confidence\": 0.5}}\n{}",
            block(
                r#"{"expected_value": 42, "interval_low": 40, "interval_high": 44, "confidence": 0.7}"#
            )
        );
        let forecast = parse_forecast(&text).expect("should parse");
        assert_eq!(forecast.expected_value, 42.0);
    }

    /// Loosening where a forecast is found must not loosen what counts as
    /// one. An unfenced object still faces every coherence check, so the
    /// certainty nemotron sometimes claims is refused exactly as before.
    #[test]
    fn an_unfenced_forecast_still_faces_the_coherence_checks() {
        let text = "{\"expected_value\": 380, \"interval_low\": 380, \
                    \"interval_high\": 380, \"confidence\": 1.0}";
        let err = parse_forecast(text).expect_err("confidence 1.0 must still be refused");
        assert!(
            matches!(err, ForecastError::Incoherent(_)),
            "expected an incoherence error, got {err:?}"
        );
    }

    /// Prose with no forecast anywhere in it is still nothing, and still says
    /// so. An unreadable answer must never become an invented forecast.
    #[test]
    fn prose_without_any_forecast_is_still_reported_as_missing() {
        let err = parse_forecast("I could not determine the line count.")
            .expect_err("prose is not a forecast");
        assert!(matches!(err, ForecastError::NoFencedBlock), "got {err:?}");
    }

    #[test]
    fn a_well_formed_forecast_parses() {
        let text = block(
            r#"{"expected_value": 0.8, "interval_low": 0.7, "interval_high": 0.9, "confidence": 0.8}"#,
        );

        assert_eq!(
            parse_forecast(&text).unwrap(),
            Forecast {
                expected_value: 0.8,
                interval_low: 0.7,
                interval_high: 0.9,
                confidence: 0.8,
            }
        );
    }

    /// The example-in-the-prose case. Reading the first block instead of
    /// the last makes this return the placeholder shape.
    #[test]
    fn the_last_block_is_the_answer() {
        let text = format!(
            "The shape you asked for is\n\n```json\n{}\n```\n\nand my actual forecast is\n\n```json\n{}\n```\n",
            r#"{"expected_value": 0.0, "interval_low": 0.0, "interval_high": 0.0, "confidence": 0.5}"#,
            r#"{"expected_value": 0.42, "interval_low": 0.4, "interval_high": 0.44, "confidence": 0.9}"#,
        );

        assert_eq!(parse_forecast(&text).unwrap().expected_value, 0.42);
    }

    /// The 11 EOF failures in the 60-directory registry run. `.pop()` took
    /// the last block whatever it held, so one empty fence after the answer
    /// threw the answer away and handed serde an empty string, which
    /// reports "EOF while parsing a value at line 1 column 0".
    ///
    /// Last-not-first still holds, and the test above still pins it. What
    /// changes is that "last" now means the last block that is actually a
    /// forecast, rather than the last block of any kind.
    #[test]
    fn an_empty_block_after_the_answer_does_not_win() {
        let text = format!(
            "My forecast is\n\n```json\n{}\n```\n\n```\n```\n",
            r#"{"expected_value": 12.0, "interval_low": 10.0, "interval_high": 14.0, "confidence": 0.9}"#,
        );

        assert_eq!(parse_forecast(&text).unwrap().expected_value, 12.0);
    }

    /// Prose in a trailing fence is the same bug wearing different clothes.
    #[test]
    fn a_trailing_non_json_block_does_not_win() {
        let text = format!(
            "```json\n{}\n```\n\nCaveats:\n\n```\nI could not read two files.\n```\n",
            r#"{"expected_value": 5.0, "interval_low": 4.0, "interval_high": 6.0, "confidence": 0.8}"#,
        );

        assert_eq!(parse_forecast(&text).unwrap().expected_value, 5.0);
    }

    /// The 8 "no fenced json block" failures. A truncated answer opens the
    /// fence and never closes it, and the body was being dropped on the
    /// floor at end of input even though it parses.
    #[test]
    fn an_unclosed_final_fence_is_still_read() {
        let text = format!(
            "Here is my forecast.\n\n```json\n{}\n",
            r#"{"expected_value": 0.3, "interval_low": 0.2, "interval_high": 0.4, "confidence": 0.7}"#,
        );

        assert_eq!(parse_forecast(&text).unwrap().expected_value, 0.3);
    }

    /// Leniency has a floor. Nothing here may invent a forecast, so a block
    /// that is merely close is still refused rather than repaired.
    #[test]
    fn a_block_that_is_not_a_forecast_is_still_refused() {
        let text = "```json\n{\"expected_value\": 1.0}\n```\n";

        assert!(parse_forecast(text).is_err());
    }

    /// And an incoherent forecast does not get skipped in favour of an
    /// earlier well-formed one. Reading backwards must not become a way to
    /// shop for the answer that happens to pass.
    #[test]
    fn an_incoherent_last_forecast_is_reported_not_skipped() {
        let text = format!(
            "```json\n{}\n```\n\n```json\n{}\n```\n",
            r#"{"expected_value": 5.0, "interval_low": 4.0, "interval_high": 6.0, "confidence": 0.8}"#,
            r#"{"expected_value": 99.0, "interval_low": 4.0, "interval_high": 6.0, "confidence": 0.8}"#,
        );

        assert!(matches!(
            parse_forecast(&text).unwrap_err(),
            ForecastError::Incoherent(_)
        ));
    }

    #[test]
    fn an_answer_with_no_block_is_refused() {
        assert_eq!(
            parse_forecast("I expect about 0.8.").unwrap_err(),
            ForecastError::NoFencedBlock
        );
    }

    #[test]
    fn a_non_finite_number_is_refused() {
        // JSON has no NaN literal, so the reachable case is a string or
        // a missing field rather than a literal NaN.
        for body in [
            r#"{"expected_value": "0.8", "interval_low": 0.7, "interval_high": 0.9, "confidence": 0.8}"#,
            r#"{"interval_low": 0.7, "interval_high": 0.9, "confidence": 0.8}"#,
        ] {
            assert!(matches!(
                parse_forecast(&block(body)).unwrap_err(),
                ForecastError::BadNumber(_)
            ));
        }
    }

    /// An interval that does not contain its own expected value is not a
    /// forecast, it is two unrelated numbers.
    #[test]
    fn an_expected_value_outside_its_interval_is_refused() {
        let text = block(
            r#"{"expected_value": 0.99, "interval_low": 0.7, "interval_high": 0.9, "confidence": 0.8}"#,
        );

        assert!(matches!(
            parse_forecast(&text).unwrap_err(),
            ForecastError::Incoherent(_)
        ));
    }

    #[test]
    fn an_inverted_interval_is_refused() {
        let text = block(
            r#"{"expected_value": 0.8, "interval_low": 0.9, "interval_high": 0.7, "confidence": 0.8}"#,
        );

        assert!(matches!(
            parse_forecast(&text).unwrap_err(),
            ForecastError::Incoherent(_)
        ));
    }

    /// Certainty is not a probability this can score. A confidence of 1
    /// with a finite interval is a claim of impossibility, and the
    /// calibration report has no band for it.
    #[test]
    fn a_confidence_outside_zero_to_one_is_refused() {
        for c in ["0", "1", "1.5", "-0.2"] {
            let text = block(&format!(
                r#"{{"expected_value": 0.8, "interval_low": 0.7, "interval_high": 0.9, "confidence": {c}}}"#
            ));
            assert!(
                matches!(
                    parse_forecast(&text).unwrap_err(),
                    ForecastError::Incoherent(_)
                ),
                "confidence {c} should be refused"
            );
        }
    }

    /// A zero-width interval is a claim of certainty, and `calibration`
    /// refuses to produce a sigma for one. Recording it would put a row
    /// in the ledger that no surprise figure can ever be computed for.
    #[test]
    fn a_zero_width_interval_parses_but_the_store_is_what_refuses_it() {
        let text = block(
            r#"{"expected_value": 0.8, "interval_low": 0.8, "interval_high": 0.8, "confidence": 0.8}"#,
        );

        // Coherent by this module's rules: the value is inside its
        // interval. The store owns the zero-width refusal, and this
        // test exists to record that the split is deliberate rather
        // than an oversight.
        assert!(parse_forecast(&text).is_ok());
    }

    /// The prompt must not carry anything the working agent found, or
    /// the forecast is conditioned on the result it is predicting.
    #[test]
    fn the_prompt_carries_the_hypothesis_and_the_metric_and_no_findings() {
        let p = forecast_prompt("caching cuts p99 latency", "p99_ms");

        assert!(p.contains("caching cuts p99 latency"));
        assert!(p.contains("p99_ms"));
        assert!(p.contains("before it is measured"));
    }
}
