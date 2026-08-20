//! aryabhatta step 2: per-experiment forecasts, and the rule that a
//! forecast cannot be written after its outcome exists.

use crate::track::Store;
use crate::TrackError;
use duckdb::OptionalExt;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A quantitative forecast about one metric of one experiment, written
/// before that experiment produces the metric.
///
/// This is not a pre-registration. A pre-registration is the scientific
/// commitment for a whole track, git-pinned and hash-checked, and there
/// is one. An expectation is a per-experiment forecast, and there will
/// be many.
#[derive(Debug, Clone, PartialEq)]
pub struct Expectation {
    pub id: String,
    pub experiment_id: String,
    pub metric_key: String,
    pub expected_value: f64,
    /// The stated central interval. `confidence` is its claimed
    /// coverage, so the pair is what later gives a sigma to divide by.
    pub interval_low: f64,
    pub interval_high: f64,
    /// Stated coverage of the interval, for example 0.80.
    pub confidence: f64,
    /// Free text, recorded and displayed, never read by a detector.
    pub assumptions: Vec<String>,
    pub recorded_at: i64,
    pub seq: i64,
}

/// Always writes a JSON array, `[]` included, so this writer never
/// leaves the column NULL. Serializing a list of strings has no failing
/// case, which is why the fallback can be a value rather than an error.
/// The same shape `validation.rs` uses for its citation columns.
fn assumptions_to_json(assumptions: &[String]) -> String {
    serde_json::to_string(assumptions).unwrap_or_else(|_| "[]".to_string())
}

/// Assumptions read back off a row.
///
/// A NULL is a legal "none recorded": the column is declared nullable
/// and this writer stores `[]` for that case, so the two agree. Text
/// that will not parse is different. It means something was recorded
/// and cannot be read, and reporting that as an empty list would say a
/// forecast was made with no assumptions when the record says
/// otherwise. Surfaces as `TrackError::Db` naming the offending text,
/// the same shape `track::unknown_status` uses for a status string the
/// code does not recognize.
fn assumptions_from_json(raw: Option<&str>, column: usize) -> duckdb::Result<Vec<String>> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    serde_json::from_str(raw).map_err(|e| {
        duckdb::Error::FromSqlConversionFailure(
            column,
            duckdb::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unreadable expectation assumptions {raw:?}: {e}"),
            )),
        )
    })
}

