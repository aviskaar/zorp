//! aryabhatta step 6a: the re-run gate.
//!
//! Surprise alone does not admit an anomaly. The experiment is repeated
//! under the same recorded conditions and the repeats are classified,
//! and only two of the four classifications get through.
//!
//! The gate does two jobs with one mechanism.
//!
//! It separates defects and phenomena from noise. In a software
//! environment most prediction error is a truncated context, a changed
//! default, or a mis-parsed result file rather than a discovery.
//!
//! It also separates reducible uncertainty from irreducible. Reward
//! prediction error directly and a system reliably finds whatever is
//! inherently random, which is the noisy TV problem from
//! intrinsic-motivation reinforcement learning. zorp's noisy TVs are
//! sampling above temperature zero, flaky tests, network latency, and
//! search results that differ between calls. A flaky test generates a
//! clean four-sigma anomaly on demand, forever, and `Volatile` is the
//! classification that throws those away.
//!
//! Classification is arithmetic and equality, so no model takes part.
//! The model's turn comes after admission, writing an explanation into
//! a column integrity rule 5 stops every detector from reading back.

use crate::calibration::surprise;
use crate::conditions::Condition;
use crate::experiment::MetricValue;
use crate::track::Store;
use crate::TrackError;
use std::collections::BTreeMap;

/// Which side of the forecast interval a value fell on.
///
/// `Inside` is not a side. Naming it here rather than treating outside
/// as a bool is what lets `Reproduced` mean "outside, and the same way
/// as last time" instead of merely "outside".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Below,
    Inside,
    Above,
}

fn side_of(value: f64, interval_low: f64, interval_high: f64) -> Side {
    if value < interval_low {
        Side::Below
    } else if value > interval_high {
        Side::Above
    } else {
        Side::Inside
    }
}

/// How the repeats classified against the forecast the original run
/// surprised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateOutcome {
    /// Every repeat fell outside the interval on the same side as the
    /// original, and the repeats agree with each other closely enough.
    /// Admitted.
    Reproduced,
    /// The repeats fell inside the interval. The forecast was right
    /// after all and the original reading was a blip. Rejected,
    /// counted.
    Transient,
    /// The repeats disagree: outside on the opposite side from the
    /// original, or spread wider than the interval itself, or present
    /// in some repeats and absent in others. Rejected, counted.
    ///
    /// This is the classification that throws away flaky tests.
    /// Dropping it would turn the ledger into a list of the
    /// environment's noise sources.
    Volatile,
    /// The conditions could not be replayed, so the gate could not be
    /// applied. Admitted and flagged, because failing to look is not
    /// evidence that there was nothing to see.
    Unverifiable,
}

impl GateOutcome {
    /// The string stored in `gate_runs.outcome` and
    /// `anomalies.gate_outcome`.
    pub fn as_str(self) -> &'static str {
        match self {
            GateOutcome::Reproduced => "reproduced",
            GateOutcome::Transient => "transient",
            GateOutcome::Volatile => "volatile",
            GateOutcome::Unverifiable => "unverifiable",
        }
    }

    /// Whether this outcome puts a row in the ledger.
    ///
    /// Two of the four. `Transient` and `Volatile` are the noise
    /// classifications, counted rather than admitted.
    pub fn admits(self) -> bool {
        matches!(self, GateOutcome::Reproduced | GateOutcome::Unverifiable)
    }

    /// Parse back what [`GateOutcome::as_str`] wrote.
    ///
    /// Four named arms and an error, with no catch-all. A row holding
    /// an outcome this crate never wrote is corruption, and decoding it
    /// to some default would hide that: the 2026-08-15 `TrackStatus`
    /// defect, exactly.
    pub fn parse(raw: &str) -> Result<Self, TrackError> {
        match raw {
            "reproduced" => Ok(GateOutcome::Reproduced),
            "transient" => Ok(GateOutcome::Transient),
            "volatile" => Ok(GateOutcome::Volatile),
            "unverifiable" => Ok(GateOutcome::Unverifiable),
            other => Err(TrackError::Malformed {
                what: "gate outcome",
                detail: format!("unknown outcome '{other}'"),
            }),
        }
    }
}

/// An outcome that surprised its own forecast, and the interval it
/// surprised.
///
/// Returned by [`Store::gate_candidate`] and carried so a caller can say
/// what it is about to spend repeats on without asking the database for
/// numbers it has already been handed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GateCandidate {
    pub observed_value: f64,
    pub interval_low: f64,
    pub interval_high: f64,
}

