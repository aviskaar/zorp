//! aryabhatta step 6b: the anomaly ledger.
//!
//! Append-only. Rows are never deleted and a status change is an
//! explicit act, never a side effect of retrieval or of time passing.
//!
//! There is one way in. [`Store::record_gate_verdict`] takes what the
//! re-run gate returned, counts the run either way, and inserts a
//! ledger row only if the gate admitted it. No other function in this
//! crate writes to `anomalies`, so nothing reaches the ledger without
//! having been gated, and no caller has to remember to count a
//! rejection.
//!
//! That counting is the point of the `gate_runs` table. Rejected
//! anomalies are counted rather than discarded silently, which is what
//! makes the noisy TV rate measurable: a rejection that leaves no row
//! cannot be counted afterwards, and a system that cannot see its own
//! noise floor cannot tell a quiet environment from a blind one.
//!
//! `explanation` is model-authored text. It is stored and displayed and
//! never read by any detector, and the search layer may not build an
//! edge from it. Without that rule the agent's own speculation becomes
//! tomorrow's observation.

use crate::rerun::{GateOutcome, GateVerdict};
use crate::track::Store;
use crate::TrackError;
use chrono::Utc;

fn now_millis() -> i64 {
    Utc::now().timestamp_millis()
}

/// Where an admitted anomaly stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomalyStatus {
    /// Admitted and not accounted for. The state every row starts in.
    Unexplained,
    /// Someone wrote down what caused it. The row stays in the ledger.
    Explained,
    /// Later work made the row moot: the metric was redefined, the
    /// forecast was withdrawn, the deviation turned out to be the same
    /// one as another row. Not a deletion, and not a claim that the
    /// deviation did not happen.
    Superseded,
}

impl AnomalyStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            AnomalyStatus::Unexplained => "unexplained",
            AnomalyStatus::Explained => "explained",
            AnomalyStatus::Superseded => "superseded",
        }
    }

    /// Parse back what [`AnomalyStatus::as_str`] wrote.
    ///
    /// Three named arms and an error. A status this crate never wrote
    /// is corruption, and a catch-all arm would decode it to something
    /// plausible and hide that. This is the 2026-08-15 `TrackStatus`
    /// defect, and it is not repeated here.
    pub fn parse(raw: &str) -> Result<Self, TrackError> {
        match raw {
            "unexplained" => Ok(AnomalyStatus::Unexplained),
            "explained" => Ok(AnomalyStatus::Explained),
            "superseded" => Ok(AnomalyStatus::Superseded),
            other => Err(TrackError::Malformed {
                what: "anomaly status",
                detail: format!("unknown status '{other}'"),
            }),
        }
    }
}

/// One admitted anomaly.
#[derive(Debug, Clone, PartialEq)]
pub struct Anomaly {
    pub id: String,
    pub track_id: String,
    pub experiment_id: String,
    pub expectation_id: String,
    pub metric_key: String,
    pub expected_value: f64,
    pub interval_low: f64,
    pub interval_high: f64,
    pub observed_value: f64,
    pub surprise_sigma: f64,
    pub gate_outcome: GateOutcome,
    pub status: AnomalyStatus,
    /// Model-authored. Never a detector input, and never an edge in the
    /// search layer.
    pub explanation: Option<String>,
    pub created_at: i64,
    pub seq: i64,
}

/// How often the gate threw a candidate away, and for which reason.
///
/// The counts are the point. A verdict on its own says whether one
/// anomaly survived; these say whether the environment is quiet enough
/// for surviving to mean anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NoiseReport {
    pub reproduced: u64,
    pub transient: u64,
    pub volatile: u64,
    pub unverifiable: u64,
}

impl NoiseReport {
    pub fn total(&self) -> u64 {
        self.reproduced + self.transient + self.volatile + self.unverifiable
    }

    pub fn admitted(&self) -> u64 {
        self.reproduced + self.unverifiable
    }

    /// The share of gate runs thrown away as noise.
    ///
    /// `None` when the gate has never run, which is not a rate of zero.
    /// A system that has gated nothing has not shown that its
    /// environment is quiet; it has shown nothing at all, and reporting
    /// 0.0 for that would read as the best possible result.
    ///
    /// `unverifiable` is not counted as noise. A replay that could not
    /// be performed says nothing about how noisy the measurement is,
    /// and folding it in here would let a broken harness look like a
    /// clean one.
    pub fn noise_rate(&self) -> Option<f64> {
        let total = self.total();
        if total == 0 {
            return None;
        }
        Some((self.transient + self.volatile) as f64 / total as f64)
    }
}

