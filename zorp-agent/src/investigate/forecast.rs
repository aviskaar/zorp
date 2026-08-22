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
            ForecastError::NoFencedBlock => write!(f, "no fenced json block in the forecast"),
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

/// The confidences a forecaster is asked to choose between.
///
/// Calibration is measured per stated confidence, and a band is only
/// worth judging once it holds enough forecasts to have expected a
/// miss. `zorp_track::calibration::required_band_n` puts that at the
/// larger of a flat floor and `1 / (1 - confidence)`, so a forecaster
/// free to write any number spreads a run across bands that can never
/// individually be judged. A real run did exactly that: thirty scored
/// forecasts landed on six confidences between 0.93 and 0.99, and the
/// 0.99 band alone would have needed a hundred rows.
///
/// So the question is asked on a grid. This does not make a forecaster
/// better calibrated and it is not meant to. It makes the answer
/// measurable: every level here needs only the flat floor, which a run
/// of ordinary size can reach. Asking instead for a free number is
/// asking to tell 0.97 from 0.98 with evidence that cannot tell them
/// apart.
///
/// Nothing enforces the grid. `parse_forecast` accepts any confidence
/// in (0, 1), because filtering out an off-grid answer would discard a
/// real forecast and bias the sample toward the runs where the model
/// happened to comply.
pub const CONFIDENCE_GRID: &[f64] = &[0.5, 0.7, 0.8, 0.9, 0.95];

/// The prompt a forecaster sees.
///
/// It carries the hypothesis and the metric name and nothing about how
/// the work will be done, because a forecast conditioned on the working
/// agent's findings is a postdiction wearing the right timestamp.
pub fn forecast_prompt(hypothesis: &str, metric_name: &str) -> String {
    let levels = CONFIDENCE_GRID
        .iter()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(", ");
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
         landing inside your interval. Choose one of: {levels}. \
         Pick the one that honestly matches how sure you are, and widen \
         or narrow the interval to fit it rather than reaching for a \
         number in between. Do not write anything after the block."
    )
}

/// The last fenced block in `text`, parsed as a forecast.
///
/// Last, not first, for the same reason `panel` reads verdicts
/// backwards: a model that quotes the requested shape while explaining
/// itself would otherwise have its example parsed as its answer.
pub fn parse_forecast(text: &str) -> Result<Forecast, ForecastError> {
    let blocks = fenced_blocks(text);
    let last = blocks.last().ok_or(ForecastError::NoFencedBlock)?;

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
    let block = blocks
        .iter()
        .rev()
        .find(|b| is_forecast_shaped(b))
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

    /// The prompt has to name the grid, or a forecaster has no way to
    /// know the answer is constrained.
    #[test]
    fn the_prompt_offers_a_fixed_set_of_confidences() {
        let p = forecast_prompt("caching cuts p99 latency", "p99_ms");

        for level in CONFIDENCE_GRID {
            assert!(p.contains(&level.to_string()), "prompt omits {level}");
        }
    }

    /// Every level on the grid has to be reachable, or the grid is
    /// asking for evidence a run can never gather. `required_band_n`
    /// grows as `1 / (1 - confidence)`, so a level too close to 1
    /// silently makes a go impossible.
    #[test]
    fn every_level_on_the_grid_is_judgeable_at_a_reachable_sample_size() {
        for level in CONFIDENCE_GRID {
            let needed = zorp_track::calibration::required_band_n(*level);
            assert!(
                needed <= zorp_track::calibration::MIN_BAND_N,
                "a band at {level} needs {needed} rows, more than the flat floor"
            );
        }
    }

    /// A forecast off the grid is still read. The grid is what we ask
    /// for, not a filter on what we accept: silently dropping an
    /// off-grid answer would discard a real forecast and bias the
    /// sample toward whatever the model does when it complies.
    #[test]
    fn a_confidence_off_the_grid_is_still_parsed() {
        let text = block(
            r#"{"expected_value": 5.0, "interval_low": 4.0, "interval_high": 6.0, "confidence": 0.97}"#,
        );

        assert_eq!(parse_forecast(&text).unwrap().confidence, 0.97);
    }
}