/// One repeat of the original experiment, and what it measured.
#[derive(Debug, Clone, PartialEq)]
pub struct Repeat {
    pub experiment_id: String,
    /// The metric this repeat produced. `None` when the repeat recorded
    /// no numeric outcome for the metric at all, which is a failed
    /// replay rather than a reading of zero.
    pub observed_value: Option<f64>,
}

/// Everything the gate looked at, and what it concluded.
///
/// Carried whole rather than reduced to the outcome. The ledger row and
/// the noise count are both derived from it, and neither should have to
/// ask the database again for numbers the gate already had in hand.
#[derive(Debug, Clone, PartialEq)]
pub struct GateVerdict {
    pub outcome: GateOutcome,
    pub track_id: String,
    pub experiment_id: String,
    pub metric_key: String,
    pub expectation_id: String,
    pub expected_value: f64,
    pub interval_low: f64,
    pub interval_high: f64,
    /// The original outcome, the one that was surprising.
    pub observed_value: f64,
    /// How far the original landed from its forecast, in units of the
    /// sigma that forecast implied.
    pub surprise_sigma: f64,
    pub repeats: Vec<Repeat>,
    /// Why the replay failed, when it did. Empty for every other
    /// outcome. Code-authored: condition keys and values, never prose.
    pub divergences: Vec<String>,
}

/// Classify a set of repeats against the interval the original
/// surprised.
///
/// `original` must already sit outside `[interval_low, interval_high]`.
/// A value inside its own forecast interval is not an anomaly and there
/// is nothing to gate.
///
/// The spec states four rules. They collapse to three branches, and
/// they are written collapsed here rather than transcribed, because a
/// branch that can never change the answer implies a distinction that
/// does not exist:
///
/// 1. Every repeat inside the interval is `Transient`. The forecast was
///    right after all and the original reading was a blip.
/// 2. Every repeat outside on the original's side, with the repeats
///    agreeing to within the interval's own width, is `Reproduced`.
/// 3. Everything else is `Volatile`.
///
/// The spec's "outside on opposite sides" is inside branch 3 rather
/// than ahead of it. A repeat on the opposite side cannot satisfy
/// branch 1 or branch 2, so checking it first was tried and could not
/// be made to change any answer: the mutation that deletes the check
/// leaves every test green, which is what a redundant branch looks
/// like. It is one case of "the repeats do not agree", and that is
/// what branch 3 means.
///
/// Branch 3 also absorbs mixed repeats, outside in some and inside in
/// others, and that is `Volatile` rather than `Transient` on purpose.
/// Transient means the anomaly went away. Mixed means it comes and
/// goes, which is the signature of the flaky test this gate exists to
/// reject.
///
/// The spread in branch 2 is measured over the repeats alone, not the
/// repeats plus the original. One repeat therefore has zero spread and
/// can never fail branch 2 on that count. That is a real limit, and the
/// honest fix is more repeats rather than folding the original into a
/// statistic about reproducibility.
fn classify(
    interval_low: f64,
    interval_high: f64,
    original: f64,
    repeats: &[f64],
) -> Result<GateOutcome, TrackError> {
    if repeats.is_empty() {
        return Err(TrackError::Malformed {
            what: "re-run gate",
            detail: "the gate needs at least one repeat; an anomaly cannot gate itself".into(),
        });
    }
    let original_side = side_of(original, interval_low, interval_high);
    if original_side == Side::Inside {
        return Err(TrackError::Malformed {
            what: "re-run gate",
            detail: format!(
                "observed value {original} is inside its own interval \
                 [{interval_low}, {interval_high}]; that is not an anomaly"
            ),
        });
    }

    let sides: Vec<Side> = repeats
        .iter()
        .map(|&r| side_of(r, interval_low, interval_high))
        .collect();

    if sides.iter().all(|&s| s == Side::Inside) {
        return Ok(GateOutcome::Transient);
    }

    // Finiteness comes from the write side, not from anything here.
    // The original value is checked by `record_expectation`, and the
    // repeats are read back out of `metrics`, which `record_metric`
    // refuses to put a non-finite number into. Naming the writers
    // matters: this fold is `f64::min`/`f64::max`, which swallow a NaN
    // silently, and `side_of` calls a NaN `Inside`. Together those
    // would report an all-NaN rerun as `Transient`, which is the safe
    // answer arrived at for the wrong reason. The guarantee has to hold
    // upstream because it is not recoverable here.
    let lowest = repeats.iter().copied().fold(f64::INFINITY, f64::min);
    let highest = repeats.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let agree = highest - lowest <= interval_high - interval_low;

    if agree && sides.iter().all(|&s| s == original_side) {
        return Ok(GateOutcome::Reproduced);
    }
    Ok(GateOutcome::Volatile)
}