/// Columns in `anomalies` that a reader is allowed to select.
///
/// Written out rather than `SELECT *` so integrity rule 5 is checkable
/// by looking at one list: `explanation` appears here because a human
/// reading the ledger needs it, and it is the only place in the crate
/// where it may appear.
const ANOMALY_COLUMNS: &str = "id, track_id, experiment_id, expectation_id, metric_key, \
     expected_value, interval_low, interval_high, observed_value, surprise_sigma, \
     gate_outcome, status, explanation, created_at, seq";

impl Store {
    /// Record what the gate decided.
    ///
    /// Always writes a `gate_runs` row. Writes an `anomalies` row, and
    /// returns it, only when the outcome admits. `Ok(None)` therefore
    /// means "gated and rejected", which is a result rather than a
    /// failure, and the rejection has been counted.
    ///
    /// Taking a whole [`GateVerdict`] rather than loose numbers is what
    /// makes the ledger ungameable from this side. There is no argument
    /// list a caller could assemble by hand; the only way to produce
    /// one of these is to have run the gate.
    pub fn record_gate_verdict(
        &self,
        verdict: &GateVerdict,
    ) -> Result<Option<Anomaly>, TrackError> {
        let created_at = now_millis();
        let seq_tag = crate::id::next_seq();

        let anomaly = if verdict.outcome.admits() {
            // Zero-padded for the reason every other id here is: ids
            // sort as text, so seq 10 would otherwise sort before seq 9
            // within one millisecond.
            let id = format!(
                "{}-anomaly-{}-{created_at}-{seq_tag:06}",
                verdict.experiment_id, verdict.metric_key
            );
            // seq is computed inside the INSERT rather than by a prior
            // SELECT COUNT(*): a separate count is O(n^2) over a
            // track's anomalies and can race to a duplicate seq.
            self.conn.execute(
                "INSERT INTO anomalies \
                 (id, track_id, experiment_id, expectation_id, metric_key, expected_value, \
                  interval_low, interval_high, observed_value, surprise_sigma, gate_outcome, \
                  status, explanation, created_at, seq) \
                 SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, \
                        COALESCE(MAX(seq), -1) + 1 FROM anomalies WHERE track_id = ?",
                duckdb::params![
                    id,
                    verdict.track_id,
                    verdict.experiment_id,
                    verdict.expectation_id,
                    verdict.metric_key,
                    verdict.expected_value,
                    verdict.interval_low,
                    verdict.interval_high,
                    verdict.observed_value,
                    verdict.surprise_sigma,
                    verdict.outcome.as_str(),
                    AnomalyStatus::Unexplained.as_str(),
                    created_at,
                    verdict.track_id
                ],
            )?;
            // Read the seq back rather than guessing it in Rust.
            // Recomputing it here would reintroduce the race the SQL
            // above avoids, and the lookup is by primary key.
            let seq: i64 = self.conn.query_row(
                "SELECT seq FROM anomalies WHERE id = ?",
                duckdb::params![id],
                |r| r.get(0),
            )?;
            Some(Anomaly {
                id,
                track_id: verdict.track_id.clone(),
                experiment_id: verdict.experiment_id.clone(),
                expectation_id: verdict.expectation_id.clone(),
                metric_key: verdict.metric_key.clone(),
                expected_value: verdict.expected_value,
                interval_low: verdict.interval_low,
                interval_high: verdict.interval_high,
                observed_value: verdict.observed_value,
                surprise_sigma: verdict.surprise_sigma,
                gate_outcome: verdict.outcome,
                status: AnomalyStatus::Unexplained,
                explanation: None,
                created_at,
                seq,
            })
        } else {
            None
        };

        let run_id = format!(
            "{}-gate-{}-{created_at}-{seq_tag:06}",
            verdict.experiment_id, verdict.metric_key
        );
        self.conn.execute(
            "INSERT INTO gate_runs \
             (id, experiment_id, metric_key, expectation_id, outcome, repeats, admitted, \
              anomaly_id, created_at, seq) \
             SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, COALESCE(MAX(seq), -1) + 1 FROM gate_runs",
            duckdb::params![
                run_id,
                verdict.experiment_id,
                verdict.metric_key,
                verdict.expectation_id,
                verdict.outcome.as_str(),
                verdict.repeats.len() as i64,
                verdict.outcome.admits(),
                anomaly.as_ref().map(|a| a.id.as_str()),
                created_at
            ],
        )?;

        Ok(anomaly)
    }