/// The guard behind integrity rule 1. `metrics` is where outcomes land,
/// so one row there for this experiment and metric key is enough to make
/// any forecast written now a postdiction.
///
/// Scoped to the metric key, not the whole experiment. An experiment
/// that has already reported latency can still take a first forecast
/// about accuracy, because nothing has been observed about accuracy yet.
fn assert_no_outcome_yet(
    store: &Store,
    experiment_id: &str,
    metric_key: &str,
) -> Result<(), TrackError> {
    let outcome_exists = store
        .conn
        .query_row(
            "SELECT 1 FROM metrics WHERE experiment_id = ? AND metric_key = ?",
            duckdb::params![experiment_id, metric_key],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if outcome_exists {
        return Err(TrackError::ExpectationAfterOutcome {
            experiment_id: experiment_id.to_string(),
            metric_key: metric_key.to_string(),
        });
    }
    Ok(())
}

/// Consistent with `record_metric`'s NotFound pattern rather than a
/// DuckDB FOREIGN KEY: fail loudly on a typo'd or stale experiment id
/// instead of quietly creating an orphan row.
///
/// Both of this module's helpers are free functions taking `&Store`
/// rather than private methods on it, and deliberately so. Every
/// inherent method on `Store` shares one namespace across the whole
/// crate no matter which module declares it, so two modules adding a
/// same-named private helper collide at compile time. That is not
/// hypothetical: `experiment.rs` already has its own version of this
/// check, and a second module added one under this exact name while
/// this file was being written. A free function is scoped to this file
/// and cannot collide with anything.
fn assert_experiment_recorded(store: &Store, experiment_id: &str) -> Result<(), TrackError> {
    let exists = store
        .conn
        .query_row(
            "SELECT 1 FROM experiments WHERE id = ?",
            duckdb::params![experiment_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(TrackError::NotFound {
            kind: "experiment",
            id: experiment_id.to_string(),
        });
    }
    Ok(())
}

/// A forecast whose own numbers do not hold together, refused before it
/// reaches the table.
///
/// `TrackError::Malformed` exists for exactly this: input the crate
/// cannot make sense of, refused before anything reaches DuckDB. `Io`
/// was the closest existing fit and it lied, printing "zorp-track io
/// error" for a forecast whose interval simply failed to bracket its own
/// expected value. `Db` would claim DuckDB complained when nothing
/// reached it, and `IntegrityMismatch` prints "prereg integrity mismatch
/// for track", naming a prereg that is not involved.
fn malformed(experiment_id: &str, metric_key: &str, detail: &str) -> TrackError {
    TrackError::Malformed {
        what: "expectation",
        detail: format!("experiment '{experiment_id}' metric '{metric_key}': {detail}"),
    }
}

impl Store {
    /// Record a forecast for `metric_key` on `experiment_id`.
    ///
    /// One argument over clippy's threshold, and left that way. The four
    /// numbers are one forecast and could be a struct, but every one of
    /// them is required and named, so bundling them would hide the shape
    /// rather than simplify it. Worth revisiting if a second caller ever
    /// wants to pass a forecast around before recording it.
    #[allow(clippy::too_many_arguments)]
    pub fn record_expectation(
        &self,
        experiment_id: &str,
        metric_key: &str,
        expected_value: f64,
        interval_low: f64,
        interval_high: f64,
        confidence: f64,
        assumptions: &[String],
    ) -> Result<Expectation, TrackError> {
        // Integrity rule 1, and the reason this module exists. An
        // expectation written once an outcome for the same metric
        // exists is a postdiction, and every number computed from it
        // downstream is theatre.
        //
        // This runs before anything else in the function on purpose.
        // The guarantee should not depend on the call first getting
        // past a shape check, so nothing above it can be rearranged
        // into a way around it.
        //
        // The guarantee is procedural, not cryptographic. It stops the
        // ordinary path. It does not stop someone editing the database
        // by hand, the way the prereg file hash does. That is an
        // accepted limit for v0: backdated expectations show up as
        // suspiciously perfect coverage in the calibration report,
        // which makes the cheat self-defeating.
        assert_no_outcome_yet(self, experiment_id, metric_key)?;
        assert_experiment_recorded(self, experiment_id)?;
        // Finiteness first, and stated explicitly rather than left to
        // fall out of the comparisons below. NaN loses every comparison
        // it takes part in, so an ordering check written the obvious way
        // waves it through, and a stored NaN turns the sigma, the
        // surprise and the coverage counts downstream to NaN as well.
        // An infinite interval end is a free coverage claim, and mean
        // interval width is exactly the statistic the calibration report
        // uses to catch that, so one infinite row would destroy the
        // defense against it.
        //
        // Named one at a time so the message says which number is the
        // problem. A single check over all four would refuse the row
        // without saying why.
        for (name, value) in [
            ("expected_value", expected_value),
            ("interval_low", interval_low),
            ("interval_high", interval_high),
            ("confidence", confidence),
        ] {
            if !value.is_finite() {
                return Err(malformed(
                    experiment_id,
                    metric_key,
                    &format!("{name} must be a finite number, got {value}"),
                ));
            }
        }
        // Safe to compare plainly now: finiteness is already
        // established above, so no NaN can slip past a `>`.
        //
        // An interval that does not bracket its own expected value is
        // not a forecast, it is two unrelated numbers, and the sigma
        // derived from it later would be meaningless.
        if interval_low > expected_value || expected_value > interval_high {
            return Err(malformed(
                experiment_id,
                metric_key,
                &format!(
                    "interval [{interval_low}, {interval_high}] does not contain expected value {expected_value}"
                ),
            ));
        }
        // Both ends are refused, not just values outside the range.
        // `confidence` is the stated coverage of the interval, and
        // surprise divides by a sigma derived from it. At 0 that sigma
        // is 0 and the division blows up; at 1 it is infinite and every
        // outcome scores zero surprise. Neither end is a coverage claim
        // a finite interval can earn anyway.
        if confidence <= 0.0 || confidence >= 1.0 {
            return Err(malformed(
                experiment_id,
                metric_key,
                &format!("confidence {confidence} must be strictly between 0 and 1"),
            ));
        }
        let recorded_at = now_millis();
        let seq_tag = crate::id::next_seq();
        // Zero-padded for the same reason experiment ids are: ids sort
        // as text, so seq 10 would otherwise sort before seq 9 within
        // one millisecond.
        let id = format!("{experiment_id}-expectation-{metric_key}-{recorded_at}-{seq_tag:06}");
        // seq is computed inside the INSERT, not by a prior
        // SELECT COUNT(*): a separate count is O(n^2) over an
        // experiment's expectations and can race to a duplicate seq.
        self.conn.execute(
            "INSERT INTO expectations \
             (id, experiment_id, metric_key, expected_value, interval_low, interval_high, confidence, assumptions, recorded_at, seq) \
             SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, COALESCE(MAX(seq), -1) + 1 FROM expectations WHERE experiment_id = ?",
            duckdb::params![
                id,
                experiment_id,
                metric_key,
                expected_value,
                interval_low,
                interval_high,
                confidence,
                assumptions_to_json(assumptions),
                recorded_at,
                experiment_id
            ],
        )?;
        // Read back the seq the database assigned rather than guessing
        // it in Rust. Recomputing it here would reintroduce exactly the
        // race the SQL above avoids, and the lookup is by primary key.
        let seq: i64 = self.conn.query_row(
            "SELECT seq FROM expectations WHERE id = ?",
            duckdb::params![id],
            |r| r.get(0),
        )?;
        Ok(Expectation {
            id,
            experiment_id: experiment_id.to_string(),
            metric_key: metric_key.to_string(),
            expected_value,
            interval_low,
            interval_high,
            confidence,
            assumptions: assumptions.to_vec(),
            recorded_at,
            seq,
        })
    }

    /// Every expectation recorded for `experiment_id`, in insertion
    /// order. Ordering is by `seq`, not `recorded_at`: two expectations
    /// written in the same millisecond would otherwise have no defined
    /// relative order, the same problem `metrics_for` solves the same
    /// way.
    pub fn expectations_for(&self, experiment_id: &str) -> Result<Vec<Expectation>, TrackError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, experiment_id, metric_key, expected_value, interval_low, interval_high, confidence, assumptions, recorded_at, seq \
             FROM expectations WHERE experiment_id = ? ORDER BY seq",
        )?;
        let rows = stmt.query_map(duckdb::params![experiment_id], |r| {
            let raw: Option<String> = r.get(7)?;
            Ok(Expectation {
                id: r.get(0)?,
                experiment_id: r.get(1)?,
                metric_key: r.get(2)?,
                expected_value: r.get(3)?,
                interval_low: r.get(4)?,
                interval_high: r.get(5)?,
                confidence: r.get(6)?,
                assumptions: assumptions_from_json(raw.as_deref(), 7)?,
                recorded_at: r.get(8)?,
                seq: r.get(9)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::experiment::MetricValue;
    use crate::track::Store;
    use tempfile::tempdir;

    #[test]
    fn record_expectation_round_trips_through_the_store() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();

        let recorded = store
            .record_expectation(
                &exp.id,
                "accuracy",
                0.80,
                0.70,
                0.90,
                0.80,
                &["harness held at 8k context".to_string()],
            )
            .unwrap();

        let found = store.expectations_for(&exp.id).unwrap();
        assert_eq!(found, vec![recorded]);
        assert_eq!(found[0].experiment_id, exp.id);
        assert_eq!(found[0].metric_key, "accuracy");
        assert_eq!(found[0].expected_value, 0.80);
        assert_eq!(found[0].interval_low, 0.70);
        assert_eq!(found[0].interval_high, 0.90);
        assert_eq!(found[0].confidence, 0.80);
        assert_eq!(
            found[0].assumptions,
            vec!["harness held at 8k context".to_string()]
        );
    }

    /// The anti-rationalization guarantee, and the reason this module
    /// exists. An expectation written after the outcome is a
    /// postdiction, and every number computed from it downstream is
    /// theatre.
    ///
    /// This test is mutation checked: delete the guard in
    /// `record_expectation` and this test must go red. A green test that
    /// survives the guard's deletion is worth nothing here.
    #[test]
    fn an_expectation_is_refused_once_that_metric_has_an_outcome() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();

        store
            .record_metric(&exp.id, "accuracy", MetricValue::Number(0.42))
            .unwrap();

        let err = store
            .record_expectation(&exp.id, "accuracy", 0.80, 0.70, 0.90, 0.80, &[])
            .expect_err("an expectation written after its outcome must be refused");
        match err {
            TrackError::ExpectationAfterOutcome {
                experiment_id,
                metric_key,
            } => {
                assert_eq!(experiment_id, exp.id);
                assert_eq!(metric_key, "accuracy");
            }
            other => panic!("wrong error: {other}"),
        }

        // A refusal that still wrote the row would be no refusal at all.
        assert!(store.expectations_for(&exp.id).unwrap().is_empty());
    }

    #[test]
    fn an_interval_that_does_not_contain_the_expected_value_is_refused() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();

        // Expected value below the interval, above it, and an interval
        // whose ends are the wrong way round.
        for (expected, low, high) in [(0.60, 0.70, 0.90), (0.95, 0.70, 0.90), (0.80, 0.90, 0.70)] {
            let err = store
                .record_expectation(&exp.id, "accuracy", expected, low, high, 0.80, &[])
                .expect_err("interval must contain the expected value");
            match err {
                TrackError::Malformed { detail: msg, .. } => assert!(
                    msg.contains("interval"),
                    "error should name the interval, got: {msg}"
                ),
                other => panic!("wrong error for ({expected}, {low}, {high}): {other}"),
            }
        }

        assert!(store.expectations_for(&exp.id).unwrap().is_empty());
    }

    /// Both ends are rejected, not just values outside the range.
    #[test]
    fn confidence_must_be_strictly_between_zero_and_one() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();

        for confidence in [0.0, 1.0, -0.1, 1.5] {
            let err = store
                .record_expectation(&exp.id, "accuracy", 0.80, 0.70, 0.90, confidence, &[])
                .expect_err("confidence must be strictly between 0 and 1");
            match err {
                TrackError::Malformed { detail: msg, .. } => assert!(
                    msg.contains("confidence"),
                    "error should name confidence, got: {msg}"
                ),
                other => panic!("wrong error for confidence {confidence}: {other}"),
            }
        }

        assert!(store.expectations_for(&exp.id).unwrap().is_empty());
    }

    /// NaN loses every comparison it takes part in, so a range check
    /// written the obvious way waves it straight through. Once stored it
    /// poisons everything downstream: the sigma, the surprise, and the
    /// coverage counts all go NaN, and because a NaN observation never
    /// falls inside an interval the row reads as an anomaly forever.
    ///
    /// Infinities are refused for a related reason. An infinitely wide
    /// interval is a free coverage claim, and the calibration report's
    /// defense against that is mean interval width, a statistic one
    /// infinite row destroys.
    #[test]
    fn a_forecast_carrying_a_non_finite_number_is_refused() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();

        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            // One position at a time, so no case can pass by accident
            // because some other field was already invalid.
            let cases = [
                (bad, 0.70, 0.90, 0.80),
                (0.80, bad, 0.90, 0.80),
                (0.80, 0.70, bad, 0.80),
                (0.80, 0.70, 0.90, bad),
            ];
            for (expected, low, high, confidence) in cases {
                let err = store
                    .record_expectation(&exp.id, "accuracy", expected, low, high, confidence, &[])
                    .expect_err("a non-finite number must be refused");
                match err {
                    TrackError::Malformed { detail: msg, .. } => assert!(
                        msg.contains("finite"),
                        "error should say what is wrong, got: {msg}"
                    ),
                    other => panic!("wrong error for {bad} at ({expected}, {low}, {high}, {confidence}): {other}"),
                }
            }
        }

        assert!(store.expectations_for(&exp.id).unwrap().is_empty());
    }

    /// A forecast about an experiment that does not exist is an orphan
    /// row nothing will ever score. Worse, the guard cannot protect it:
    /// a typo'd id has no metrics, so the postdiction check waves it
    /// through every time.
    #[test]
    fn record_expectation_on_a_missing_experiment_errors() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();

        let err = store
            .record_expectation("nope", "accuracy", 0.80, 0.70, 0.90, 0.80, &[])
            .unwrap_err();
        assert!(
            matches!(err, TrackError::NotFound { kind: "experiment", ref id } if id == "nope"),
            "wrong error: {err}"
        );
        assert!(store.expectations_for("nope").unwrap().is_empty());
    }

    /// Assumptions that cannot be read are a read failure, not an empty
    /// list. Coercing them would report a forecast as having been made
    /// with no assumptions when the record plainly says otherwise, and
    /// silently reading over a hand-edited row is the last thing this
    /// module should do.
    #[test]
    fn unreadable_assumptions_are_an_error_not_an_empty_list() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();
        store
            .record_expectation(
                &exp.id,
                "accuracy",
                0.80,
                0.70,
                0.90,
                0.80,
                &["ctx 8k".into()],
            )
            .unwrap();

        store
            .conn
            .execute(
                "UPDATE expectations SET assumptions = 'not json at all' WHERE experiment_id = ?",
                duckdb::params![exp.id],
            )
            .unwrap();

        let err = store
            .expectations_for(&exp.id)
            .expect_err("should refuse to read");
        let msg = err.to_string();
        assert!(
            msg.contains("not json at all"),
            "error should name the offending text, got: {msg}"
        );
    }

    /// A NULL is different from unreadable text. The column is declared
    /// nullable, so NULL is a legal "none recorded" and reads as one.
    #[test]
    fn null_assumptions_read_as_none_recorded() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();
        store
            .record_expectation(&exp.id, "accuracy", 0.80, 0.70, 0.90, 0.80, &[])
            .unwrap();

        store
            .conn
            .execute(
                "UPDATE expectations SET assumptions = NULL WHERE experiment_id = ?",
                duckdb::params![exp.id],
            )
            .unwrap();

        let found = store.expectations_for(&exp.id).unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].assumptions.is_empty());
    }

    /// The guard is about one metric, not the whole experiment. An
    /// experiment that has already reported latency has observed
    /// nothing about accuracy, so a first forecast about accuracy is
    /// still a prediction.
    ///
    /// This test is mutation checked too: drop `AND metric_key = ?`
    /// from the guard's query and it must go red. Without it the guard
    /// would over-refuse, and the first metric an experiment records
    /// would silently end its ability to forecast anything else.
    #[test]
    fn an_outcome_for_one_metric_does_not_block_a_forecast_about_another() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let exp = store.create_experiment("t1", "t1-prereg").unwrap();

        store
            .record_metric(&exp.id, "latency_ms", MetricValue::Number(120.0))
            .unwrap();

        store
            .record_expectation(&exp.id, "accuracy", 0.80, 0.70, 0.90, 0.80, &[])
            .expect("a first forecast about accuracy is still a prediction");

        let found = store.expectations_for(&exp.id).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].metric_key, "accuracy");
    }

    #[test]
    fn expectations_for_is_ordered_by_insertion_and_scoped_to_one_experiment() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        let a = store.create_experiment("t1", "t1-prereg").unwrap();
        let b = store.create_experiment("t1", "t1-prereg").unwrap();

        // Ids embed a millisecond timestamp and a tight loop lands many
        // inserts inside one millisecond, so `recorded_at` gives no
        // usable order here. `seq` is what actually holds the sequence.
        for i in 0..20 {
            store
                .record_expectation(&a.id, &format!("m{i}"), 0.80, 0.70, 0.90, 0.80, &[])
                .unwrap();
        }
        store
            .record_expectation(&b.id, "elsewhere", 0.10, 0.00, 0.20, 0.50, &[])
            .unwrap();

        let found = store.expectations_for(&a.id).unwrap();
        let keys: Vec<String> = found.iter().map(|e| e.metric_key.clone()).collect();
        let expected: Vec<String> = (0..20).map(|i| format!("m{i}")).collect();
        assert_eq!(keys, expected);
        let seqs: Vec<i64> = found.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, (0..20).collect::<Vec<i64>>());

        // seq restarts per experiment, so b's single row is seq 0 and
        // not 20.
        let other = store.expectations_for(&b.id).unwrap();
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].seq, 0);
        assert!(store.expectations_for("nope").unwrap().is_empty());
    }
}