/// One experiment's conditions, as a sorted list of key and rendered
/// value.
///
/// Repeated keys are an append-only history, so the last recorded value
/// for each key is the one that was in force. Rendering to a string is
/// enough because only equality is ever asked of it, and it keeps the
/// divergence message readable without a second lookup.
fn condition_fingerprint(conditions: &[Condition]) -> Vec<(String, String)> {
    let mut latest: BTreeMap<String, String> = BTreeMap::new();
    for condition in conditions {
        latest.insert(
            condition.condition_key.clone(),
            render_value(&condition.value),
        );
    }
    latest.into_iter().collect()
}

/// The type tag is part of the rendering, so the string `"1"` and the
/// number 1 do not compare equal. Two different recorded types are a
/// changed condition even when they print the same.
fn render_value(value: &MetricValue) -> String {
    match value {
        MetricValue::Number(n) => format!("number:{n}"),
        MetricValue::Text(s) => format!("string:{s}"),
        MetricValue::Bool(b) => format!("bool:{b}"),
    }
}

/// Every key where the two fingerprints disagree, including keys
/// present in one and missing from the other.
fn diverging_keys(original: &[(String, String)], repeat: &[(String, String)]) -> Vec<String> {
    let a: BTreeMap<&str, &str> = original
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let b: BTreeMap<&str, &str> = repeat
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let mut out = Vec::new();
    for (key, value) in &a {
        match b.get(key) {
            Some(other) if other == value => {}
            Some(other) => out.push(format!("{key}: {value} became {other}")),
            None => out.push(format!("{key}: {value} was not recorded on the repeat")),
        }
    }
    for (key, value) in &b {
        if !a.contains_key(key) {
            out.push(format!("{key}: {value} is new on the repeat"));
        }
    }
    out
}

impl Store {
    /// Run the re-run gate for one metric of one experiment.
    ///
    /// The forecast scored is the last one recorded for the metric and
    /// the outcome scored is the first recorded, which is the rule the
    /// calibration report already uses. Two readers of one forecast
    /// disagreeing about which row counts would be worse than either
    /// rule being the wrong choice.
    ///
    /// Reads only. Nothing here writes. [`Store::record_gate_verdict`]
    /// is the thing that writes, and it takes what this returns.
    pub fn rerun_gate(
        &self,
        experiment_id: &str,
        metric_key: &str,
        repeat_experiment_ids: &[&str],
    ) -> Result<GateVerdict, TrackError> {
        let track_id = self.track_id_for_experiment(experiment_id)?;
        let expectation = self.last_expectation(experiment_id, metric_key)?;
        let observed_value = self
            .first_number_outcome(experiment_id, metric_key)?
            .ok_or_else(|| TrackError::Malformed {
                what: "re-run gate",
                detail: format!(
                    "experiment '{experiment_id}' recorded no numeric outcome for metric \
                     '{metric_key}'; there is nothing to gate"
                ),
            })?;

        // An anomaly the ledger cannot carry a surprise for must not be
        // admitted at all. `surprise_sigma` is NOT NULL there on
        // purpose, and a zero-width interval is exactly the case
        // `calibration::sigma` already refuses: the naive division
        // gives NaN on an exact hit, NaN loses every comparison, and a
        // forecaster who claimed certainty and was wrong would sort as
        // the least surprising row in the ledger.
        let surprise_sigma = surprise(
            observed_value,
            expectation.expected_value,
            expectation.interval_low,
            expectation.interval_high,
            expectation.confidence,
        )
        .map_err(|why| TrackError::Malformed {
            what: "re-run gate",
            detail: format!(
                "surprise is undefined for experiment '{experiment_id}' metric \
                 '{metric_key}': {why:?}"
            ),
        })?;

        let original_fingerprint = condition_fingerprint(&self.conditions_for(experiment_id)?);

        let mut repeats = Vec::new();
        let mut divergences = Vec::new();
        for repeat_id in repeat_experiment_ids {
            // Named explicitly rather than left to fail later on an
            // empty conditions read. A typo'd repeat id would otherwise
            // look like an experiment whose conditions all diverged,
            // which is a wrong answer dressed as a right one.
            self.track_id_for_experiment(repeat_id)?;
            let repeat_fingerprint = condition_fingerprint(&self.conditions_for(repeat_id)?);
            for detail in diverging_keys(&original_fingerprint, &repeat_fingerprint) {
                divergences.push(format!("{repeat_id}: {detail}"));
            }
            let value = self.first_number_outcome(repeat_id, metric_key)?;
            if value.is_none() {
                divergences.push(format!(
                    "{repeat_id}: no numeric outcome recorded for metric '{metric_key}'"
                ));
            }
            repeats.push(Repeat {
                experiment_id: (*repeat_id).to_string(),
                observed_value: value,
            });
        }

        let outcome = if divergences.is_empty() {
            let values: Vec<f64> = repeats.iter().filter_map(|r| r.observed_value).collect();
            classify(
                expectation.interval_low,
                expectation.interval_high,
                observed_value,
                &values,
            )?
        } else {
            // A replay that did not replay classifies nothing. The
            // repeats are still carried so a reader can see what came
            // back, but they are not evidence about a phenomenon
            // observed under conditions that no longer hold.
            GateOutcome::Unverifiable
        };

        Ok(GateVerdict {
            outcome,
            track_id,
            experiment_id: experiment_id.to_string(),
            metric_key: metric_key.to_string(),
            expectation_id: expectation.id,
            expected_value: expectation.expected_value,
            interval_low: expectation.interval_low,
            interval_high: expectation.interval_high,
            observed_value,
            surprise_sigma,
            repeats,
            divergences,
        })
    }