    /// Every anomaly recorded for `track_id`, oldest first.
    ///
    /// Ordered by `seq`, not `created_at`: two rows written in the same
    /// millisecond would otherwise have no defined relative order, the
    /// same problem `metrics_for` solves the same way.
    pub fn anomalies_for_track(&self, track_id: &str) -> Result<Vec<Anomaly>, TrackError> {
        let sql =
            format!("SELECT {ANOMALY_COLUMNS} FROM anomalies WHERE track_id = ? ORDER BY seq");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(duckdb::params![track_id], decode_anomaly)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row??);
        }
        Ok(out)
    }

    /// One anomaly by id.
    pub fn anomaly(&self, id: &str) -> Result<Anomaly, TrackError> {
        let sql = format!("SELECT {ANOMALY_COLUMNS} FROM anomalies WHERE id = ?");
        self.conn
            .query_row(&sql, duckdb::params![id], decode_anomaly)
            .map_err(|_| TrackError::NotFound {
                kind: "anomaly",
                id: id.to_string(),
            })?
    }

    /// Move an anomaly to a new status, with the reasoning behind it.
    ///
    /// An explicit act, which is the whole point. Nothing moves a row
    /// out of `unexplained` by reading it or by waiting.
    ///
    /// `explanation` is model-authored prose and is stored as given.
    /// Passing `None` leaves whatever was there, so recording a
    /// supersession does not erase the account of what happened.
    ///
    /// Marking a row `explained` without an explanation is refused. The
    /// status is a claim that the deviation has been accounted for, and
    /// a claim with nothing behind it is the failure mode the whole
    /// ledger exists to prevent.
    pub fn set_anomaly_status(
        &self,
        id: &str,
        status: AnomalyStatus,
        explanation: Option<&str>,
    ) -> Result<Anomaly, TrackError> {
        let current = self.anomaly(id)?;
        let text = explanation.map(str::trim).filter(|t| !t.is_empty());
        if status == AnomalyStatus::Explained && text.is_none() && current.explanation.is_none() {
            return Err(TrackError::Malformed {
                what: "anomaly status",
                detail: format!(
                    "anomaly '{id}' cannot be marked explained with no explanation recorded"
                ),
            });
        }
        match text {
            Some(text) => self.conn.execute(
                "UPDATE anomalies SET status = ?, explanation = ? WHERE id = ?",
                duckdb::params![status.as_str(), text, id],
            )?,
            None => self.conn.execute(
                "UPDATE anomalies SET status = ? WHERE id = ?",
                duckdb::params![status.as_str(), id],
            )?,
        };
        self.anomaly(id)
    }

    /// How the gate has classified everything it has been asked about.
    ///
    /// Counts `gate_runs`, not `anomalies`, which is the difference
    /// between "how much did we admit" and "how much did we look at".
    pub fn noise_report(&self) -> Result<NoiseReport, TrackError> {
        let mut stmt = self
            .conn
            .prepare("SELECT outcome, COUNT(*) FROM gate_runs GROUP BY outcome")?;
        let rows = stmt.query_map([], |r| {
            let outcome: String = r.get(0)?;
            let count: i64 = r.get(1)?;
            Ok((outcome, count))
        })?;
        let mut report = NoiseReport::default();
        for row in rows {
            let (outcome, count) = row?;
            let count = count.max(0) as u64;
            match GateOutcome::parse(&outcome)? {
                GateOutcome::Reproduced => report.reproduced = count,
                GateOutcome::Transient => report.transient = count,
                GateOutcome::Volatile => report.volatile = count,
                GateOutcome::Unverifiable => report.unverifiable = count,
            }
        }
        Ok(report)
    }
}