    /// Whether this experiment's outcome is worth gating at all.
    ///
    /// `None` in three cases, and all three mean the same thing to a
    /// caller: do not spend model calls on repeats. There is no forecast
    /// for the metric, so nothing was predicted and nothing can be
    /// surprising. There is no numeric outcome, so nothing was measured.
    /// Or the outcome fell inside its stated interval, which is a
    /// forecast that was right, and `classify` refuses one of those
    /// outright rather than classifying it.
    ///
    /// This exists so the decision to replay is made on the same forecast
    /// and the same outcome [`Store::rerun_gate`] will later use, read by
    /// the same two private readers. A caller picking the rows itself
    /// could admit a candidate the gate then refuses for disagreeing
    /// about which row counted, and it would have paid for the repeats
    /// before finding out.
    pub fn gate_candidate(
        &self,
        experiment_id: &str,
        metric_key: &str,
    ) -> Result<Option<GateCandidate>, TrackError> {
        let expectation = match self.last_expectation(experiment_id, metric_key) {
            Ok(e) => e,
            // No forecast is a normal state, not a failure: forecasting
            // is off by default and an unforecast attempt is the honest
            // shape of a record nobody asked to predict.
            Err(TrackError::NotFound { .. }) => return Ok(None),
            Err(e) => return Err(e),
        };
        let Some(observed_value) = self.first_number_outcome(experiment_id, metric_key)? else {
            return Ok(None);
        };
        if side_of(
            observed_value,
            expectation.interval_low,
            expectation.interval_high,
        ) == Side::Inside
        {
            return Ok(None);
        }
        Ok(Some(GateCandidate {
            observed_value,
            interval_low: expectation.interval_low,
            interval_high: expectation.interval_high,
        }))
    }

    pub(crate) fn track_id_for_experiment(
        &self,
        experiment_id: &str,
    ) -> Result<String, TrackError> {
        self.conn
            .query_row(
                "SELECT track_id FROM experiments WHERE id = ?",
                duckdb::params![experiment_id],
                |r| r.get(0),
            )
            .map_err(|_| TrackError::NotFound {
                kind: "experiment",
                id: experiment_id.to_string(),
            })
    }

    /// The last forecast recorded for the metric.
    ///
    /// `expectations` deliberately allows a forecast to be rewritten
    /// while no outcome exists, so there can be several. The last one
    /// is the belief actually held when the run happened.
    fn last_expectation(
        &self,
        experiment_id: &str,
        metric_key: &str,
    ) -> Result<crate::expectations::Expectation, TrackError> {
        self.expectations_for(experiment_id)?
            .into_iter()
            .rfind(|e| e.metric_key == metric_key)
            .ok_or_else(|| TrackError::NotFound {
                kind: "expectation",
                id: format!("{experiment_id}/{metric_key}"),
            })
    }

    /// The first numeric outcome recorded for the metric.
    ///
    /// First, not last, matching what the calibration report scores. A
    /// non-numeric metric stored under the same key is not an outcome
    /// this gate can read, and is skipped rather than coerced.
    fn first_number_outcome(
        &self,
        experiment_id: &str,
        metric_key: &str,
    ) -> Result<Option<f64>, TrackError> {
        let mut stmt = self.conn.prepare(
            "SELECT value_number FROM metrics \
             WHERE experiment_id = ? AND metric_key = ? AND value_type = 'number' \
             ORDER BY seq LIMIT 1",
        )?;
        let mut rows = stmt.query(duckdb::params![experiment_id, metric_key])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::experiment::MetricValue;
    use tempfile::tempdir;

    fn open() -> (tempfile::TempDir, Store) {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        (dir, store)
    }

    // ---- classify, the arithmetic on its own ----

    #[test]
    fn a_repeat_outside_on_the_same_side_reproduces() {
        assert_eq!(
            classify(0.70, 0.90, 0.50, &[0.52]).unwrap(),
            GateOutcome::Reproduced
        );
    }

    #[test]
    fn a_repeat_back_inside_the_interval_is_transient() {
        assert_eq!(
            classify(0.70, 0.90, 0.50, &[0.80]).unwrap(),
            GateOutcome::Transient
        );
    }

    /// The one-repeat case of "outside on opposite sides". Reading the
    /// spec's rule as being about the original's side is what makes it
    /// bite with a single repeat, and a single repeat is the common
    /// case.
    #[test]
    fn a_repeat_outside_on_the_opposite_side_is_volatile() {
        assert_eq!(
            classify(0.70, 0.90, 0.50, &[0.95]).unwrap(),
            GateOutcome::Volatile
        );
    }

    #[test]
    fn repeats_outside_on_opposite_sides_are_volatile() {
        assert_eq!(
            classify(0.70, 0.90, 0.50, &[0.50, 0.99]).unwrap(),
            GateOutcome::Volatile
        );
    }

    /// The flaky test. It fires in two runs of three, so it is not
    /// transient, it is noise that happens to be on the right side.
    #[test]
    fn an_anomaly_that_comes_and_goes_is_volatile_not_transient() {
        assert_eq!(
            classify(0.70, 0.90, 0.50, &[0.65, 0.75, 0.68]).unwrap(),
            GateOutcome::Volatile
        );
    }

    /// Interval width 0.20, repeat spread 0.25. Both repeats are below
    /// the interval and both agree with the original's side, so only
    /// the spread rule can catch this.
    #[test]
    fn repeats_spread_wider_than_the_interval_are_volatile() {
        assert_eq!(
            classify(0.70, 0.90, 0.50, &[0.40, 0.65]).unwrap(),
            GateOutcome::Volatile
        );
    }

    /// The boundary of the rule above: a spread exactly equal to the
    /// interval width is not wider than it, so it still reproduces.
    /// "Exceeds" is doing real work in the spec, and tightening the
    /// comparison here would silently reject honest repeats.
    ///
    /// Whole numbers, deliberately. Written with the decimals the rest
    /// of these tests use, the spread and the width differ in the last
    /// bit and the case is not the boundary at all: the mutation that
    /// tightens the comparison survives such a test, which is how this
    /// one came to be written this way.
    #[test]
    fn a_spread_exactly_equal_to_the_interval_width_still_reproduces() {
        let width = 20.0 - 10.0;
        let spread = 5.0 - (-5.0);
        assert_eq!(
            width, spread,
            "the test is only the boundary if these are equal"
        );
        assert_eq!(
            classify(10.0, 20.0, 5.0, &[-5.0, 5.0]).unwrap(),
            GateOutcome::Reproduced
        );
    }

    /// One bit wider is wider. The pair with the test above pins the
    /// comparison from both sides, so neither loosening nor tightening
    /// it goes unnoticed.
    #[test]
    fn a_spread_one_step_wider_than_the_interval_is_volatile() {
        assert_eq!(
            classify(10.0, 20.0, 5.0, &[-5.000000001, 5.0]).unwrap(),
            GateOutcome::Volatile
        );
    }

    #[test]
    fn the_gate_refuses_to_run_with_no_repeats() {
        let err = classify(0.70, 0.90, 0.50, &[]).expect_err("no repeats is not a classification");
        assert!(err.to_string().contains("at least one repeat"), "{err}");
    }

    #[test]
    fn a_value_inside_its_own_interval_is_not_an_anomaly() {
        let err = classify(0.70, 0.90, 0.80, &[0.80]).expect_err("0.80 is inside [0.70, 0.90]");
        assert!(err.to_string().contains("not an anomaly"), "{err}");
    }

    #[test]
    fn an_outcome_above_the_interval_reproduces_above_it() {
        assert_eq!(
            classify(0.70, 0.90, 0.99, &[0.98]).unwrap(),
            GateOutcome::Reproduced
        );
    }

    // ---- the gate against a real store ----

    /// One experiment with a forecast, an outcome, and a set of
    /// conditions, ready to be gated or replayed.
    fn seeded(
        store: &Store,
        track_id: &str,
        conditions: &[(&str, MetricValue)],
        outcome: Option<f64>,
        forecast: Option<(f64, f64, f64)>,
    ) -> String {
        let exp = store.create_experiment(track_id, "prereg").unwrap();
        for (key, value) in conditions {
            store.record_condition(&exp.id, key, value).unwrap();
        }
        if let Some((expected, low, high)) = forecast {
            store
                .record_expectation(&exp.id, "accuracy", expected, low, high, 0.80, &[])
                .unwrap();
        }
        if let Some(value) = outcome {
            store
                .record_metric(&exp.id, "accuracy", MetricValue::Number(value))
                .unwrap();
        }
        exp.id
    }

    fn track(store: &Store) -> &'static str {
        store.create_track("t1", "hyp").unwrap();
        "t1"
    }