/// Decode one row of [`ANOMALY_COLUMNS`].
///
/// The outer `duckdb::Result` is the row read; the inner is the two
/// enum parses, which have their own error rather than being coerced
/// into a DuckDB one. Both are surfaced, so a corrupt status is a
/// refusal rather than a silently plausible value.
#[allow(clippy::type_complexity)]
fn decode_anomaly(r: &duckdb::Row<'_>) -> duckdb::Result<Result<Anomaly, TrackError>> {
    let gate_outcome: String = r.get(10)?;
    let status: String = r.get(11)?;
    let anomaly = Anomaly {
        id: r.get(0)?,
        track_id: r.get(1)?,
        experiment_id: r.get(2)?,
        expectation_id: r.get(3)?,
        metric_key: r.get(4)?,
        expected_value: r.get(5)?,
        interval_low: r.get(6)?,
        interval_high: r.get(7)?,
        observed_value: r.get(8)?,
        surprise_sigma: r.get(9)?,
        gate_outcome: GateOutcome::Reproduced,
        status: AnomalyStatus::Unexplained,
        explanation: r.get(12)?,
        created_at: r.get(13)?,
        seq: r.get(14)?,
    };
    Ok((|| {
        Ok(Anomaly {
            gate_outcome: GateOutcome::parse(&gate_outcome)?,
            status: AnomalyStatus::parse(&status)?,
            ..anomaly
        })
    })())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::experiment::MetricValue;
    use crate::test_support::table_counts;
    use tempfile::tempdir;

    fn open() -> (tempfile::TempDir, Store) {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("zorp.duckdb")).unwrap();
        store.create_track("t1", "hyp").unwrap();
        (dir, store)
    }

    /// An original that deviated, and a repeat under the same
    /// conditions landing on `repeat_value`.
    fn gated(store: &Store, repeat_value: Option<f64>) -> GateVerdict {
        let conditions = [("model", MetricValue::Text("opus".into()))];
        let original = store.create_experiment("t1", "prereg").unwrap();
        for (key, value) in &conditions {
            store.record_condition(&original.id, key, value).unwrap();
        }
        store
            .record_expectation(&original.id, "accuracy", 0.80, 0.70, 0.90, 0.80, &[])
            .unwrap();
        store
            .record_metric(&original.id, "accuracy", MetricValue::Number(0.50))
            .unwrap();

        let repeat = store.create_experiment("t1", "prereg").unwrap();
        for (key, value) in &conditions {
            store.record_condition(&repeat.id, key, value).unwrap();
        }
        if let Some(value) = repeat_value {
            store
                .record_metric(&repeat.id, "accuracy", MetricValue::Number(value))
                .unwrap();
        }
        store
            .rerun_gate(&original.id, "accuracy", &[repeat.id.as_str()])
            .unwrap()
    }

    #[test]
    fn a_reproduced_anomaly_lands_in_the_ledger() {
        let (_dir, store) = open();
        let verdict = gated(&store, Some(0.51));
        assert_eq!(verdict.outcome, GateOutcome::Reproduced);

        let anomaly = store.record_gate_verdict(&verdict).unwrap().unwrap();
        assert_eq!(anomaly.status, AnomalyStatus::Unexplained);
        assert_eq!(anomaly.gate_outcome, GateOutcome::Reproduced);
        assert_eq!(anomaly.observed_value, 0.50);
        assert_eq!(anomaly.explanation, None);
        assert_eq!(store.anomalies_for_track("t1").unwrap(), vec![anomaly]);
    }

    /// The counting rule. A rejected candidate leaves no ledger row and
    /// a `gate_runs` row, because a rejection nobody recorded cannot be
    /// counted afterwards.
    #[test]
    fn a_transient_candidate_is_counted_and_not_admitted() {
        let (_dir, store) = open();
        let verdict = gated(&store, Some(0.80));
        assert_eq!(verdict.outcome, GateOutcome::Transient);

        assert_eq!(store.record_gate_verdict(&verdict).unwrap(), None);
        assert!(store.anomalies_for_track("t1").unwrap().is_empty());

        let report = store.noise_report().unwrap();
        assert_eq!(report.transient, 1);
        assert_eq!(report.total(), 1);
        assert_eq!(report.admitted(), 0);
    }

    #[test]
    fn a_volatile_candidate_is_counted_and_not_admitted() {
        let (_dir, store) = open();
        let verdict = gated(&store, Some(0.99));
        assert_eq!(verdict.outcome, GateOutcome::Volatile);

        assert_eq!(store.record_gate_verdict(&verdict).unwrap(), None);
        assert!(store.anomalies_for_track("t1").unwrap().is_empty());
        assert_eq!(store.noise_report().unwrap().volatile, 1);
    }

    /// A replay that could not be performed is admitted and flagged.
    /// Refusing to look is not evidence that there was nothing to see.
    #[test]
    fn an_unverifiable_candidate_is_admitted_and_flagged() {
        let (_dir, store) = open();
        let verdict = gated(&store, None);
        assert_eq!(verdict.outcome, GateOutcome::Unverifiable);

        let anomaly = store.record_gate_verdict(&verdict).unwrap().unwrap();
        assert_eq!(anomaly.gate_outcome, GateOutcome::Unverifiable);
        assert_eq!(store.noise_report().unwrap().unverifiable, 1);
    }

    /// The noise rate is what says whether the environment is quiet
    /// enough for an admission to mean anything.
    #[test]
    fn the_noise_rate_counts_rejections_over_every_gate_run() {
        let (_dir, store) = open();
        for value in [Some(0.51), Some(0.80), Some(0.99)] {
            let verdict = gated(&store, value);
            store.record_gate_verdict(&verdict).unwrap();
        }
        let report = store.noise_report().unwrap();
        assert_eq!(report.reproduced, 1);
        assert_eq!(report.transient, 1);
        assert_eq!(report.volatile, 1);
        assert_eq!(report.total(), 3);
        assert!((report.noise_rate().unwrap() - 2.0 / 3.0).abs() < 1e-12);
    }

    /// Zero runs is not a noise rate of zero. Reporting 0.0 for a gate
    /// that has never run would read as the best possible result when
    /// it is the absence of any result.
    #[test]
    fn a_gate_that_never_ran_has_no_noise_rate() {
        let (_dir, store) = open();
        assert_eq!(store.noise_report().unwrap().noise_rate(), None);
    }

    /// An unverifiable replay says nothing about how noisy the
    /// measurement is. Counting it as noise would let a broken harness
    /// look like a clean one.
    #[test]
    fn unverifiable_runs_are_not_counted_as_noise() {
        let (_dir, store) = open();
        for value in [Some(0.51), None] {
            let verdict = gated(&store, value);
            store.record_gate_verdict(&verdict).unwrap();
        }
        let report = store.noise_report().unwrap();
        assert_eq!(report.unverifiable, 1);
        assert_eq!(report.noise_rate(), Some(0.0));
        assert_eq!(report.admitted(), 2);
    }

    #[test]
    fn an_explanation_moves_a_row_to_explained_and_is_stored() {
        let (_dir, store) = open();
        let verdict = gated(&store, Some(0.51));
        let anomaly = store.record_gate_verdict(&verdict).unwrap().unwrap();

        let updated = store
            .set_anomaly_status(
                &anomaly.id,
                AnomalyStatus::Explained,
                Some("the eval harness truncated the prompt at 8k"),
            )
            .unwrap();
        assert_eq!(updated.status, AnomalyStatus::Explained);
        assert_eq!(
            updated.explanation.as_deref(),
            Some("the eval harness truncated the prompt at 8k")
        );
        assert_eq!(updated.id, anomaly.id);
    }

    /// `explained` is a claim that the deviation has been accounted
    /// for. A claim with nothing behind it is exactly the failure the
    /// ledger exists to prevent.
    #[test]
    fn explained_with_no_explanation_is_refused() {
        let (_dir, store) = open();
        let verdict = gated(&store, Some(0.51));
        let anomaly = store.record_gate_verdict(&verdict).unwrap().unwrap();

        let err = store
            .set_anomaly_status(&anomaly.id, AnomalyStatus::Explained, None)
            .expect_err("explained with nothing behind it is not an explanation");
        assert!(err.to_string().contains("no explanation"), "{err}");
        assert_eq!(
            store.anomaly(&anomaly.id).unwrap().status,
            AnomalyStatus::Unexplained
        );
    }

    #[test]
    fn whitespace_is_not_an_explanation() {
        let (_dir, store) = open();
        let verdict = gated(&store, Some(0.51));
        let anomaly = store.record_gate_verdict(&verdict).unwrap().unwrap();

        store
            .set_anomaly_status(&anomaly.id, AnomalyStatus::Explained, Some("   \n  "))
            .expect_err("blank prose is not an explanation");
    }

    /// Superseding a row must not erase the account of what happened.
    #[test]
    fn superseding_keeps_the_explanation_already_recorded() {
        let (_dir, store) = open();
        let verdict = gated(&store, Some(0.51));
        let anomaly = store.record_gate_verdict(&verdict).unwrap().unwrap();
        store
            .set_anomaly_status(&anomaly.id, AnomalyStatus::Explained, Some("harness bug"))
            .unwrap();

        let updated = store
            .set_anomaly_status(&anomaly.id, AnomalyStatus::Superseded, None)
            .unwrap();
        assert_eq!(updated.status, AnomalyStatus::Superseded);
        assert_eq!(updated.explanation.as_deref(), Some("harness bug"));
    }

    /// Append-only. A status change rewrites one column of one row and
    /// adds nothing, removes nothing.
    #[test]
    fn a_status_change_deletes_no_rows() {
        let (_dir, store) = open();
        let verdict = gated(&store, Some(0.51));
        let anomaly = store.record_gate_verdict(&verdict).unwrap().unwrap();

        let before = table_counts(&store);
        store
            .set_anomaly_status(&anomaly.id, AnomalyStatus::Superseded, Some("merged"))
            .unwrap();
        assert_eq!(table_counts(&store), before);
    }

    /// Reading the ledger must not move anything out of `unexplained`.
    /// "Never a side effect of retrieval" is the spec's phrase and this
    /// is the test of it.
    #[test]
    fn reading_the_ledger_changes_nothing() {
        let (_dir, store) = open();
        let verdict = gated(&store, Some(0.51));
        let anomaly = store.record_gate_verdict(&verdict).unwrap().unwrap();

        let before = table_counts(&store);
        store.anomalies_for_track("t1").unwrap();
        store.anomaly(&anomaly.id).unwrap();
        store.noise_report().unwrap();
        assert_eq!(table_counts(&store), before);
        assert_eq!(
            store.anomaly(&anomaly.id).unwrap().status,
            AnomalyStatus::Unexplained
        );
    }

    #[test]
    fn an_unknown_anomaly_id_is_not_found() {
        let (_dir, store) = open();
        let err = store
            .set_anomaly_status("no-such-anomaly", AnomalyStatus::Superseded, None)
            .expect_err("an unknown id is not a silent no-op");
        assert!(err.to_string().contains("no-such-anomaly"), "{err}");
    }

    /// Corruption is refused rather than decoded to something
    /// plausible. This is the shape of the 2026-08-15 `TrackStatus`
    /// defect, checked here rather than assumed away.
    #[test]
    fn a_status_this_crate_never_wrote_is_refused_on_read() {
        let (_dir, store) = open();
        let verdict = gated(&store, Some(0.51));
        let anomaly = store.record_gate_verdict(&verdict).unwrap().unwrap();
        store
            .conn
            .execute(
                "UPDATE anomalies SET status = 'probably fine' WHERE id = ?",
                duckdb::params![anomaly.id],
            )
            .unwrap();

        let err = store
            .anomaly(&anomaly.id)
            .expect_err("an unknown status must not decode to unexplained");
        assert!(err.to_string().contains("probably fine"), "{err}");
    }

    #[test]
    fn every_status_round_trips_through_its_string() {
        for status in [
            AnomalyStatus::Unexplained,
            AnomalyStatus::Explained,
            AnomalyStatus::Superseded,
        ] {
            assert_eq!(AnomalyStatus::parse(status.as_str()).unwrap(), status);
        }
    }

    /// Two admitted anomalies on one track get distinct ids and
    /// consecutive seqs, so the ledger has a defined order even when
    /// both land in the same millisecond.
    #[test]
    fn the_ledger_orders_rows_within_a_millisecond() {
        let (_dir, store) = open();
        let first = gated(&store, Some(0.51));
        let second = gated(&store, Some(0.52));
        let a = store.record_gate_verdict(&first).unwrap().unwrap();
        let b = store.record_gate_verdict(&second).unwrap().unwrap();

        assert_ne!(a.id, b.id);
        assert_eq!(a.seq, 0);
        assert_eq!(b.seq, 1);
        let ledger = store.anomalies_for_track("t1").unwrap();
        assert_eq!(ledger.len(), 2);
        assert_eq!(ledger[0].id, a.id);
        assert_eq!(ledger[1].id, b.id);
    }

    /// Integrity rule 5, from the ledger's side. `explanation` may be
    /// read here and nowhere else, so no detector query is allowed to
    /// name it. The detector module owns the list; this asserts the
    /// column is on it.
    #[test]
    fn explanation_is_on_the_model_authored_list() {
        assert!(crate::detectors::MODEL_AUTHORED_COLUMNS.contains(&"explanation"));
    }
}