    #[test]
    fn identical_conditions_and_a_repeated_deviation_reproduce() {
        let (_dir, store) = open();
        let t = track(&store);
        let conditions = [("model", MetricValue::Text("opus".into()))];
        let original = seeded(&store, t, &conditions, Some(0.50), Some((0.80, 0.70, 0.90)));
        let repeat = seeded(&store, t, &conditions, Some(0.51), None);

        let verdict = store
            .rerun_gate(&original, "accuracy", &[repeat.as_str()])
            .unwrap();
        assert_eq!(verdict.outcome, GateOutcome::Reproduced);
        assert!(verdict.divergences.is_empty());
        assert_eq!(verdict.observed_value, 0.50);
        assert_eq!(verdict.track_id, "t1");
        assert_eq!(verdict.repeats.len(), 1);
        assert_eq!(verdict.repeats[0].observed_value, Some(0.51));
    }

    /// The whole reason `unverifiable` exists. The deviation repeated
    /// perfectly, but under a different model, so it is not the same
    /// experiment run twice.
    #[test]
    fn a_changed_condition_makes_the_replay_unverifiable() {
        let (_dir, store) = open();
        let t = track(&store);
        let original = seeded(
            &store,
            t,
            &[("model", MetricValue::Text("opus".into()))],
            Some(0.50),
            Some((0.80, 0.70, 0.90)),
        );
        let repeat = seeded(
            &store,
            t,
            &[("model", MetricValue::Text("haiku".into()))],
            Some(0.50),
            None,
        );

        let verdict = store
            .rerun_gate(&original, "accuracy", &[repeat.as_str()])
            .unwrap();
        assert_eq!(verdict.outcome, GateOutcome::Unverifiable);
        assert_eq!(verdict.divergences.len(), 1);
        assert!(verdict.divergences[0].contains("model"), "{:?}", verdict);
        assert!(verdict.divergences[0].contains("opus"), "{:?}", verdict);
        assert!(verdict.divergences[0].contains("haiku"), "{:?}", verdict);
    }

    /// A condition the original recorded and the repeat did not is a
    /// divergence too. Comparing only shared keys would let a replay
    /// silently drop half the setup and still be called identical.
    #[test]
    fn a_condition_missing_from_the_repeat_is_a_divergence() {
        let (_dir, store) = open();
        let t = track(&store);
        let original = seeded(
            &store,
            t,
            &[
                ("model", MetricValue::Text("opus".into())),
                ("context_tokens", MetricValue::Number(8192.0)),
            ],
            Some(0.50),
            Some((0.80, 0.70, 0.90)),
        );
        let repeat = seeded(
            &store,
            t,
            &[("model", MetricValue::Text("opus".into()))],
            Some(0.50),
            None,
        );

        let verdict = store
            .rerun_gate(&original, "accuracy", &[repeat.as_str()])
            .unwrap();
        assert_eq!(verdict.outcome, GateOutcome::Unverifiable);
        assert!(
            verdict
                .divergences
                .iter()
                .any(|d| d.contains("context_tokens")),
            "{:?}",
            verdict.divergences
        );
    }

    #[test]
    fn a_condition_added_by_the_repeat_is_a_divergence() {
        let (_dir, store) = open();
        let t = track(&store);
        let original = seeded(
            &store,
            t,
            &[("model", MetricValue::Text("opus".into()))],
            Some(0.50),
            Some((0.80, 0.70, 0.90)),
        );
        let repeat = seeded(
            &store,
            t,
            &[
                ("model", MetricValue::Text("opus".into())),
                ("retries", MetricValue::Number(3.0)),
            ],
            Some(0.50),
            None,
        );

        let verdict = store
            .rerun_gate(&original, "accuracy", &[repeat.as_str()])
            .unwrap();
        assert_eq!(verdict.outcome, GateOutcome::Unverifiable);
        assert!(
            verdict.divergences.iter().any(|d| d.contains("retries")),
            "{:?}",
            verdict.divergences
        );
    }

    /// The number 1 and the string "1" are different conditions. A
    /// fingerprint that dropped the type tag would call this a clean
    /// replay.
    #[test]
    fn a_condition_that_changed_type_is_a_divergence() {
        let (_dir, store) = open();
        let t = track(&store);
        let original = seeded(
            &store,
            t,
            &[("retries", MetricValue::Number(1.0))],
            Some(0.50),
            Some((0.80, 0.70, 0.90)),
        );
        let repeat = seeded(
            &store,
            t,
            &[("retries", MetricValue::Text("1".into()))],
            Some(0.50),
            None,
        );

        let verdict = store
            .rerun_gate(&original, "accuracy", &[repeat.as_str()])
            .unwrap();
        assert_eq!(verdict.outcome, GateOutcome::Unverifiable);
    }

    /// Conditions are append-only, so a key recorded twice has a
    /// history. The value in force is the last one, and comparing
    /// against the first would call a genuinely matching replay a
    /// divergence.
    #[test]
    fn only_the_last_recorded_value_of_a_condition_counts() {
        let (_dir, store) = open();
        let t = track(&store);
        let original = store.create_experiment(t, "prereg").unwrap();
        store
            .record_condition(&original.id, "model", &MetricValue::Text("haiku".into()))
            .unwrap();
        store
            .record_condition(&original.id, "model", &MetricValue::Text("opus".into()))
            .unwrap();
        store
            .record_expectation(&original.id, "accuracy", 0.80, 0.70, 0.90, 0.80, &[])
            .unwrap();
        store
            .record_metric(&original.id, "accuracy", MetricValue::Number(0.50))
            .unwrap();

        let repeat = seeded(
            &store,
            t,
            &[("model", MetricValue::Text("opus".into()))],
            Some(0.51),
            None,
        );

        let verdict = store
            .rerun_gate(&original.id, "accuracy", &[repeat.as_str()])
            .unwrap();
        assert_eq!(verdict.outcome, GateOutcome::Reproduced, "{:?}", verdict);
    }

    #[test]
    fn a_repeat_that_recorded_no_outcome_is_unverifiable() {
        let (_dir, store) = open();
        let t = track(&store);
        let conditions = [("model", MetricValue::Text("opus".into()))];
        let original = seeded(&store, t, &conditions, Some(0.50), Some((0.80, 0.70, 0.90)));
        let repeat = seeded(&store, t, &conditions, None, None);

        let verdict = store
            .rerun_gate(&original, "accuracy", &[repeat.as_str()])
            .unwrap();
        assert_eq!(verdict.outcome, GateOutcome::Unverifiable);
        assert!(
            verdict
                .divergences
                .iter()
                .any(|d| d.contains("no numeric outcome")),
            "{:?}",
            verdict.divergences
        );
    }

    /// The gate and the calibration report must agree about which
    /// forecast counts, or the same anomaly gets two different sigmas
    /// depending on who asked.
    #[test]
    fn the_gate_scores_the_last_forecast_recorded() {
        let (_dir, store) = open();
        let t = track(&store);
        let exp = store.create_experiment(t, "prereg").unwrap();
        // A wide first draft, then the forecast actually held.
        store
            .record_expectation(&exp.id, "accuracy", 0.50, 0.00, 1.00, 0.80, &[])
            .unwrap();
        store
            .record_expectation(&exp.id, "accuracy", 0.80, 0.70, 0.90, 0.80, &[])
            .unwrap();
        store
            .record_metric(&exp.id, "accuracy", MetricValue::Number(0.50))
            .unwrap();
        let repeat = seeded(&store, t, &[], Some(0.51), None);

        let verdict = store
            .rerun_gate(&exp.id, "accuracy", &[repeat.as_str()])
            .unwrap();
        assert_eq!(verdict.interval_low, 0.70);
        assert_eq!(verdict.interval_high, 0.90);
        // Under the wide first draft 0.50 is inside the interval and
        // the gate would have refused to run at all.
        assert_eq!(verdict.outcome, GateOutcome::Reproduced);
    }

    #[test]
    fn a_zero_width_interval_is_refused_rather_than_scored() {
        let (_dir, store) = open();
        let t = track(&store);
        let exp = store.create_experiment(t, "prereg").unwrap();
        store
            .record_expectation(&exp.id, "accuracy", 0.80, 0.80, 0.80, 0.80, &[])
            .unwrap();
        store
            .record_metric(&exp.id, "accuracy", MetricValue::Number(0.50))
            .unwrap();
        let repeat = seeded(&store, t, &[], Some(0.51), None);

        let err = store
            .rerun_gate(&exp.id, "accuracy", &[repeat.as_str()])
            .expect_err("a zero-width interval has no sigma, so it has no surprise");
        assert!(err.to_string().contains("surprise is undefined"), "{err}");
    }

    #[test]
    fn an_experiment_with_no_forecast_cannot_be_gated() {
        let (_dir, store) = open();
        let t = track(&store);
        let exp = seeded(&store, t, &[], Some(0.50), None);
        let repeat = seeded(&store, t, &[], Some(0.51), None);

        let err = store
            .rerun_gate(&exp, "accuracy", &[repeat.as_str()])
            .expect_err("no forecast means nothing was predicted");
        assert!(err.to_string().contains("expectation not found"), "{err}");
    }

    #[test]
    fn an_unknown_repeat_id_is_named_rather_than_read_as_a_divergence() {
        let (_dir, store) = open();
        let t = track(&store);
        let exp = seeded(&store, t, &[], Some(0.50), Some((0.80, 0.70, 0.90)));

        let err = store
            .rerun_gate(&exp, "accuracy", &["not-an-experiment"])
            .expect_err("a typo'd repeat id is a caller error, not a divergence");
        assert!(err.to_string().contains("not-an-experiment"), "{err}");
    }

    #[test]
    fn the_gate_writes_nothing() {
        let (_dir, store) = open();
        let t = track(&store);
        let conditions = [("model", MetricValue::Text("opus".into()))];
        let original = seeded(&store, t, &conditions, Some(0.50), Some((0.80, 0.70, 0.90)));
        let repeat = seeded(&store, t, &conditions, Some(0.51), None);

        let before = crate::test_support::table_counts(&store);
        store
            .rerun_gate(&original, "accuracy", &[repeat.as_str()])
            .unwrap();
        let after = crate::test_support::table_counts(&store);
        assert_eq!(before, after);
    }

    #[test]
    fn every_outcome_round_trips_through_its_string() {
        for outcome in [
            GateOutcome::Reproduced,
            GateOutcome::Transient,
            GateOutcome::Volatile,
            GateOutcome::Unverifiable,
        ] {
            assert_eq!(GateOutcome::parse(outcome.as_str()).unwrap(), outcome);
        }
    }

    #[test]
    fn an_unknown_outcome_string_is_refused_rather_than_defaulted() {
        let err = GateOutcome::parse("mostly reproduced")
            .expect_err("an outcome this crate never wrote is corruption");
        assert!(err.to_string().contains("mostly reproduced"), "{err}");
    }

    #[test]
    fn only_reproduced_and_unverifiable_admit() {
        assert!(GateOutcome::Reproduced.admits());
        assert!(GateOutcome::Unverifiable.admits());
        assert!(!GateOutcome::Transient.admits());
        assert!(!GateOutcome::Volatile.admits());
    }
}
